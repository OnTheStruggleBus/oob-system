//! Store tests: one per #21 acceptance criterion and design-note test-plan
//! item. Every test uses its own temp-dir database.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use super::*;

/// A throwaway directory removed on drop (no dev-dependency needed).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "oob-dac-store-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn db(&self) -> PathBuf {
        self.0.join("oob-dac.sqlite")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(path: &Path) -> Store {
    Store::open(path, &StoreOptions::default()).unwrap()
}

/// A valid terminal whose unit address and serial are unique per `n`.
fn sample(n: u64) -> NewTerminal {
    let mut t = NewTerminal::new(
        UnitAddress::new(n, 0).unwrap(), // synthetic
        format!("GI{n:010}"),
        TerminalType::OneWay,
        "dct2000",
    );
    t.account_number = Some(format!("ACCT{n}"));
    t
}

fn create(store: &mut Store, n: u64) -> Result<TerminalId, StoreError> {
    store.write(|tx| tx.create_terminal("test", &sample(n)))
}

/// Every row of `table`, rendered, in key order.
fn dump(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY 1"))
        .unwrap();
    let cols = stmt.column_count();
    stmt.query_map([], |r| {
        let mut row = String::new();
        for i in 0..cols {
            row.push_str(&format!("{:?}|", r.get_ref(i)?));
        }
        Ok(row)
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
        r.get(0)
    })
    .unwrap()
}

fn tables_of(conn: &Connection) -> BTreeSet<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

// --- AC: exactly one database file; WAL; no ATTACHed second database ---

#[test]
fn one_file_wal_full_no_attach() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    create(&mut store, 1).unwrap();
    store.rebuild_derived().unwrap();
    let _reader = store.reader().unwrap();

    let dbs: Vec<String> = {
        let mut stmt = store.conn.prepare("PRAGMA database_list").unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert!(
        dbs.iter().all(|d| d == "main" || d == "temp"),
        "attached databases: {dbs:?}"
    );
    assert!(dbs.contains(&"main".to_string()));

    let mode: String = store
        .conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let sync: i64 = store
        .conn
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sync, 2, "synchronous must be FULL");
    let fk: i64 = store
        .conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1);

    // The directory holds the one database, its WAL companions and the
    // writer lock only.
    let files: BTreeSet<String> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    for f in &files {
        assert!(f.starts_with("oob-dac.sqlite"), "unexpected file {f}");
    }
}

#[test]
fn attach_is_impossible_on_writer_and_reader() {
    // QA-7: SQLITE_LIMIT_ATTACHED = 0 on every connection.
    let dir = TempDir::new();
    let store = open(&dir.db());
    let reader = store.reader().unwrap();
    for conn in [&store.conn, &reader.conn] {
        assert!(
            conn.execute_batch("ATTACH ':memory:' AS x").is_err(),
            "ATTACH must fail"
        );
        assert_eq!(conn.limit(Limit::SQLITE_LIMIT_ATTACHED).unwrap(), 0);
    }
}

