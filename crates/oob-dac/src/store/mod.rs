//! The oob-dac store: **one sqlite file** holding every table oob-dac owns
//! (r6 F6-5, ADR-002 decision 1, ADR-003 §6 — the DB is the installation).
//!
//! - **Engine.** `rusqlite` with bundled sqlite (one pinned sqlite for every
//!   install). One writer: the [`Store`] owns the only read-write
//!   connection and holds an exclusive lock on `<db>.lock` for its lifetime,
//!   so a second open fails; write transactions are `BEGIN IMMEDIATE`.
//!   Async callers reach it through a [`StoreHandle`] backed by a dedicated
//!   writer thread. Readers use separate read-only connections
//!   ([`Store::reader`]), which WAL runs alongside the writer.
//! - **Pragmas.** `journal_mode=WAL`, `synchronous=FULL` (precious ledgers
//!   survive power loss), `foreign_keys=ON`, `recursive_triggers=ON`, each
//!   read back on open; a busy timeout; and a `quick_check` on open
//!   ([`StoreError::Corrupt`]).
//! - **Exactly one database.** Every connection is opened with
//!   `SQLITE_LIMIT_ATTACHED = 0`, so `ATTACH` is impossible by construction.
//! - **Classification.** Every table is [`Precious`](Durability::Precious),
//!   [`DerivedFromEvents`](Durability::DerivedFromEvents) or
//!   [`DerivedObserved`](Durability::DerivedObserved) in [`TABLES`].
//! - **Outbox.** Precious writes and their [`Event`]s commit together;
//!   [`Deriver`]s fold the events into derived tables afterwards, idempotently,
//!   and [`Store::rebuild_derived`] drops and rebuilds every derived table.
//!   A committed write always returns its value; derivation lag and errors
//!   are reported by [`Store::derivation_health`], never by failing a write
//!   or an open.
//! - **Migrations.** Forward-only, on `PRAGMA user_version`; a newer file is
//!   refused ([`StoreError::SchemaTooNew`]).
//!
//! ```
//! use oob_dac::store::{NewTerminal, Store, StoreOptions, TerminalType};
//!
//! # let dir = std::env::temp_dir().join(format!("oob-dac-doc-{}", std::process::id()));
//! # std::fs::create_dir_all(&dir)?;
//! let mut store = Store::open(&dir.join("oob-dac.sqlite"), &StoreOptions::default())?;
//! let ua = "000-00000-00001-000".parse()?; // synthetic, not a real box
//! let id = store.write(|tx| {
//!     // The terminal row and its TerminalCreated event commit together.
//!     tx.create_terminal(
//!         "marshal",
//!         &NewTerminal::new(ua, "GI0000000001", TerminalType::OneWay, "dct2000"),
//!     )
//! })?;
//! assert_eq!(id.get(), 1); // allocated by the store, never supplied
//! assert!(store.reader()?.has_status_row(id)?); // derived from the event
//! # drop(store);
//! # std::fs::remove_dir_all(&dir)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod audit;
mod classify;
mod error;
mod handle;
mod migrate;
mod outbox;
mod terminal;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::limits::Limit;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};

pub use audit::{AuditEntry, AuditRecord};
pub use classify::{BOX_IDENTIFYING, Durability, TABLES, TableClass, class_of};
pub use error::{ConstraintKind, StoreError};
pub use handle::StoreHandle;
pub use migrate::SCHEMA_VERSION;
pub use outbox::{Deriver, EVENT_KINDS, Event, EventSeq, TerminalStatusDeriver};
pub use terminal::{NewTerminal, TERMINAL_ID_MAX, Terminal, TerminalId, TerminalType, UnitAddress};

/// Install settings for the store (ADR-003 Layer 1, read from the daemon's
/// configuration in #29).
#[derive(Debug, Clone)]
pub struct StoreOptions {
    /// How long a connection waits on a lock before failing.
    pub busy_timeout: Duration,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            busy_timeout: Duration::from_secs(5),
        }
    }
}

/// A write transaction. Dropping it without commit rolls it back; the
/// store commits it when the closure passed to [`Store::write`] returns
/// `Ok`.
pub struct Tx<'c> {
    inner: rusqlite::Transaction<'c>,
}

impl Tx<'_> {
    pub(crate) fn conn(&self) -> &Connection {
        &self.inner
    }

    fn commit(self) -> Result<(), StoreError> {
        Ok(self.inner.commit()?)
    }
}

/// What [`Store::rebuild_derived`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildReport {
    /// The derived tables dropped and recreated.
    pub tables: Vec<&'static str>,
    /// Events replayed into them (summed over derivers).
    pub events_replayed: u64,
    /// Set when the replay failed after the recreate committed; the replay
    /// resumes on the next write or open.
    pub derivation_error: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// Panic after the precious commit, before derivation (a crash).
    PanicBeforeDerive,
    /// Every derivation attempt fails with an error.
    FailDerive,
}

