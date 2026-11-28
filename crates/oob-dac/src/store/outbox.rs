//! The outbox (r6 F6-5): precious first, derived on replay.
//!
//! A precious change with derived consequences writes its row **and** an
//! [`Event`] in one transaction ([`Tx::append_event`]). After commit, each
//! [`Deriver`] folds the events past its watermark, applying the derived
//! changes and advancing the watermark in one transaction per batch. A
//! crash between the two leaves events unapplied; the next write or open
//! replays them. [`Deriver::apply`] is idempotent per event, so replay is
//! safe even if a watermark were lost.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{StoreError, Tx};

/// A precious event. Stored as JSON in `event_log.payload`, with the
/// variant name in `event_log.kind`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Event {
    /// A terminal record was created (Define Terminal).
    TerminalCreated {
        /// The store-allocated 24-bit terminal ID.
        terminal_id: u32,
    },
    /// A configuration object got a new revision (#25): a save, a rollback
    /// or the first-run seed. The applier (#29) and the Console's live view
    /// consume it.
    ConfigSaved {
        /// The object kind (`lineup`, `pid_plan`, `class_budgets`,
        /// `mux_settings` or `unit`). Not `kind`: that is the serde tag.
        config_kind: String,
        /// The terminal ID for a unit; absent for a singleton.
        object_id: Option<u32>,
        /// The new revision.
        revision: u32,
    },
}

impl Event {
    /// The `event_log.kind` string.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::TerminalCreated { .. } => "TerminalCreated",
            Self::ConfigSaved { .. } => "ConfigSaved",
        }
    }

    /// The schema version that introduced this kind (QA-3).
    ///
    /// **Rule:** a new `Event` variant ships with a new migration and names
    /// that migration's number here, so an older build meets the newer file
    /// as [`StoreError::SchemaTooNew`] instead of an undecodable event. The
    /// test `event_kinds_are_pinned_to_schema_versions` keeps the
    /// [`EVENT_KINDS`] table and this match in step, and freezes the kinds
    /// of every released schema version (QA-9).
    pub fn introduced_in(&self) -> u32 {
        match self {
            Self::TerminalCreated { .. } => 1,
            Self::ConfigSaved { .. } => 3,
        }
    }
}

/// Every event kind, by the schema version that introduced it. Entries for
/// released versions never change; a new kind needs a new version.
pub const EVENT_KINDS: &[(u32, &[&str])] = &[(1, &["TerminalCreated"]), (3, &["ConfigSaved"])];

/// An `event_log` sequence number (monotonic, never reused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventSeq(pub(crate) i64);

impl EventSeq {
    /// The raw sequence number.
    pub fn get(self) -> i64 {
        self.0
    }
}

/// Folds `event_log` into derived tables. One per group of derived tables.
pub trait Deriver: Send {
    /// Stable name; the key in `derive_watermark`.
    fn name(&self) -> &'static str;
    /// The derived tables (or rows, see [`TerminalStatusDeriver`]) it writes.
    fn tables(&self) -> &'static [&'static str];
    /// Applies one event. Must be idempotent: applying the same `seq` twice
    /// leaves the derived state unchanged.
    fn apply(&self, tx: &Tx<'_>, seq: EventSeq, ev: &Event) -> Result<(), StoreError>;
}

/// Seeds a `terminal_status` row (no observations yet) when a terminal is
/// created. The table is observed; only the row's existence comes from an
/// event, so rebuild recreates the table empty and replay re-seeds the rows.
pub struct TerminalStatusDeriver;

impl Deriver for TerminalStatusDeriver {
    fn name(&self) -> &'static str {
        "terminal_status"
    }
    fn tables(&self) -> &'static [&'static str] {
        &["terminal_status"]
    }
    fn apply(&self, tx: &Tx<'_>, _seq: EventSeq, ev: &Event) -> Result<(), StoreError> {
        match ev {
            Event::TerminalCreated { terminal_id } => {
                tx.conn().execute(
                    "INSERT OR IGNORE INTO terminal_status (terminal_id) VALUES (?1)",
                    [terminal_id],
                )?;
            }
            // No derived table folds configuration yet; the applier (#29)
            // reads the revision itself.
            Event::ConfigSaved { .. } => {}
        }
        Ok(())
    }
}

/// The derivers every store runs, in order.
pub(crate) fn builtin_derivers() -> Vec<Box<dyn Deriver>> {
    vec![Box::new(TerminalStatusDeriver)]
}

/// Events replayed per derivation transaction.
pub(crate) const BATCH: i64 = 256;

impl Tx<'_> {
    /// Appends `ev` to `event_log` inside this transaction (precious write
    /// and event commit together, or not at all).
    pub fn append_event(&self, actor: &str, ev: &Event) -> Result<EventSeq, StoreError> {
        let payload = serde_json::to_string(ev)?;
        let seq = self.conn().query_row(
            "INSERT INTO event_log (kind, payload, actor) VALUES (?1, ?2, ?3) RETURNING seq",
            params![ev.kind(), payload, actor],
            |r| r.get(0),
        )?;
        Ok(EventSeq(seq))
    }

    /// The watermark of `deriver` (0 when it has never run).
    pub(crate) fn watermark(&self, deriver: &str) -> Result<i64, StoreError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT applied_seq FROM derive_watermark WHERE deriver = ?1",
                [deriver],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub(crate) fn set_watermark(&self, deriver: &str, seq: i64) -> Result<(), StoreError> {
        self.conn().execute(
            "INSERT INTO derive_watermark (deriver, applied_seq) VALUES (?1, ?2) \
             ON CONFLICT (deriver) DO UPDATE SET applied_seq = excluded.applied_seq",
            params![deriver, seq],
        )?;
        Ok(())
    }

    /// Up to [`BATCH`] events after `after`, in order.
    pub(crate) fn events_after(&self, after: i64) -> Result<Vec<(EventSeq, Event)>, StoreError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT seq, payload FROM event_log WHERE seq > ?1 ORDER BY seq LIMIT ?2")?;
        let rows = stmt.query_map(params![after, BATCH], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, payload) = row?;
            out.push((EventSeq(seq), serde_json::from_str(&payload)?));
        }
        Ok(out)
    }
}