#[test]
fn configure_writer_sets_and_verifies_pragmas() {
    // QA-5: start from the opposite of what the store needs.
    let dir = TempDir::new();
    let conn = Connection::open(dir.db()).unwrap();
    conn.pragma_update(None, "synchronous", "NORMAL").unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.pragma_update(None, "recursive_triggers", "OFF")
        .unwrap();
    configure_writer(&conn).unwrap();
    for (pragma, want) in [
        ("synchronous", 2i64),
        ("foreign_keys", 1),
        ("recursive_triggers", 1),
    ] {
        let got: i64 = conn
            .query_row(&format!("PRAGMA {pragma}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(got, want, "{pragma}");
    }
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn second_writer_is_refused() {
    // QA-4: one Store per file; the lock is released on drop.
    let dir = TempDir::new();
    let first = open(&dir.db());
    assert!(matches!(
        Store::open(&dir.db(), &StoreOptions::default()),
        Err(StoreError::Open(_))
    ));
    drop(first);
    drop(open(&dir.db()));
}

// --- AC: a classification registry covers every table ---

#[test]
fn registry_matches_schema_exactly() {
    let dir = TempDir::new();
    let store = open(&dir.db());
    let actual = tables_of(&store.conn);
    let registered: BTreeSet<String> = TABLES.iter().map(|t| t.name.to_string()).collect();
    let unclassified: Vec<_> = actual.difference(&registered).collect();
    let stale: Vec<_> = registered.difference(&actual).collect();
    assert!(
        unclassified.is_empty(),
        "unclassified tables: {unclassified:?}"
    );
    assert!(
        stale.is_empty(),
        "registry entries with no table: {stale:?}"
    );
    assert_eq!(registered.len(), TABLES.len(), "duplicate registry entry");
}

#[test]
fn append_only_tables_are_precious_and_guarded() {
    let dir = TempDir::new();
    let store = open(&dir.db());
    for t in TABLES.iter().filter(|t| t.append_only) {
        assert_eq!(t.durability, Durability::Precious, "{}", t.name);
        for op in ["update", "delete", "overwrite"] {
            let trig = format!("{}_no_{op}", t.name);
            let n: i64 = store
                .conn
                .query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type = 'trigger' AND name = ?1 \
                     AND tbl_name = ?2",
                    [&trig, t.name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing trigger {trig}");
        }
    }
}

#[test]
fn derivers_write_only_classified_derived_tables() {
    for d in outbox::builtin_derivers() {
        for t in d.tables() {
            let class = class_of(t).unwrap_or_else(|| panic!("{t} unclassified"));
            assert_ne!(class.durability, Durability::Precious, "{t}");
        }
    }
}

// --- AC: migrations versioned from the first table ---

#[test]
fn empty_db_migrates_to_head() {
    assert_eq!(SCHEMA_VERSION, 2);
    for (i, (n, _)) in migrate::MIGRATIONS.iter().enumerate() {
        assert_eq!(*n as usize, i + 1, "migrations must be contiguous from 1");
    }
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    assert_eq!(migrate::user_version(&store.conn).unwrap(), SCHEMA_VERSION);

    // Golden schema: every object the migrations create.
    let mut stmt = store
        .conn
        .prepare(
            "SELECT type || ' ' || name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' \
             ORDER BY type, name",
        )
        .unwrap();
    let objects: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    drop(stmt);
    assert_eq!(
        objects,
        [
            "table audit_log",
            "table confirmation_token",
            "table derive_watermark",
            "table event_log",
            "table install_setting",
            "table terminal",
            "table terminal_status",
            "trigger audit_log_no_delete",
            "trigger audit_log_no_overwrite",
            "trigger audit_log_no_update",
            "trigger event_log_no_delete",
            "trigger event_log_no_overwrite",
            "trigger event_log_no_update",
        ]
    );

    // Running the migrations again is a no-op.
    assert_eq!(migrate::migrate(&mut store.conn).unwrap(), 0);
    drop(store);
    let store = open(&dir.db());
    assert_eq!(migrate::user_version(&store.conn).unwrap(), SCHEMA_VERSION);
}

#[test]
fn newer_schema_is_refused() {
    let dir = TempDir::new();
    drop(open(&dir.db()));
    let raw = Connection::open(dir.db()).unwrap();
    raw.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    drop(raw);
    match Store::open(&dir.db(), &StoreOptions::default()) {
        Err(StoreError::SchemaTooNew { found, supported }) => {
            assert_eq!(found, SCHEMA_VERSION + 1);
            assert_eq!(supported, SCHEMA_VERSION);
        }
        other => panic!("expected SchemaTooNew, got {:?}", other.err()),
    }
}

#[test]
fn failed_migration_rolls_back_to_previous_version() {
    // QA-6: migration 2 fails half-way; the file stays at version 1.
    let dir = TempDir::new();
    let mut conn = Connection::open(dir.db()).unwrap();
    let migrations: &[(u32, &str)] = &[
        (1, "CREATE TABLE a (x INTEGER) STRICT;"),
        (
            2,
            "CREATE TABLE x (a INTEGER) STRICT; CREATE TABLE x (a INTEGER) STRICT;",
        ),
    ];
    match migrate::migrate_with(&mut conn, migrations) {
        Err(StoreError::Migrate { to: 2, .. }) => {}
        other => panic!("expected Migrate {{ to: 2 }}, got {other:?}"),
    }
    assert_eq!(migrate::user_version(&conn).unwrap(), 1);
    let tables = tables_of(&conn);
    assert!(tables.contains("a"));
    assert!(!tables.contains("x"), "half-applied migration left table x");
}

#[test]
fn garbage_file_is_corrupt() {
    let dir = TempDir::new();
    std::fs::write(dir.db(), vec![0x5a; 8192]).unwrap();
    assert!(matches!(
        Store::open(&dir.db(), &StoreOptions::default()),
        Err(StoreError::Corrupt(_))
    ));
}

// --- AC: terminal table mirrors Define Terminal; never seeded from git ---

#[test]
fn terminal_round_trips_with_allocated_id() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let mut t = sample(7);
    t.terminal_type = TerminalType::FoneWay;
    t.on_plant = true;
    t.install_date = Some("2026-11-23".into());
    t.bsi_name = Some("bsi".into());
    t.downstream_plant = Some("DS1".into());
    t.upstream_plant = Some("US1".into());
    t.sacrificial = true;
    let id = store.write(|tx| tx.create_terminal("op", &t)).unwrap();
    assert_eq!(id.get(), 1);
    let id2 = create(&mut store, 8).unwrap();
    assert_eq!(id2.get(), 2);
    assert!((1..=TERMINAL_ID_MAX).contains(&id2.get()));

    let got = store.reader().unwrap().terminal(id).unwrap().unwrap();
    assert_eq!(got.id, id);
    assert_eq!(got.record, t);
    assert!(got.created_utc.ends_with('Z'));
    assert_eq!(store.reader().unwrap().terminals().unwrap().len(), 2);
}

#[test]
fn terminal_ids_are_never_reused() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let a = create(&mut store, 1).unwrap();
    // Remove the newest terminal directly (no delete API exists yet).
    store
        .conn
        .execute("DELETE FROM terminal WHERE terminal_id = ?1", [a.get()])
        .unwrap();
    let b = create(&mut store, 2).unwrap();
    assert!(b > a);
}

#[test]
fn duplicate_unit_address_and_serial_are_rejected_atomically() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    create(&mut store, 1).unwrap();

    let mut dup_ua = sample(2);
    dup_ua.unit_address = sample(1).unit_address;
    let err = store
        .write(|tx| tx.create_terminal("t", &dup_ua))
        .unwrap_err();
    assert!(matches!(
        err,
        StoreError::Constraint(ConstraintKind::DuplicateUnitAddress)
    ));

    let mut dup_serial = sample(3);
    dup_serial.serial = sample(1).serial;
    let err = store
        .write(|tx| tx.create_terminal("t", &dup_serial))
        .unwrap_err();
    assert!(matches!(
        err,
        StoreError::Constraint(ConstraintKind::DuplicateSerial)
    ));

    // Failed writes left neither a row nor an event behind.
    assert_eq!(count(&store.conn, "terminal"), 1);
    assert_eq!(count(&store.conn, "event_log"), 1);
}

#[test]
fn bad_fields_are_rejected() {
    assert!("Three-Way".parse::<TerminalType>().is_err());
    for ok in ["One-Way", "Fone-Way", "Two-Way", "Ethernet", "DOCSIS"] {
        assert_eq!(ok.parse::<TerminalType>().unwrap().as_str(), ok);
    }

    let dir = TempDir::new();
    let mut store = open(&dir.db());
    // The schema CHECK holds even if Rust validation were bypassed.
    let err = store
        .write(|tx| {
            tx.conn().execute(
                "INSERT INTO terminal (unit_address, ua_check, serial, terminal_type, box_type) \
                 VALUES (1, 1, 'ABCDEFGHIJKL', 'Three-Way', 'dct2000')",
                [],
            )?;
            Ok(())
        })
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Constraint(ConstraintKind::Check(_))),
        "{err:?}"
    );

    let mut short = sample(1);
    short.serial = "SHORT".into();
    assert!(matches!(
        store.write(|tx| tx.create_terminal("t", &short)),
        Err(StoreError::Constraint(ConstraintKind::Invalid(_)))
    ));

    // QA-8: dates and IP addresses are typed, in Rust and in the schema.
    let bad_records: Vec<NewTerminal> = vec![
        NewTerminal {
            install_date: Some("next tuesday".into()),
            ..sample(2)
        },
        NewTerminal {
            install_date: Some("2026-02-30".into()),
            ..sample(2)
        },
        NewTerminal {
            auto_registration_date: Some("2026-1-05".into()),
            ..sample(2)
        },
        NewTerminal {
            ip_address: Some("10.0.0.256".into()),
            ..sample(2)
        },
    ];
    for t in &bad_records {
        assert!(
            matches!(
                store.write(|tx| tx.create_terminal("t", t)),
                Err(StoreError::Constraint(ConstraintKind::Invalid(_)))
            ),
            "{t:?}"
        );
    }
    let good = NewTerminal {
        install_date: Some("2024-02-29".into()),
        auto_registration_date: Some("2026-11-23".into()),
        ip_address: Some("fd00::1".into()),
        ..sample(3)
    };
    store.write(|tx| tx.create_terminal("t", &good)).unwrap();
    for (col, val) in [
        ("install_date", "next tuesday"),
        ("install_date", "2026-02-30"),
        ("auto_registration_date", "2026-1-05"),
        ("ip_address", "not an ip"),
    ] {
        let err = store
            .write(|tx| {
                tx.conn().execute(
                    &format!(
                        "INSERT INTO terminal (unit_address, ua_check, serial, terminal_type, \
                         box_type, {col}) VALUES (9, 0, 'ABCDEFGHIJKL', 'One-Way', 'dct2000', ?1)"
                    ),
                    [val],
                )?;
                Ok(())
            })
            .unwrap_err();
        assert!(
            matches!(err, StoreError::Constraint(ConstraintKind::Check(_))),
            "{col}={val}: {err:?}"
        );
    }
}