/// How far derived state lags the precious log (QA-2, QA-3). A failing
/// deriver never fails a committed write or an open; it shows up here, for
/// the daemon's health check and the canary to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivationHealth {
    /// Events not yet applied by the slowest deriver.
    pub pending_events: u64,
    /// The last derivation error, cleared by the next successful pass.
    pub last_error: Option<String>,
}

/// The store: the single read-write connection to the one database file.
pub struct Store {
    conn: Connection,
    path: PathBuf,
    opts: StoreOptions,
    derivers: Vec<Box<dyn Deriver>>,
    derive_error: Option<String>,
    /// Held for the store's lifetime: one writer per file (QA-4).
    _lock: std::fs::File,
    #[cfg(test)]
    fault: Option<Fault>,
}

fn open_err(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> StoreError {
    StoreError::Open(std::io::Error::other(e))
}

/// The writer lock file next to the database: `<db>.lock`.
fn lock_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

/// Makes `ATTACH` impossible on `conn` (QA-7) and checks it took.
fn forbid_attach(conn: &Connection) -> Result<(), StoreError> {
    conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
    if conn.limit(Limit::SQLITE_LIMIT_ATTACHED)? != 0 {
        return Err(open_err("could not forbid ATTACH"));
    }
    Ok(())
}

/// Sets the writer's pragmas and reads each one back (QA-5): WAL,
/// `synchronous=FULL`, `foreign_keys=ON`, `recursive_triggers=ON` (so
/// REPLACE fires the journals' delete triggers, QA-1).
pub(crate) fn configure_writer(conn: &Connection) -> Result<(), StoreError> {
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(open_err(format!("journal_mode is {mode}, not wal")));
    }
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "recursive_triggers", "ON")?;
    for (pragma, want) in [
        ("synchronous", 2),
        ("foreign_keys", 1),
        ("recursive_triggers", 1),
    ] {
        let got: i64 = conn.query_row(&format!("PRAGMA {pragma}"), [], |r| r.get(0))?;
        if got != want {
            return Err(open_err(format!("PRAGMA {pragma} is {got}, not {want}")));
        }
    }
    Ok(())
}