#[test]
fn terminal_id_space_exhaustion_is_an_error() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    create(&mut store, 1).unwrap();
    store
        .conn
        .execute(
            "UPDATE sqlite_sequence SET seq = ?1 WHERE name = 'terminal'",
            [TERMINAL_ID_MAX],
        )
        .unwrap();
    assert!(matches!(
        create(&mut store, 2),
        Err(StoreError::Constraint(ConstraintKind::TerminalIdExhausted))
    ));
}

#[test]
fn unit_address_printed_form() {
    // Synthetic addresses only: real ones are box-identifying (ADR-003 §4).
    let ua: UnitAddress = "000-01234-56789-012".parse().unwrap();
    assert_eq!(ua.address(), 123_456_789);
    assert_eq!(ua.check(), 12);
    assert_eq!(ua.to_string(), "000-01234-56789-012");
    let max = UnitAddress::new(UnitAddress::MAX, 999).unwrap();
    assert_eq!(max.to_string(), "109-95116-27775-999");
    assert_eq!(max.to_string().parse::<UnitAddress>().unwrap(), max);
    for bad in [
        "109-95116-27776-000", // 2^40, one past 40 bits
        "000-01234-56789",
        "000-0123a-56789-012",
        "0000-1234-56789-012",
    ] {
        assert!(bad.parse::<UnitAddress>().is_err(), "{bad}");
    }
}

#[test]
fn no_box_instance_comes_from_git() {
    // ARCH-3: the rule is "no box instance", not "no rows" — migrations may
    // seed install settings (#23), never a box-identifying table. The store
    // allocates every terminal ID (NewTerminal has no ID field). The
    // seed-loader half (first-run seed load, #25) lands there.
    assert!(BOX_IDENTIFYING.iter().all(|t| class_of(t).is_some()));
    let dir = TempDir::new();
    let store = open(&dir.db());
    for table in BOX_IDENTIFYING {
        assert_eq!(count(&store.conn, table), 0, "{table}");
        for (n, sql) in migrate::MIGRATIONS {
            let flat = sql
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            for verb in ["insert into", "insert or replace into", "replace into"] {
                for quoted in [table.to_string(), format!("\"{table}\"")] {
                    for end in [" ", "("] {
                        assert!(
                            !flat.contains(&format!("{verb} {quoted}{end}")),
                            "migration {n} inserts into box-identifying table {table}"
                        );
                    }
                }
            }
        }
    }
}

// --- append-only journals ---

#[test]
fn journals_are_append_only() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    create(&mut store, 1).unwrap();
    store
        .write(|tx| {
            tx.append_audit(&AuditEntry {
                actor: "marshal".into(),
                role: "owner".into(),
                front_door: "test".into(),
                command: "define-terminal".into(),
                target: Some("1".into()),
                outcome: "ok".into(),
                before: None,
                after: Some("{\"id\":1}".into()),
            })
        })
        .unwrap();
    for (table, sql) in [
        ("event_log", "UPDATE event_log SET actor = 'x'"),
        ("event_log", "DELETE FROM event_log"),
        ("audit_log", "UPDATE audit_log SET outcome = 'x'"),
        ("audit_log", "DELETE FROM audit_log"),
        // QA-1: REPLACE deletes the old row without a DELETE statement.
        (
            "event_log",
            "INSERT OR REPLACE INTO event_log (seq, kind, payload, actor) \
             VALUES (1, 'TerminalCreated', '{\"kind\":\"TerminalCreated\",\"terminal_id\":1}', \
             'forged')",
        ),
        (
            "event_log",
            "REPLACE INTO event_log (seq, kind, payload, actor) \
             VALUES (1, 'TerminalCreated', '{}', 'forged')",
        ),
        (
            "audit_log",
            "REPLACE INTO audit_log (seq, actor, role, front_door, command, outcome) \
             VALUES (1, 'FORGED', 'owner', 'test', 'c', 'ok')",
        ),
        (
            "audit_log",
            "INSERT OR REPLACE INTO audit_log (seq, actor, role, front_door, command, outcome) \
             VALUES (1, 'FORGED', 'owner', 'test', 'c', 'ok')",
        ),
        (
            "event_log",
            "INSERT INTO event_log (seq, kind, payload, actor) VALUES (1, 'x', '{}', 'forged') \
             ON CONFLICT (seq) DO UPDATE SET actor = excluded.actor",
        ),
    ] {
        let err = store
            .write(|tx| {
                tx.conn().execute(sql, [])?;
                Ok(())
            })
            .unwrap_err();
        match err {
            StoreError::AppendOnly { table: t } => assert_eq!(t, table),
            other => panic!("{sql}: expected AppendOnly, got {other:?}"),
        }
    }
    let audit = store.reader().unwrap().audit_records().unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].entry.actor, "marshal");
    assert_eq!(audit[0].entry.outcome, "ok");
    let actor: String = store
        .conn
        .query_row("SELECT actor FROM event_log WHERE seq = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(actor, "test");
    // Ordinary appends still work after all that.
    create(&mut store, 2).unwrap();
}