impl Store {
    /// Opens (creating if absent) the store at `path`, checks it, migrates it
    /// to [`SCHEMA_VERSION`], and replays any events not yet derived.
    ///
    /// Only one `Store` may hold a file: a second open while the first is
    /// alive fails with [`StoreError::Open`]. A derivation failure does not
    /// fail the open; it is reported by [`derivation_health`](Self::derivation_health).
    pub fn open(path: &Path, opts: &StoreOptions) -> Result<Store, StoreError> {
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path(path))
            .map_err(StoreError::Open)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(open_err(format!(
                    "{} is already open by another writer",
                    path.display()
                )));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(StoreError::Open(e)),
        }
        let mut conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(open_err)?;
        conn.busy_timeout(opts.busy_timeout)?;
        forbid_attach(&conn)?;
        quick_check(&conn)?;
        configure_writer(&conn)?;
        migrate::migrate(&mut conn)?;
        let mut store = Store {
            conn,
            path: path.to_owned(),
            opts: opts.clone(),
            derivers: outbox::builtin_derivers(),
            derive_error: None,
            _lock: lock,
            #[cfg(test)]
            fault: None,
        };
        // Replay what a crash left behind; a failure is reported, not fatal.
        let _ = store.derive_pending();
        Ok(store)
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `f` in one write transaction (`BEGIN IMMEDIATE`) and commits
    /// it, then derives the consequences of any events it appended.
    ///
    /// If `f` fails, nothing is written. Once the commit succeeds the result
    /// is `Ok(value)`: the precious write is the outcome. A derivation
    /// failure after the commit is recorded in
    /// [`derivation_health`](Self::derivation_health), and the next write or
    /// open replays the pending events.
    pub fn write<T>(
        &mut self,
        f: impl FnOnce(&Tx<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let tx = Tx {
            inner: self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?,
        };
        let value = f(&tx)?;
        tx.commit()?;
        #[cfg(test)]
        if self.fault == Some(Fault::PanicBeforeDerive) {
            panic!("injected crash between the precious commit and derivation");
        }
        let _ = self.derive_pending();
        Ok(value)
    }

    /// How far derived state lags, and the last derivation error.
    pub fn derivation_health(&self) -> Result<DerivationHealth, StoreError> {
        let mut pending = 0u64;
        for d in &self.derivers {
            let n: i64 = self.conn.query_row(
                "SELECT count(*) FROM event_log WHERE seq > coalesce(\
                 (SELECT applied_seq FROM derive_watermark WHERE deriver = ?1), 0)",
                [d.name()],
                |r| r.get(0),
            )?;
            pending = pending.max(n as u64);
        }
        Ok(DerivationHealth {
            pending_events: pending,
            last_error: self.derive_error.clone(),
        })
    }

    /// Folds every event past each deriver's watermark into its derived
    /// tables, one transaction per batch. Returns the number of
    /// (deriver, event) applications. The outcome is also recorded for
    /// [`derivation_health`](Self::derivation_health).
    pub fn derive_pending(&mut self) -> Result<u64, StoreError> {
        let result = self.derive_pass();
        self.derive_error = result.as_ref().err().map(ToString::to_string);
        result
    }

    fn derive_pass(&mut self) -> Result<u64, StoreError> {
        let mut applied = 0;
        for d in &self.derivers {
            loop {
                let tx = Tx {
                    inner: self
                        .conn
                        .transaction_with_behavior(TransactionBehavior::Immediate)?,
                };
                let from = tx.watermark(d.name())?;
                let batch = tx.events_after(from)?;
                let Some(&(last, _)) = batch.last() else {
                    break;
                };
                for (seq, ev) in &batch {
                    #[cfg(test)]
                    if self.fault == Some(Fault::FailDerive) {
                        return Err(StoreError::Corrupt("injected derivation failure".into()));
                    }
                    d.apply(&tx, *seq, ev)?;
                }
                tx.set_watermark(d.name(), last.get())?;
                tx.commit()?;
                applied += batch.len() as u64;
            }
        }
        Ok(applied)
    }

    /// Drops and recreates every derived table, then replays `event_log`
    /// from the start (used by restore, #24). `DerivedFromEvents` tables are
    /// rebuilt exactly; `DerivedObserved` tables come back with no
    /// observations; `Precious` tables are never touched.
    ///
    /// Once the drop-and-recreate commits, a replay failure does not fail
    /// the rebuild: it is reported in [`RebuildReport::derivation_error`] and
    /// [`derivation_health`](Self::derivation_health), and later writes or
    /// opens continue the replay.
    pub fn rebuild_derived(&mut self) -> Result<RebuildReport, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut tables = Vec::new();
        for class in TABLES
            .iter()
            .filter(|t| t.durability != Durability::Precious)
        {
            // The table's own DDL first, then its indexes and triggers.
            let ddl: Vec<String> = {
                let mut stmt = tx.prepare(
                    "SELECT sql FROM sqlite_schema WHERE tbl_name = ?1 AND sql IS NOT NULL \
                     ORDER BY type <> 'table', name",
                )?;
                stmt.query_map([class.name], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            if ddl.is_empty() {
                return Err(StoreError::Corrupt(format!(
                    "classified table {} is missing",
                    class.name
                )));
            }
            tx.execute_batch(&format!("DROP TABLE \"{}\"", class.name))?;
            for sql in &ddl {
                tx.execute_batch(sql)?;
            }
            tables.push(class.name);
        }
        tx.commit()?;
        let (events_replayed, derivation_error) = match self.derive_pending() {
            Ok(n) => (n, None),
            Err(e) => (0, Some(e.to_string())),
        };
        Ok(RebuildReport {
            tables,
            events_replayed,
            derivation_error,
        })
    }

    /// Opens a read-only connection to the same file (`ATTACH` forbidden).
    pub fn reader(&self) -> Result<Reader, StoreError> {
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(open_err)?;
        conn.busy_timeout(self.opts.busy_timeout)?;
        forbid_attach(&conn)?;
        Ok(Reader { conn })
    }
}

fn quick_check(conn: &Connection) -> Result<(), StoreError> {
    let result: rusqlite::Result<String> = conn.query_row("PRAGMA quick_check", [], |r| r.get(0));
    match result {
        Ok(s) if s == "ok" => Ok(()),
        Ok(s) => Err(StoreError::Corrupt(s)),
        Err(rusqlite::Error::SqliteFailure(e, msg))
            if matches!(
                e.code,
                rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt
            ) =>
        {
            Err(StoreError::Corrupt(msg.unwrap_or_else(|| e.to_string())))
        }
        Err(e) => Err(e.into()),
    }
}

/// A read-only connection to the store file.
pub struct Reader {
    conn: Connection,
}

impl Reader {
    /// One terminal record.
    pub fn terminal(&self, id: TerminalId) -> Result<Option<Terminal>, StoreError> {
        terminal::terminal(&self.conn, id)
    }

    /// Every terminal record, by ID.
    pub fn terminals(&self) -> Result<Vec<Terminal>, StoreError> {
        terminal::terminals(&self.conn)
    }

    /// Every audit record, by sequence number.
    pub fn audit_records(&self) -> Result<Vec<AuditRecord>, StoreError> {
        audit::audit_records(&self.conn)
    }

    /// Whether `id` has a `terminal_status` row (seeded by its
    /// `TerminalCreated` event).
    pub fn has_status_row(&self, id: TerminalId) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM terminal_status WHERE terminal_id = ?1",
                [id.get()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