// --- AC: outbox; crash injection between precious commit and derivation ---

fn derived_state(conn: &Connection) -> (Vec<String>, Vec<String>) {
    (
        dump(conn, "terminal_status"),
        dump(conn, "derive_watermark"),
    )
}

#[test]
fn crash_between_commit_and_derivation_is_repaired_on_reopen() {
    // Reference: a clean run of the same writes.
    let clean_dir = TempDir::new();
    let mut clean = open(&clean_dir.db());
    for n in 1..=3 {
        create(&mut clean, n).unwrap();
    }
    let expected = derived_state(&clean.conn);

    let dir = TempDir::new();
    let mut store = open(&dir.db());
    create(&mut store, 1).unwrap();
    create(&mut store, 2).unwrap();
    store.fault = Some(Fault::PanicBeforeDerive);
    let crashed = catch_unwind(AssertUnwindSafe(|| create(&mut store, 3)));
    assert!(crashed.is_err(), "the injected crash must fire");
    drop(store);

    // On disk: the precious row and its event committed; derivation lags.
    let raw = Connection::open(dir.db()).unwrap();
    assert_eq!(count(&raw, "terminal"), 3);
    assert_eq!(count(&raw, "event_log"), 3);
    assert_eq!(count(&raw, "terminal_status"), 2);
    drop(raw);

    let store = open(&dir.db());
    assert_eq!(derived_state(&store.conn), expected);
}

#[test]
fn derivation_failure_keeps_the_committed_value_and_reports_lag() {
    // QA-2 / ARCH-4: a committed write returns its value; lag is reported.
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    store.fault = Some(Fault::FailDerive);
    let a = create(&mut store, 1).expect("committed write must return Ok");
    assert_eq!(a.get(), 1);
    assert_eq!(count(&store.conn, "terminal"), 1);
    assert_eq!(count(&store.conn, "terminal_status"), 0);
    let health = store.derivation_health().unwrap();
    assert_eq!(health.pending_events, 1);
    assert!(health.last_error.is_some());

    // An audit append under the same fault is also Ok, exactly once.
    store
        .write(|tx| {
            tx.append_audit(&AuditEntry {
                actor: "a".into(),
                role: "owner".into(),
                front_door: "test".into(),
                command: "c".into(),
                target: None,
                outcome: "ok".into(),
                before: None,
                after: None,
            })
        })
        .unwrap();
    assert_eq!(count(&store.conn, "audit_log"), 1);

    store.fault = None;
    let b = create(&mut store, 2).unwrap();
    let reader = store.reader().unwrap();
    assert!(reader.has_status_row(a).unwrap());
    assert!(reader.has_status_row(b).unwrap());
    assert_eq!(
        store.derivation_health().unwrap(),
        DerivationHealth {
            pending_events: 0,
            last_error: None
        }
    );
}

#[test]
fn undecodable_event_does_not_brick_open() {
    // QA-3: an event kind this build does not know (a bug, or a newer build
    // that skipped the schema bump) is reported, never fatal to open.
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    create(&mut store, 1).unwrap();
    store
        .conn
        .execute(
            "INSERT INTO event_log (kind, payload, actor) VALUES ('Future', '{\"kind\":\"Future\"}', 'x')",
            [],
        )
        .unwrap();
    drop(store);

    let mut store = open(&dir.db());
    let health = store.derivation_health().unwrap();
    assert_eq!(health.pending_events, 1);
    assert!(
        health
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("Future"),
        "{health:?}"
    );
    // Precious data is still reachable and writable.
    assert_eq!(store.reader().unwrap().terminals().unwrap().len(), 1);
    create(&mut store, 2).unwrap();
    assert_eq!(count(&store.conn, "terminal"), 2);
}

/// Event kinds per **released** schema version (QA-9). Append a row when a
/// version ships; never edit an existing row. Version 1 ships with #21.
const RELEASED_EVENT_KINDS: &[(u32, &[&str])] = &[(1, &["TerminalCreated"])];

/// One sample of every `Event` variant. The exhaustive `match` (crate-local,
/// so `#[non_exhaustive]` does not apply) stops a new variant compiling until
/// it gets an arm here; the arm is where its sample goes.
fn all_event_samples() -> Vec<Event> {
    let samples = vec![Event::TerminalCreated { terminal_id: 1 }];
    for ev in &samples {
        match ev {
            Event::TerminalCreated { .. } => {}
        }
    }
    samples
}

/// The variant names serde accepts for `Event`, read from its
/// unknown-variant error ("expected `A`" / "expected one of `A`, `B`").
fn serde_variant_names() -> BTreeSet<&'static str> {
    let err = serde_json::from_str::<Event>(r#"{"kind":"__no_such_kind__"}"#)
        .unwrap_err()
        .to_string();
    let expected = err
        .split_once("expected")
        .unwrap_or_else(|| panic!("unexpected serde error: {err}"))
        .1;
    let names: BTreeSet<&'static str> = expected
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|s| &*Box::leak(s.to_string().into_boxed_str()))
        .collect();
    assert!(!names.is_empty(), "no variant names in: {err}");
    names
}

#[test]
fn event_kinds_are_pinned_to_schema_versions() {
    // QA-3 / QA-9: every Event variant names the migration that introduced
    // it, EVENT_KINDS lists exactly those kinds, and kinds of a released
    // schema version are frozen. A new variant without a new schema version
    // fails here (or fails to compile in `all_event_samples`).
    let samples = all_event_samples();

    // Every variant serde knows has a sample: a variant added without one
    // is caught even if its `match` arm was written.
    assert_eq!(
        samples.iter().map(Event::kind).collect::<BTreeSet<_>>(),
        serde_variant_names(),
        "all_event_samples must cover every Event variant"
    );

    // Frozen: a version's kinds never change once it is released.
    let frozen_max = RELEASED_EVENT_KINDS.last().map_or(0, |(v, _)| *v);
    for (version, frozen) in RELEASED_EVENT_KINDS {
        let now: BTreeSet<&str> = samples
            .iter()
            .filter(|e| e.introduced_in() == *version)
            .map(Event::kind)
            .collect();
        assert_eq!(
            now,
            frozen.iter().copied().collect::<BTreeSet<_>>(),
            "schema version {version} is released; its event kinds are frozen — \
             a new kind needs a new migration"
        );
    }
    // Anything outside the frozen list is new in the current, unreleased
    // schema version.
    for ev in &samples {
        if ev.introduced_in() > frozen_max {
            assert_eq!(ev.introduced_in(), SCHEMA_VERSION, "{}", ev.kind());
            assert!(
                SCHEMA_VERSION > frozen_max,
                "{} needs a new schema version",
                ev.kind()
            );
        }
    }

    let mut listed = BTreeSet::new();
    let mut last_version = 0;
    for (version, kinds) in EVENT_KINDS {
        assert!(*version > last_version, "EVENT_KINDS must be ascending");
        assert!(*version <= SCHEMA_VERSION);
        last_version = *version;
        for k in *kinds {
            assert!(listed.insert(*k), "{k} listed twice");
        }
    }
    let actual: BTreeSet<&str> = samples.iter().map(Event::kind).collect();
    assert_eq!(listed, actual, "EVENT_KINDS out of step with Event");
    for ev in &samples {
        let (v, kinds) = EVENT_KINDS
            .iter()
            .find(|(_, ks)| ks.contains(&ev.kind()))
            .unwrap();
        assert_eq!(*v, ev.introduced_in(), "{}", ev.kind());
        assert!(kinds.contains(&ev.kind()));
        // Round-trips through the stored JSON form.
        let json = serde_json::to_string(ev).unwrap();
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), *ev);
    }
}

#[test]
fn writes_begin_immediate() {
    // QA-4 follow-up: the write lock is taken at BEGIN, so a busy database
    // fails the write before the closure runs (a DEFERRED transaction would
    // run the closure and fail at its first write instead).
    let dir = TempDir::new();
    let opts = StoreOptions {
        busy_timeout: std::time::Duration::from_millis(50),
    };
    let mut store = Store::open(&dir.db(), &opts).unwrap();
    let other = Connection::open(dir.db()).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut ran = false;
    let result = store.write(|_tx| {
        ran = true;
        Ok(())
    });
    assert!(
        result.is_err(),
        "write must fail while another writer holds the lock"
    );
    assert!(!ran, "closure ran: the transaction did not begin IMMEDIATE");
    other.execute_batch("ROLLBACK").unwrap();
    store.write(|_tx| Ok(())).unwrap();
}

// --- AC: derived tables can be dropped and rebuilt from precious ones ---

#[test]
fn rebuild_derived_restores_derived_and_never_touches_precious() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    for n in 1..=5 {
        create(&mut store, n).unwrap();
    }
    store
        .write(|tx| {
            tx.append_audit(&AuditEntry {
                actor: "a".into(),
                role: "owner".into(),
                front_door: "test".into(),
                command: "c".into(),
                target: None,
                outcome: "ok".into(),
                before: None,
                after: None,
            })
        })
        .unwrap();
    // A poll observation (DerivedObserved) that rebuild must clear.
    store
        .conn
        .execute(
            "UPDATE terminal_status SET failures = 3, last_response_utc = '2026-11-23T00:00:00Z' \
             WHERE terminal_id = 2",
            [],
        )
        .unwrap();

    let precious = |c: &Connection| -> Vec<Vec<String>> {
        TABLES
            .iter()
            .filter(|t| t.durability == Durability::Precious)
            .map(|t| dump(c, t.name))
            .collect()
    };
    let from_events = |c: &Connection| -> Vec<Vec<String>> {
        TABLES
            .iter()
            .filter(|t| t.durability == Durability::DerivedFromEvents)
            .map(|t| dump(c, t.name))
            .collect()
    };
    let precious_before = precious(&store.conn);
    let from_events_before = from_events(&store.conn);

    let report = store.rebuild_derived().unwrap();
    assert_eq!(
        report.tables,
        ["terminal_status", "derive_watermark", "confirmation_token"]
    );
    assert_eq!(report.events_replayed, 5);
    assert_eq!(report.derivation_error, None);

    assert_eq!(precious(&store.conn), precious_before);
    assert_eq!(from_events(&store.conn), from_events_before);
    // Observed: every slot is back, with no observations.
    assert_eq!(count(&store.conn, "terminal_status"), 5);
    let observed: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM terminal_status WHERE last_response_utc IS NOT NULL \
             OR failures IS NOT NULL OR power_leveling_attempts IS NOT NULL \
             OR non_participating_count IS NOT NULL OR chronic_non_responding IS NOT NULL \
             OR d_screen IS NOT NULL OR observed_utc IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(observed, 0);
    // The registry still matches the schema after the rebuild.
    let registered: BTreeSet<String> = TABLES.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(tables_of(&store.conn), registered);
    // And the foreign key survived the recreate.
    let err = store
        .write(|tx| {
            tx.conn()
                .execute("INSERT INTO terminal_status (terminal_id) VALUES (999)", [])?;
            Ok(())
        })
        .unwrap_err();
    assert!(matches!(
        err,
        StoreError::Constraint(ConstraintKind::ForeignKey)
    ));
}

// --- idempotence: applying an event twice changes nothing ---

/// xorshift64*: a deterministic stand-in for a property-test generator.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

#[test]
fn replaying_events_is_idempotent() {
    for seed in 1..=16u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let dir = TempDir::new();
        let mut store = open(&dir.db());
        let n = 1 + rng.next() % 20;
        for i in 0..n {
            create(&mut store, seed * 1000 + i).unwrap();
        }
        let expected = derived_state(&store.conn);

        // Re-apply a random multiset of already-applied events directly.
        let events = store
            .write(|tx| {
                let evs = tx.events_after(0)?;
                let d = TerminalStatusDeriver;
                for _ in 0..(rng.next() % 40) {
                    let (seq, ev) = &evs[(rng.next() % evs.len() as u64) as usize];
                    d.apply(tx, *seq, ev)?;
                }
                Ok(evs.len())
            })
            .unwrap();
        assert_eq!(events as u64, n);
        assert_eq!(derived_state(&store.conn), expected, "seed {seed}");

        // A lost watermark replays everything; the result is unchanged.
        store
            .conn
            .execute("DELETE FROM derive_watermark", [])
            .unwrap();
        assert_eq!(store.derive_pending().unwrap(), n);
        assert_eq!(derived_state(&store.conn), expected, "seed {seed}");
    }
}

// --- the single-writer handle ---

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

#[test]
fn writer_thread_serves_handles() {
    let dir = TempDir::new();
    let (handle, thread) = open(&dir.db()).spawn_writer(8).unwrap();
    let a = handle
        .call_blocking(|s| s.write(|tx| tx.create_terminal("h", &sample(1))))
        .unwrap();
    let h2 = handle.clone();
    let b = block_on(h2.call(|s| s.write(|tx| tx.create_terminal("h", &sample(2))))).unwrap();
    assert_eq!((a.get(), b.get()), (1, 2));
    let n = handle
        .call_blocking(|s| Ok(s.reader()?.terminals()?.len()))
        .unwrap();
    assert_eq!(n, 2);
    drop(handle);
    drop(h2);
    thread.join().unwrap();
}
