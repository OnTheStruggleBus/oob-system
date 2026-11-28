//! Configuration-store tests: one or more per #25 acceptance criterion and
//! design-note §9 test-plan item. Box-free; every test has its own
//! temp-dir database. Tests that need the command bus live in
//! `command/policy_tests.rs` (only `command/` may name the mux).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use rusqlite::Connection;

use crate::command::target::{Filters, StoreDirectory, Target, TerminalDirectory};
use crate::command::{CommandContext, CommandError, FrontDoor, Role};
use crate::store::{
    BOX_IDENTIFYING, Durability, Event, Reader, Store, StoreError, StoreOptions, TerminalId,
    UnitAddress, class_of,
};

use super::check::doubles::{AllowAll, Recording};
use super::doc::{ClassBudgets, Lineup, MuxSettings, UnitDoc};
use super::*;

const SEED: &str = include_str!("../../tests/fixtures/seed-bundle.json");

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "oob-dac-config-{}-{}",
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

fn open(dir: &TempDir) -> Store {
    Store::open(&dir.db(), &StoreOptions::default()).unwrap()
}

fn catalog() -> Arc<InMemoryCatalog> {
    Arc::new(InMemoryCatalog::new(["dct2000", "dct1700"]))
}

fn service(v: Arc<dyn ConfigValidator>) -> ConfigService {
    ConfigService::new(v, catalog(), ConfigLimits::default())
}

fn structural() -> ConfigService {
    service(Arc::new(StructuralValidator))
}

fn owner() -> CommandContext {
    CommandContext {
        actor: "marshal".into(),
        role: Role::Owner,
        front_door: FrontDoor::Rest,
        confirmation: None,
    }
}

fn bundle() -> SeedBundle {
    SeedBundle::from_json(SEED).unwrap()
}

fn lineup() -> Lineup {
    bundle().lineup
}

fn budgets() -> ClassBudgets {
    bundle().class_budgets
}

fn mux() -> MuxSettings {
    bundle().mux_settings
}

const LINEUP: ObjectKey = ObjectKey {
    kind: ConfigKind::Lineup,
    object_id: None,
};
const BUDGETS: ObjectKey = ObjectKey {
    kind: ConfigKind::ClassBudgets,
    object_id: None,
};
const MUX: ObjectKey = ObjectKey {
    kind: ConfigKind::MuxSettings,
    object_id: None,
};

/// A lineup that differs from the seed by one channel name.
fn lineup_named(name: &str) -> Doc {
    let mut l = lineup();
    l.vcm[0].name = name.into();
    Doc::Lineup(l)
}

fn save(
    store: &mut Store,
    svc: &ConfigService,
    key: ObjectKey,
    doc: Doc,
    base: BaseRevision,
) -> Result<Saved, ConfigError> {
    store
        .write_with(|tx| svc.save(tx, &owner(), key, &doc, base))
        .map(|w| w.saved)
}

fn new_unit(n: u64) -> NewUnit {
    NewUnit {
        unit_address: UnitAddress::new(n, 0).unwrap(), // synthetic
        serial: format!("GI{n:010}"),
        doc: UnitDoc::new("One-Way", "dct2000"),
    }
}

fn enroll(store: &mut Store, svc: &ConfigService, n: u64) -> TerminalId {
    store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), &new_unit(n)))
        .unwrap()
        .0
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
        r.get(0)
    })
    .unwrap()
}

fn raw(dir: &TempDir) -> Connection {
    Connection::open(dir.db()).unwrap()
}

fn reader(store: &Store) -> Reader {
    store.reader().unwrap()
}

fn head(store: &Store, key: ObjectKey) -> Option<Revision> {
    reader(store).config_head(key).unwrap()
}

/// Every `config_revision` row, rendered.
fn revision_rows(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT * FROM config_revision ORDER BY kind, object_id, revision")
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

// ---- revisions -------------------------------------------------------------

#[test]
fn every_save_creates_head_plus_one_and_an_identical_document_is_unchanged() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    assert_eq!(
        save(
            &mut store,
            &svc,
            LINEUP,
            lineup_named("A"),
            BaseRevision::None
        )
        .unwrap(),
        Saved::Revised { revision: 1 }
    );
    assert_eq!(
        save(
            &mut store,
            &svc,
            LINEUP,
            lineup_named("B"),
            BaseRevision::At(1)
        )
        .unwrap(),
        Saved::Revised { revision: 2 }
    );
    // Same digest as the head: nothing written.
    let before = count(&raw(&dir), "config_revision");
    assert_eq!(
        save(
            &mut store,
            &svc,
            LINEUP,
            lineup_named("B"),
            BaseRevision::At(2)
        )
        .unwrap(),
        Saved::Unchanged { revision: 2 }
    );
    assert_eq!(count(&raw(&dir), "config_revision"), before);
    let h = head(&store, LINEUP).unwrap();
    assert_eq!(h.meta.revision, 2);
    assert_eq!(h.doc, lineup_named("B"));
    assert_eq!(h.meta.origin, Origin::Save);
    assert_eq!(h.meta.actor, "marshal");
    assert_eq!(h.meta.doc_sha256, h.doc.sha256());
}

#[test]
fn a_stale_or_missing_base_is_a_conflict_and_stores_nothing() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    // Nothing there yet: At(1) is stale.
    assert!(matches!(
        save(
            &mut store,
            &svc,
            LINEUP,
            lineup_named("A"),
            BaseRevision::At(1)
        ),
        Err(ConfigError::Conflict { head: 0 })
    ));
    save(
        &mut store,
        &svc,
        LINEUP,
        lineup_named("A"),
        BaseRevision::None,
    )
    .unwrap();
    save(
        &mut store,
        &svc,
        LINEUP,
        lineup_named("B"),
        BaseRevision::At(1),
    )
    .unwrap();
    for base in [BaseRevision::None, BaseRevision::At(1), BaseRevision::At(9)] {
        let e = save(&mut store, &svc, LINEUP, lineup_named("C"), base).unwrap_err();
        assert!(matches!(e, ConfigError::Conflict { head: 2 }), "{e:?}");
        assert_eq!(e.reason(), "conflict");
    }
    assert_eq!(count(&raw(&dir), "config_revision"), 2);
}

#[test]
fn concurrent_saves_on_one_base_exactly_one_wins() {
    for _ in 0..20 {
        let dir = TempDir::new();
        let mut store = open(&dir);
        save(
            &mut store,
            &structural(),
            LINEUP,
            lineup_named("A"),
            BaseRevision::None,
        )
        .unwrap();
        let (handle, _t) = store.spawn_writer(8).unwrap();
        let admin = Arc::new(ConfigAdmin::from_service(
            handle.clone(),
            structural(),
            Arc::new(NoApplier),
        ));
        let gate = Arc::new(Barrier::new(2));
        let racers: Vec<_> = ["X", "Y"]
            .into_iter()
            .map(|name| {
                let (a, g) = (admin.clone(), gate.clone());
                thread::spawn(move || {
                    g.wait();
                    a.run(
                        &owner(),
                        &AdminOp::ConfigSave {
                            key: LINEUP,
                            doc: lineup_named(name),
                            base: BaseRevision::At(1),
                        },
                    )
                })
            })
            .collect();
        let results: Vec<_> = racers.into_iter().map(|t| t.join().unwrap()).collect();
        let wins = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(wins, 1, "{results:?}");
        assert!(
            results
                .iter()
                .any(|r| matches!(r, Err(ConfigError::Conflict { head: 2 })))
        );
        let hist = handle
            .call_blocking(|s| {
                s.reader()?
                    .config_history(LINEUP, None, 50)
                    .map_err(|e| StoreError::Corrupt(e.to_string()))
            })
            .unwrap();
        assert_eq!(hist.iter().map(|m| m.revision).collect::<Vec<_>>(), [1, 2]);
    }
}

#[test]
fn revision_numbers_are_contiguous_and_history_pages() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    for n in 0..12u32 {
        let base = BaseRevision::of((n > 0).then_some(n));
        save(
            &mut store,
            &svc,
            LINEUP,
            lineup_named(&format!("N{n}")),
            base,
        )
        .unwrap();
    }
    let r = reader(&store);
    let all = r.config_history(LINEUP, None, 500).unwrap();
    assert_eq!(
        all.iter().map(|m| m.revision).collect::<Vec<_>>(),
        (1..=12).collect::<Vec<_>>()
    );
    let page = r.config_history(LINEUP, Some(5), 3).unwrap();
    assert_eq!(
        page.iter().map(|m| m.revision).collect::<Vec<_>>(),
        [6, 7, 8]
    );
    // Limits clamp to 1..=HISTORY_PAGE_MAX.
    assert_eq!(r.config_history(LINEUP, None, 0).unwrap().len(), 1);
    assert_eq!(
        r.config_revision(LINEUP, 3).unwrap().doc,
        lineup_named("N2")
    );
    assert!(matches!(
        r.config_revision(LINEUP, 99),
        Err(ConfigError::UnknownRevision { revision: 99, .. })
    ));
}

#[test]
fn a_document_over_the_size_cap_or_of_the_wrong_kind_is_refused() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let tiny = ConfigService::new(
        Arc::new(StructuralValidator),
        catalog(),
        ConfigLimits { max_doc_bytes: 64 },
    );
    let e = save(
        &mut store,
        &tiny,
        LINEUP,
        lineup_named("A"),
        BaseRevision::None,
    )
    .unwrap_err();
    assert!(
        matches!(e, ConfigError::DocTooLarge { max: 64, .. }),
        "{e:?}"
    );
    assert_eq!(e.reason(), "doc-too-large");
    let svc = structural();
    // A lineup saved as class budgets.
    let e = save(
        &mut store,
        &svc,
        BUDGETS,
        lineup_named("A"),
        BaseRevision::None,
    )
    .unwrap_err();
    assert_eq!(e.reason(), "wrong-kind");
    // A unit is not a singleton save.
    let id = enroll(&mut store, &svc, 1);
    let e = save(
        &mut store,
        &svc,
        ObjectKey::unit(id),
        Doc::Unit(UnitDoc::new("One-Way", "dct2000")),
        BaseRevision::At(1),
    )
    .unwrap_err();
    assert_eq!(e.reason(), "wrong-kind");
    assert_eq!(count(&raw(&dir), "config_revision"), 1);
}

#[test]
fn canonical_json_is_sorted_compact_and_stable() {
    let d = Doc::MuxSettings(mux());
    let json = d.canonical_json();
    assert!(!json.contains(' ') && !json.contains('\n'), "{json}");
    assert!(json.starts_with("{\"pacer\":{\"max_pull\":64,"), "{json}");
    // Parsing back and re-encoding is byte-identical, so the digest is too.
    let again = Doc::from_json(ConfigKind::MuxSettings, &json).unwrap();
    assert_eq!(again.canonical_json(), json);
    assert_eq!(again.sha256(), d.sha256());
    assert_eq!(d.sha256().len(), 64);
    // Unknown fields are refused, not silently dropped.
    assert!(Doc::from_json(ConfigKind::MuxSettings, &json.replacen('{', "{\"x\":1,", 1)).is_err());
}

// ---- rollback --------------------------------------------------------------

#[test]
fn rollback_writes_a_new_revision_equal_to_the_target_and_never_rewrites_history() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    for (n, name) in ["A", "B", "C"].into_iter().enumerate() {
        let base = BaseRevision::of((n > 0).then_some(n as u32));
        save(&mut store, &svc, LINEUP, lineup_named(name), base).unwrap();
    }
    let history_before = revision_rows(&raw(&dir));
    let w = store
        .write_with(|tx| svc.rollback(tx, &owner(), LINEUP, 1, BaseRevision::At(3)))
        .unwrap();
    assert_eq!(w.saved, Saved::Revised { revision: 4 });
    let h = head(&store, LINEUP).unwrap();
    assert_eq!(h.doc, lineup_named("A"));
    assert_eq!(h.meta.origin, Origin::Rollback);
    assert_eq!(h.meta.rolled_back_to, Some(1));
    let r1 = reader(&store).config_revision(LINEUP, 1).unwrap();
    assert_eq!(h.meta.doc_sha256, r1.meta.doc_sha256);
    // Rows 1..=3 are byte-identical; one row was added.
    let after = revision_rows(&raw(&dir));
    assert_eq!(after.len(), 4);
    assert_eq!(after[..3], history_before[..]);
    // Rolling back to what the head already is: unchanged.
    let w = store
        .write_with(|tx| svc.rollback(tx, &owner(), LINEUP, 1, BaseRevision::At(4)))
        .unwrap();
    assert_eq!(w.saved, Saved::Unchanged { revision: 4 });
    // Unknown target, stale base.
    let e = store
        .write_with(|tx| svc.rollback(tx, &owner(), LINEUP, 9, BaseRevision::At(4)))
        .unwrap_err();
    assert_eq!(e.reason(), "unknown-revision");
    let e = store
        .write_with(|tx| svc.rollback(tx, &owner(), LINEUP, 2, BaseRevision::At(3)))
        .unwrap_err();
    assert!(matches!(e, ConfigError::Conflict { head: 4 }));
}

#[test]
fn a_rollback_the_validator_now_refuses_is_refused_and_stores_nothing() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    // A guardrail "tightened" after revision 1: names starting with OLD.
    let v = Arc::new(Recording::new(|c, _| {
        if c.lineup.is_some_and(|l| l.vcm[0].name.starts_with("OLD")) {
            vec![Violation {
                rule: "test-old-name",
                objects: vec![ObjectRef::new(LINEUP, "/vcm/0/name")],
                detail: "old names are refused now".into(),
                hard: true,
            }]
        } else {
            Vec::new()
        }
    }));
    let svc = service(v.clone());
    // Revision 1 predates the rule: written straight to the table.
    let open_svc = service(Arc::new(AllowAll));
    save(
        &mut store,
        &open_svc,
        LINEUP,
        lineup_named("OLD"),
        BaseRevision::None,
    )
    .unwrap();
    save(
        &mut store,
        &svc,
        LINEUP,
        lineup_named("NEW"),
        BaseRevision::At(1),
    )
    .unwrap();
    let before = revision_rows(&raw(&dir));
    let audit_before = count(&raw(&dir), "audit_log");
    let e = store
        .write_with(|tx| svc.rollback(tx, &owner(), LINEUP, 1, BaseRevision::At(2)))
        .unwrap_err();
    let ConfigError::Refused(r) = &e else {
        panic!("{e:?}")
    };
    assert_eq!(r.rules(), ["test-old-name"]);
    assert_eq!(r.violations()[0].objects[0].path, "/vcm/0/name");
    assert_eq!(revision_rows(&raw(&dir)), before);
    assert_eq!(count(&raw(&dir), "audit_log"), audit_before);
    assert_eq!(head(&store, LINEUP).unwrap().meta.revision, 2);
}

/// Test schema: mux_settings is at schema 2, whose step renames
/// `pacer.max_pull` from schema 1's `pacer.pull_max`.
fn schema_two(kind: ConfigKind) -> u32 {
    if kind == ConfigKind::MuxSettings {
        2
    } else {
        1
    }
}

fn rename_pull_max(mut v: serde_json::Value) -> Result<serde_json::Value, String> {
    let pacer = v
        .get_mut("pacer")
        .and_then(|p| p.as_object_mut())
        .ok_or("no pacer")?;
    let x = pacer.remove("pull_max").ok_or("no pull_max")?;
    pacer.insert("max_pull".into(), x);
    Ok(v)
}

static STEPS: &[doc::Upgrade] = &[doc::Upgrade {
    kind: ConfigKind::MuxSettings,
    from: 1,
    step: rename_pull_max,
}];

#[test]
fn a_rollback_to_an_older_schema_upgrades_in_memory_and_never_rewrites_the_row() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    // Revision 1 stored at schema 1 by an "older build" (old field name).
    let old_json = mux().pipe_json().replace("\"max_pull\"", "\"pull_max\"");
    store
        .write_with(|tx| -> Result<(), ConfigError> {
            let seq = tx.append_audit(&crate::store::AuditEntry {
                actor: "old-build".into(),
                role: "Owner".into(),
                front_door: "Rest".into(),
                command: "config.save".into(),
                target: Some("mux_settings@1".into()),
                outcome: "ok".into(),
                before: None,
                after: Some(old_json.clone()),
            })?;
            persist::insert(
                tx.conn(),
                &persist::NewRevision {
                    key: MUX,
                    revision: 1,
                    doc: &old_json,
                    sha: &doc::sha256_hex(old_json.as_bytes()),
                    schema: 1,
                    origin: Origin::Save,
                    rolled_back_to: None,
                    actor: "old-build",
                    audit_seq: seq,
                },
            )?;
            Ok(())
        })
        .unwrap();
    let svc = service(Arc::new(StructuralValidator)).with_schema(schema_two, STEPS);
    let mut faster = mux();
    faster.pacer.max_pull = 128;
    save(
        &mut store,
        &svc,
        MUX,
        Doc::MuxSettings(faster),
        BaseRevision::At(1),
    )
    .unwrap();
    let w = store
        .write_with(|tx| svc.rollback(tx, &owner(), MUX, 1, BaseRevision::At(2)))
        .unwrap();
    assert_eq!(w.saved, Saved::Revised { revision: 3 });
    assert_eq!(w.doc, Doc::MuxSettings(mux()));
    let c = raw(&dir);
    let (doc1, schema1): (String, u32) = c
        .query_row(
            "SELECT doc, doc_schema FROM config_revision WHERE kind = 'mux_settings' \
             AND revision = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((doc1, schema1), (old_json, 1), "row 1 was rewritten");
    let schema3: u32 = c
        .query_row(
            "SELECT doc_schema FROM config_revision WHERE kind = 'mux_settings' AND revision = 3",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(schema3, 2);
    // A build that reads only schema 1 refuses the schema-2 head by name.
    let e = reader(&store).config_head(MUX).unwrap_err();
    assert_eq!(e.reason(), "doc-schema-too-new");
}

trait PipeJson {
    fn pipe_json(&self) -> String;
}

impl PipeJson for MuxSettings {
    fn pipe_json(&self) -> String {
        Doc::MuxSettings(self.clone()).canonical_json()
    }
}

// ---- journal ---------------------------------------------------------------

#[test]
fn journal_rows_carry_actor_role_door_target_before_and_after() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap();
    save(
        &mut store,
        &svc,
        LINEUP,
        lineup_named("B"),
        BaseRevision::At(1),
    )
    .unwrap();
    store
        .write_with(|tx| svc.rollback(tx, &owner(), LINEUP, 1, BaseRevision::At(2)))
        .unwrap();
    let id = enroll(&mut store, &svc, 7);
    let audit = reader(&store).audit_records().unwrap();
    let find = |cmd: &str, target: &str| {
        audit
            .iter()
            .find(|a| a.entry.command == cmd && a.entry.target.as_deref() == Some(target))
            .unwrap_or_else(|| panic!("no {cmd} {target} in {audit:#?}"))
            .clone()
    };
    let seed = find("config.seed", "lineup@1");
    assert!(seed.entry.actor.starts_with("seed:") && seed.entry.actor.len() == 17);
    assert_eq!(seed.entry.before, None);
    assert_eq!(
        seed.entry.after.as_deref(),
        Some(Doc::Lineup(lineup()).canonical_json().as_str())
    );
    let s = find("config.save", "lineup@2");
    assert_eq!(
        (
            &*s.entry.actor,
            &*s.entry.role,
            &*s.entry.front_door,
            &*s.entry.outcome
        ),
        ("marshal", "Owner", "Rest", "ok")
    );
    assert_eq!(
        s.entry.before.as_deref(),
        Some(Doc::Lineup(lineup()).canonical_json().as_str())
    );
    assert_eq!(
        s.entry.after.as_deref(),
        Some(lineup_named("B").canonical_json().as_str())
    );
    let r = find("config.rollback", "lineup@3");
    assert_eq!(r.entry.before, s.entry.after);
    assert_eq!(r.entry.after, seed.entry.after);
    let e = find("unit.enroll", &format!("unit/{}@1", id.get()));
    // The journal never carries the unit address or serial.
    let after = e.entry.after.unwrap();
    assert!(!after.contains("GI0000000007") && !after.contains("000-00000-00007"));
    // Each revision points at its journal row.
    let h = head(&store, LINEUP).unwrap();
    assert_eq!(h.meta.audit_seq, r.seq);
}

#[test]
fn a_refusal_through_the_runtime_is_journaled_with_its_rules_and_stores_nothing() {
    let dir = TempDir::new();
    let store = open(&dir);
    let (handle, _t) = store.spawn_writer(8).unwrap();
    let v = Recording::new(|_, _| {
        vec![Violation {
            rule: "test-refuse",
            objects: vec![ObjectRef::new(LINEUP, "")],
            detail: "no".into(),
            hard: false,
        }]
    });
    let admin =
        ConfigAdmin::from_service(handle.clone(), service(Arc::new(v)), Arc::new(NoApplier));
    let e = admin
        .run(
            &owner(),
            &AdminOp::ConfigSave {
                key: LINEUP,
                doc: lineup_named("A"),
                base: BaseRevision::None,
            },
        )
        .unwrap_err();
    assert_eq!(e.reason(), "config-refused");
    assert_eq!(CommandError::Config(e).reason(), "config-refused");
    let audit = handle
        .call_blocking(|s| s.reader()?.audit_records())
        .unwrap();
    assert_eq!(audit.len(), 1);
    let a = &audit[0].entry;
    assert_eq!(a.command, "config.save");
    assert_eq!(a.target.as_deref(), Some("lineup"));
    assert_eq!(a.outcome, "refused:test-refuse");
    assert_eq!(
        a.after.as_deref(),
        Some(lineup_named("A").canonical_json().as_str())
    );
    let n = handle
        .call_blocking(|s| {
            Ok(s.reader()?
                .config_history(LINEUP, None, 10)
                .map_err(|e| StoreError::Corrupt(e.to_string()))?
                .len())
        })
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(admin.journal_failures(), 0);
}

#[test]
fn config_revision_is_append_only_by_trigger() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    save(
        &mut store,
        &svc,
        LINEUP,
        lineup_named("A"),
        BaseRevision::None,
    )
    .unwrap();
    for sql in [
        "UPDATE config_revision SET actor = 'forged'",
        "DELETE FROM config_revision",
        "REPLACE INTO config_revision (kind, object_id, revision, doc, doc_sha256, doc_schema, \
         origin, actor, audit_seq) SELECT kind, object_id, revision, '{}', doc_sha256, \
         doc_schema, origin, 'forged', audit_seq FROM config_revision",
        "INSERT OR REPLACE INTO config_revision (kind, object_id, revision, doc, doc_sha256, \
         doc_schema, origin, actor, audit_seq) SELECT kind, object_id, revision, '{}', \
         doc_sha256, doc_schema, origin, 'forged', audit_seq FROM config_revision",
        "INSERT INTO config_revision (kind, object_id, revision, doc, doc_sha256, doc_schema, \
         origin, actor, audit_seq) SELECT kind, object_id, revision, '{}', doc_sha256, \
         doc_schema, origin, 'forged', audit_seq FROM config_revision WHERE true \
         ON CONFLICT DO UPDATE SET actor = 'forged'",
    ] {
        let e = store
            .write(|tx| {
                tx.conn().execute(sql, [])?;
                Ok(())
            })
            .unwrap_err();
        assert!(
            matches!(
                e,
                StoreError::AppendOnly {
                    table: "config_revision"
                }
            ),
            "{sql}: {e:?}"
        );
    }
    assert_eq!(head(&store, LINEUP).unwrap().meta.actor, "marshal");
    // Registered as precious and append-only (the #21 registry test checks
    // the triggers of every append-only entry).
    let c = class_of("config_revision").unwrap();
    assert_eq!((c.durability, c.append_only), (Durability::Precious, true));
}

#[test]
fn no_code_path_updates_or_deletes_a_journal() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy();
            if p.extension().is_none_or(|x| x != "rs") || name.ends_with("tests.rs") {
                continue;
            }
            let flat = std::fs::read_to_string(&p)
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            for table in ["config_revision", "audit_log"] {
                for verb in [
                    "update",
                    "delete from",
                    "replace into",
                    "insert or replace into",
                ] {
                    assert!(
                        !flat.contains(&format!("{verb} {table}")),
                        "{}: {verb} {table}",
                        p.display()
                    );
                }
            }
        }
    }
}

// ---- atomicity -------------------------------------------------------------

#[test]
fn a_failure_after_the_revision_insert_leaves_neither_row() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural().with_fault(Fault::AfterRevision);
    let c = raw(&dir);
    let before: Vec<i64> = ["config_revision", "audit_log", "event_log"]
        .iter()
        .map(|t| count(&c, t))
        .collect();
    let e = save(
        &mut store,
        &svc,
        LINEUP,
        lineup_named("A"),
        BaseRevision::None,
    )
    .unwrap_err();
    assert_eq!(e.reason(), "store-error");
    let after: Vec<i64> = ["config_revision", "audit_log", "event_log"]
        .iter()
        .map(|t| count(&c, t))
        .collect();
    assert_eq!(before, after);
    // The same failure on enroll also takes the terminal row with it.
    let e = store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), &new_unit(1)))
        .unwrap_err();
    assert_eq!(e.reason(), "store-error");
    assert_eq!(count(&c, "terminal"), 0);
}

#[test]
fn a_committed_save_appends_its_config_saved_event() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    save(
        &mut store,
        &structural(),
        BUDGETS,
        Doc::ClassBudgets(budgets()),
        BaseRevision::None,
    )
    .unwrap();
    let payload: String = raw(&dir)
        .query_row(
            "SELECT payload FROM event_log WHERE kind = 'ConfigSaved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Event>(&payload).unwrap(),
        Event::ConfigSaved {
            config_kind: "class_budgets".into(),
            object_id: None,
            revision: 1,
        }
    );
}

// ---- box types -------------------------------------------------------------

#[test]
fn an_unknown_box_type_is_refused_on_enroll_update_and_rollback() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let mut u = new_unit(1);
    u.doc.box_type = "dct9999".into();
    let e = store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), &u))
        .unwrap_err();
    assert!(matches!(&e, ConfigError::UnknownBoxType(t) if t == "dct9999"));
    assert_eq!(count(&raw(&dir), "terminal"), 0);

    let id = enroll(&mut store, &svc, 1);
    let mut d = UnitDoc::new("One-Way", "dct9999");
    let e = store
        .write_with(|tx| svc.update_unit(tx, &owner(), id, &d, BaseRevision::At(1)))
        .unwrap_err();
    assert_eq!(e.reason(), "unknown-box-type");

    // Revision 2 is a known type under a wider catalog; a build that ships
    // a narrower catalog refuses the rollback to it.
    d.box_type = "dct1700".into();
    store
        .write_with(|tx| svc.update_unit(tx, &owner(), id, &d, BaseRevision::At(1)))
        .unwrap();
    d.box_type = "dct2000".into();
    store
        .write_with(|tx| svc.update_unit(tx, &owner(), id, &d, BaseRevision::At(2)))
        .unwrap();
    let narrow = ConfigService::new(
        Arc::new(StructuralValidator),
        Arc::new(InMemoryCatalog::new(["dct2000"])),
        ConfigLimits::default(),
    );
    let e = store
        .write_with(|tx| narrow.rollback(tx, &owner(), ObjectKey::unit(id), 2, BaseRevision::At(3)))
        .unwrap_err();
    assert_eq!(e.reason(), "unknown-box-type");
    // The structural validator names the same rule when the service's own
    // check is not what refuses (validator-level coverage).
    let snap_units = persist::Units::default();
    let heads = Heads::default();
    let cat = InMemoryCatalog::new(["dct2000"]);
    let refusal = StructuralValidator
        .validate(
            &heads.snapshot(&snap_units, &cat),
            &Change::UnitUpdate {
                id,
                doc: &UnitDoc::new("Nine-Way", "dct9999"),
            },
        )
        .unwrap_err();
    assert_eq!(refusal.rules(), ["unknown-box-type", "terminal-type"]);
}

#[test]
fn nothing_creates_a_box_type_at_runtime() {
    // No configuration kind for box types.
    let kinds: Vec<&str> = ConfigKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "lineup",
            "pid_plan",
            "class_budgets",
            "mux_settings",
            "unit"
        ]
    );
    // The catalog module has no mutating method and no adder.
    let text = include_str!("catalog.rs");
    for bad in [
        "&mut self",
        "fn insert",
        "fn add",
        "fn remove",
        "fn register",
        "fn set_",
    ] {
        assert!(!text.contains(bad), "catalog.rs contains {bad:?}");
    }
    // And no admin op names one.
    let admin = include_str!("admin.rs");
    assert!(
        !admin.contains("BoxType("),
        "an AdminOp variant names a box type"
    );
}

#[test]
fn the_file_catalog_reads_ids_and_refuses_duplicates() {
    let c = FileCatalog::from_json(
        br#"{"box_types":[{"id":"dct2000","status":"documented"},{"id":"dct1700"}]}"#,
    )
    .unwrap();
    assert!(c.contains("dct2000") && c.contains("dct1700") && !c.contains("x"));
    assert_eq!(c.ids(), ["dct1700", "dct2000"]);
    assert_eq!(c.digest().len(), 64);
    assert!(matches!(
        FileCatalog::from_json(br#"{"box_types":[{"id":"a"},{"id":"a"}]}"#),
        Err(CatalogError::DuplicateId(_))
    ));
    assert!(matches!(
        FileCatalog::from_json(br#"{"box_types":[{"id":""}]}"#),
        Err(CatalogError::EmptyId)
    ));
}

// ---- seed ------------------------------------------------------------------

#[test]
fn the_first_run_seed_writes_revision_one_of_each_singleton_and_no_box_instance() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let report = store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap();
    let SeedReport::Seeded { actor, keys } = report else {
        panic!("not seeded")
    };
    assert_eq!(actor, format!("seed:{}", &bundle().sha256()[..12]));
    assert_eq!(keys.len(), 4);
    for kind in ConfigKind::SINGLETONS {
        let h = head(&store, ObjectKey::singleton(kind)).unwrap();
        assert_eq!(h.meta.revision, 1);
        assert_eq!(h.meta.origin, Origin::Seed);
        assert_eq!(h.meta.actor, actor);
    }
    // ADR-003 §6 (carried over from #21): no box instance from git.
    let c = raw(&dir);
    for table in BOX_IDENTIFYING {
        assert_eq!(count(&c, table), 0, "{table}");
    }
    let units: i64 = c
        .query_row(
            "SELECT count(*) FROM config_revision WHERE kind = 'unit'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(units, 0);
    // A second call is a no-op.
    let rows = revision_rows(&c);
    assert_eq!(
        store
            .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
            .unwrap(),
        SeedReport::AlreadySeeded
    );
    assert_eq!(revision_rows(&c), rows);
}

#[test]
fn a_partially_configured_store_is_not_seeded_over() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    save(
        &mut store,
        &svc,
        MUX,
        Doc::MuxSettings(mux()),
        BaseRevision::None,
    )
    .unwrap();
    assert_eq!(
        store
            .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
            .unwrap(),
        SeedReport::AlreadySeeded
    );
    assert!(head(&store, LINEUP).is_none());
    assert_eq!(count(&raw(&dir), "config_revision"), 1);
}

#[test]
fn a_seed_bundle_cannot_carry_units_and_a_refused_seed_writes_nothing() {
    let with_units = SEED.replacen('{', "{\"units\":[{\"serial\":\"GI0000000001\"}],", 1);
    let e = SeedBundle::from_json(&with_units).unwrap_err();
    assert!(e.contains("units"), "{e}");
    let dir = TempDir::new();
    let mut store = open(&dir);
    let v = Recording::new(|_, c| match c {
        Change::Seed(_) => vec![Violation {
            rule: "test-seed",
            objects: Vec::new(),
            detail: String::new(),
            hard: true,
        }],
        _ => Vec::new(),
    });
    let svc = service(Arc::new(v));
    let e = store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap_err();
    assert_eq!(e.reason(), "config-refused");
    assert_eq!(count(&raw(&dir), "config_revision"), 0);
    assert_eq!(count(&raw(&dir), "audit_log"), 0);
}

// ---- units -----------------------------------------------------------------

/// The `terminal` row's mutable fields equal the latest unit revision.
fn assert_rows_match_heads(store: &Store) {
    let r = reader(store);
    for t in r.terminals().unwrap() {
        let h = r.config_head(ObjectKey::unit(t.id)).unwrap().unwrap();
        assert_eq!(
            Doc::Unit(UnitDoc::from_record(&t.record)),
            h.doc,
            "terminal {} row and head disagree",
            t.id.get()
        );
    }
}

#[test]
fn the_terminal_row_equals_the_latest_unit_revision_after_any_sequence() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let ids: Vec<TerminalId> = (1..=4).map(|n| enroll(&mut store, &svc, n)).collect();
    let identity: Vec<_> = reader(&store)
        .terminals()
        .unwrap()
        .into_iter()
        .map(|t| (t.id, t.record.unit_address, t.record.serial))
        .collect();
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for step in 0..200 {
        let id = ids[(next() % 4) as usize];
        let key = ObjectKey::unit(id);
        let h = head(&store, key).unwrap().meta.revision;
        let res = match next() % 5 {
            0 | 1 => {
                let mut d = UnitDoc::new(
                    ["One-Way", "Two-Way", "DOCSIS"][(next() % 3) as usize],
                    ["dct2000", "dct1700"][(next() % 2) as usize],
                );
                d.account_number = Some(format!("A{}", next() % 7));
                d.on_plant = next() % 2 == 0;
                d.downstream_plant = Some(format!("p{}", next() % 3));
                store
                    .write_with(|tx| svc.update_unit(tx, &owner(), id, &d, BaseRevision::At(h)))
                    .map(|_| ())
            }
            2 => store
                .write_with(|tx| svc.set_retired(tx, &owner(), id, true))
                .map(|_| ()),
            3 => store
                .write_with(|tx| svc.set_retired(tx, &owner(), id, false))
                .map(|_| ()),
            _ => {
                let to = 1 + (next() % u64::from(h)) as u32;
                store
                    .write_with(|tx| svc.rollback(tx, &owner(), key, to, BaseRevision::At(h)))
                    .map(|_| ())
            }
        };
        match res {
            Ok(()) | Err(ConfigError::Retired(_)) => {}
            Err(e) => panic!("step {step}: {e:?}"),
        }
        assert_rows_match_heads(&store);
    }
    // Identity never moved.
    let now: Vec<_> = reader(&store)
        .terminals()
        .unwrap()
        .into_iter()
        .map(|t| (t.id, t.record.unit_address, t.record.serial))
        .collect();
    assert_eq!(now, identity);
}

#[test]
fn identity_is_not_in_the_unit_document_and_cannot_be_edited() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let id = enroll(&mut store, &svc, 1);
    let json = head(&store, ObjectKey::unit(id))
        .unwrap()
        .doc
        .canonical_json();
    for field in ["serial", "unit_address", "ua_check", "terminal_id"] {
        assert!(!json.contains(field), "{field} in {json}");
        let forged = json.replacen('{', &format!("{{\"{field}\":\"x\","), 1);
        assert!(
            Doc::from_json(ConfigKind::Unit, &forged).is_err(),
            "{field}"
        );
    }
}

#[test]
fn a_retired_unit_is_kept_out_of_targeting_and_refuses_edits_until_reinstated() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let a = enroll(&mut store, &svc, 1);
    let b = enroll(&mut store, &svc, 2);
    assert!(
        store
            .write_with(|tx| svc.set_retired(tx, &owner(), b, true))
            .unwrap()
    );
    // Idempotent: retiring again changes nothing.
    assert!(
        !store
            .write_with(|tx| svc.set_retired(tx, &owner(), b, true))
            .unwrap()
    );
    let dirx = StoreDirectory::new(reader(&store));
    let ids: Vec<u32> = dirx
        .resolve(&Target::All, &Filters::default())
        .unwrap()
        .iter()
        .map(|t| t.id.get())
        .collect();
    assert_eq!(ids, [a.get()]);
    assert!(matches!(
        dirx.resolve(&Target::Single(b), &Filters::default()),
        Err(CommandError::UnknownTerminal(_))
    ));
    let e = store
        .write_with(|tx| {
            svc.update_unit(
                tx,
                &owner(),
                b,
                &UnitDoc::new("One-Way", "dct2000"),
                BaseRevision::At(1),
            )
        })
        .unwrap_err();
    assert_eq!(e.reason(), "unit-retired");
    // Never deleted; the row and its history remain.
    let t = reader(&store).terminal(b).unwrap().unwrap();
    assert!(t.retired_utc.is_some());
    assert!(
        store
            .write_with(|tx| svc.set_retired(tx, &owner(), b, false))
            .unwrap()
    );
    assert_eq!(
        dirx.resolve(&Target::All, &Filters::default())
            .unwrap()
            .len(),
        2
    );
    let audit = reader(&store).audit_records().unwrap();
    let retire = audit
        .iter()
        .find(|a| a.entry.command == "unit.retire")
        .unwrap();
    assert_eq!(retire.entry.before.as_deref(), Some("{\"retired\":false}"));
    assert_eq!(retire.entry.after.as_deref(), Some("{\"retired\":true}"));
    assert!(audit.iter().any(|a| a.entry.command == "unit.reinstate"));
    let unknown = TerminalId(99);
    let e = store
        .write_with(|tx| svc.set_retired(tx, &owner(), unknown, true))
        .unwrap_err();
    assert_eq!(e.reason(), "unknown-object");
}

#[test]
fn a_unit_is_enrolled_non_sacrificial_and_duplicates_are_named() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let mut u = new_unit(1);
    u.doc.sacrificial = true;
    let e = store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), &u))
        .unwrap_err();
    assert_eq!(e.reason(), "sacrificial-at-enroll");
    enroll(&mut store, &svc, 1);
    let mut same_ua = new_unit(1);
    same_ua.serial = "GI9999999999".into();
    let e = store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), &same_ua))
        .unwrap_err();
    assert_eq!(e.reason(), "duplicate-unit-address");
    let mut same_serial = new_unit(3);
    same_serial.serial = new_unit(1).serial;
    let e = store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), &same_serial))
        .unwrap_err();
    assert_eq!(e.reason(), "duplicate-serial");
    let mut bad_date = new_unit(2);
    bad_date.doc.install_date = Some("2026-02-30".into());
    let e = store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), &bad_date))
        .unwrap_err();
    assert_eq!(e.reason(), "config-refused");
    assert_eq!(count(&raw(&dir), "terminal"), 1);
    // The redacted Debug never prints identity.
    let dbg = format!("{:?}", new_unit(5));
    assert!(
        !dbg.contains("GI0000000005") && dbg.contains("redacted"),
        "{dbg}"
    );
}

// ---- validator seam --------------------------------------------------------

#[test]
fn the_validator_sees_the_whole_candidate_and_a_refusal_writes_nothing() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let refuse_pid = Arc::new(Mutex::new(false));
    let flag = refuse_pid.clone();
    let v = Arc::new(Recording::new(move |c, change| {
        if *flag.lock().unwrap()
            && matches!(change, Change::Save { key, .. } if key.kind == ConfigKind::PidPlan)
        {
            // Refused because of another object (the budgets).
            assert!(c.class_budgets.is_some());
            vec![Violation {
                rule: "test-pid-vs-budget",
                objects: vec![
                    ObjectRef::new(ObjectKey::singleton(ConfigKind::PidPlan), "/emm_streams"),
                    ObjectRef::new(BUDGETS, "/classes/c0"),
                ],
                detail: "cross-object".into(),
                hard: true,
            }]
        } else {
            Vec::new()
        }
    }));
    let svc = service(v.clone());
    store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap();
    let id = enroll(&mut store, &svc, 1);
    let mut pid = bundle().pid_plan;
    pid.transport_stream_id = 9;
    let pid_key = ObjectKey::singleton(ConfigKind::PidPlan);
    save(
        &mut store,
        &svc,
        pid_key,
        Doc::PidPlan(pid.clone()),
        BaseRevision::At(1),
    )
    .unwrap();
    let last = v.seen().pop().unwrap();
    assert!(last.change.starts_with("Save"), "{}", last.change);
    assert_eq!(
        last.pid_plan.as_ref(),
        Some(&pid),
        "the change is in the candidate"
    );
    assert_eq!(
        last.lineup,
        Some(lineup()),
        "unchanged heads are in the candidate"
    );
    assert_eq!(last.class_budgets, Some(budgets()));
    assert_eq!(last.mux_settings, Some(mux()));
    assert_eq!(last.units.len(), 1);
    assert_eq!(last.units[0].0, id);
    // Now refuse: nothing is written, and the refusal names both objects.
    *refuse_pid.lock().unwrap() = true;
    let rows = revision_rows(&raw(&dir));
    pid.transport_stream_id = 10;
    let e = save(
        &mut store,
        &svc,
        pid_key,
        Doc::PidPlan(pid),
        BaseRevision::At(2),
    )
    .unwrap_err();
    let ConfigError::Refused(r) = e else { panic!() };
    let kinds: BTreeSet<ConfigKind> = r.violations()[0].objects.iter().map(|o| o.kind).collect();
    assert_eq!(
        kinds,
        BTreeSet::from([ConfigKind::PidPlan, ConfigKind::ClassBudgets])
    );
    assert_eq!(revision_rows(&raw(&dir)), rows);
    // An enroll's candidate also carries the existing units; a batch is
    // validated once, as one candidate.
    let calls = v.seen().len();
    store
        .write_with(|tx| {
            svc.save_batch(
                tx,
                &owner(),
                &[
                    SaveChange {
                        key: LINEUP,
                        doc: lineup_named("Z"),
                        base: BaseRevision::At(1),
                    },
                    SaveChange {
                        key: MUX,
                        doc: Doc::MuxSettings(mux()),
                        base: BaseRevision::At(1),
                    },
                ],
            )
        })
        .unwrap();
    let seen = v.seen();
    assert_eq!(seen.len(), calls + 1, "one validation for the batch");
    assert_eq!(
        seen[calls].lineup,
        Some(match lineup_named("Z") {
            Doc::Lineup(l) => l,
            _ => unreachable!(),
        })
    );
    assert!(
        Refusal::new(Vec::new()).is_none(),
        "a refusal is never empty"
    );
}

#[test]
fn a_batch_is_atomic_and_refuses_a_key_twice() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap();
    let rows = revision_rows(&raw(&dir));
    // The second change has a stale base: the first is not written either.
    let e = store
        .write_with(|tx| {
            svc.save_batch(
                tx,
                &owner(),
                &[
                    SaveChange {
                        key: LINEUP,
                        doc: lineup_named("Q"),
                        base: BaseRevision::At(1),
                    },
                    SaveChange {
                        key: MUX,
                        doc: Doc::MuxSettings(mux()),
                        base: BaseRevision::At(7),
                    },
                ],
            )
        })
        .unwrap_err();
    assert_eq!(e.reason(), "conflict");
    assert_eq!(revision_rows(&raw(&dir)), rows);
    let twice = SaveChange {
        key: LINEUP,
        doc: lineup_named("Q"),
        base: BaseRevision::At(1),
    };
    let e = store
        .write_with(|tx| svc.save_batch(tx, &owner(), &[twice.clone(), twice.clone()]))
        .unwrap_err();
    assert_eq!(e.reason(), "wrong-kind");
    let w = store
        .write_with(|tx| svc.save_batch(tx, &owner(), &[twice]))
        .unwrap();
    assert_eq!(w[0].saved, Saved::Revised { revision: 2 });
}

// ---- applier ---------------------------------------------------------------

struct FakeApplier {
    store: Mutex<Option<crate::store::StoreHandle>>,
    calls: Mutex<Vec<(ObjectKey, u32, bool)>>,
    status: ApplyStatus,
}

impl ConfigApplier for FakeApplier {
    fn apply(&self, key: ObjectKey, _doc: &Doc, revision: u32) -> ApplyStatus {
        // Committed already? Read it back through a separate connection.
        let committed = self.store.lock().unwrap().as_ref().is_some_and(|h| {
            h.call_blocking(move |s| {
                Ok(s.reader()?
                    .config_head(key)
                    .map_err(|e| StoreError::Corrupt(e.to_string()))?
                    .is_some_and(|r| r.meta.revision == revision))
            })
            .unwrap()
        });
        self.calls.lock().unwrap().push((key, revision, committed));
        self.status.clone()
    }
}

#[test]
fn the_applier_runs_after_commit_and_a_failed_apply_does_not_unsave() {
    let dir = TempDir::new();
    let store = open(&dir);
    let (handle, _t) = store.spawn_writer(8).unwrap();
    let applier = Arc::new(FakeApplier {
        store: Mutex::new(Some(handle.clone())),
        calls: Mutex::new(Vec::new()),
        status: ApplyStatus::Failed {
            reason: "c0 floor below live demand".into(),
        },
    });
    let admin = ConfigAdmin::from_service(handle.clone(), structural(), applier.clone());
    let op = AdminOp::ConfigSave {
        key: BUDGETS,
        doc: Doc::ClassBudgets(budgets()),
        base: BaseRevision::None,
    };
    let out = admin.run(&owner(), &op).unwrap();
    assert_eq!(
        out,
        AdminOutcome::Saved(SavedItem {
            key: BUDGETS,
            saved: Saved::Revised { revision: 1 },
            applied: Some(ApplyStatus::Failed {
                reason: "c0 floor below live demand".into()
            }),
        })
    );
    assert_eq!(*applier.calls.lock().unwrap(), [(BUDGETS, 1, true)]);
    // Still saved.
    let n = handle
        .call_blocking(|s| {
            Ok(s.reader()?
                .config_head(BUDGETS)
                .map_err(|e| StoreError::Corrupt(e.to_string()))?
                .map(|r| r.meta.revision))
        })
        .unwrap();
    assert_eq!(n, Some(1));
    // An unchanged save is not applied again.
    let again = AdminOp::ConfigSave {
        key: BUDGETS,
        doc: Doc::ClassBudgets(budgets()),
        base: BaseRevision::At(1),
    };
    let AdminOutcome::Saved(item) = admin.run(&owner(), &again).unwrap() else {
        panic!()
    };
    assert_eq!(
        (item.saved, item.applied),
        (Saved::Unchanged { revision: 1 }, None)
    );
    assert_eq!(applier.calls.lock().unwrap().len(), 1);
    // A refused save never reaches the applier.
    let mut bad = UnitDoc::new("One-Way", "dct2000");
    bad.ip_address = Some("not-an-ip".into());
    let e = admin
        .run(
            &owner(),
            &AdminOp::UnitEnroll(NewUnit {
                doc: bad,
                ..new_unit(1)
            }),
        )
        .unwrap_err();
    assert_eq!(e.reason(), "config-refused");
    assert_eq!(applier.calls.lock().unwrap().len(), 1);
}

#[test]
fn boot_applies_valid_heads_and_reports_an_invalid_one_without_panicking() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    // Saved before a guardrail on mux settings existed.
    let svc = service(Arc::new(AllowAll));
    store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap();
    let (handle, _t) = store.spawn_writer(8).unwrap();
    let strict = service(Arc::new(Recording::new(|_, c| match c {
        Change::Save { key, .. } if key.kind == ConfigKind::MuxSettings => vec![Violation {
            rule: "test-tightened",
            objects: vec![ObjectRef::new(MUX, "/queues")],
            detail: String::new(),
            hard: true,
        }],
        _ => Vec::new(),
    })));
    let applier = Arc::new(FakeApplier {
        store: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
        status: ApplyStatus::Applied,
    });
    let admin = ConfigAdmin::from_service(handle, strict, applier.clone());
    let boot = admin.boot().unwrap();
    assert_eq!(boot.len(), 4);
    for b in &boot {
        if b.key == MUX {
            let BootStatus::Invalid(r) = &b.status else {
                panic!("{b:?}")
            };
            assert_eq!(r.rules(), ["test-tightened"]);
        } else {
            assert_eq!(
                b.status,
                BootStatus::Applied(ApplyStatus::Applied),
                "{:?}",
                b.key
            );
        }
        assert_eq!(b.revision, Some(1));
    }
    let applied: BTreeSet<ConfigKind> = applier
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.0.kind)
        .collect();
    assert!(!applied.contains(&ConfigKind::MuxSettings));
    assert_eq!(applied.len(), 3);
}

#[test]
fn boot_on_a_fresh_store_reports_unconfigured() {
    let dir = TempDir::new();
    let store = open(&dir);
    let (handle, _t) = store.spawn_writer(8).unwrap();
    let admin = ConfigAdmin::new(
        handle,
        Arc::new(StructuralValidator),
        catalog(),
        Arc::new(NoApplier),
        ConfigLimits::default(),
    );
    assert!(
        admin
            .boot()
            .unwrap()
            .iter()
            .all(|b| b.status == BootStatus::Unconfigured && b.revision.is_none())
    );
    assert_eq!(admin.catalog_digest(), catalog().digest());
}

// ---- psi table versions ----------------------------------------------------

#[test]
fn psi_table_versions_upsert_and_are_never_pruned() {
    use crate::store::PsiTableVersion;
    let dir = TempDir::new();
    let mut store = open(&dir);
    let row = |kind: &str, version, retired| PsiTableVersion {
        kind: kind.into(),
        version,
        bytes: vec![0x00, version],
        retired,
    };
    store
        .write(|tx| tx.save_psi_table_versions(&[row("pat", 3, false), row("pmt:1", 7, false)]))
        .unwrap();
    // Pmt1 left the plan: it is saved retired; a later snapshot without it
    // keeps it.
    store
        .write(|tx| tx.save_psi_table_versions(&[row("pat", 4, false), row("pmt:1", 7, true)]))
        .unwrap();
    store
        .write(|tx| tx.save_psi_table_versions(&[row("pat", 5, false)]))
        .unwrap();
    assert_eq!(
        reader(&store).psi_table_versions().unwrap(),
        [row("pat", 5, false), row("pmt:1", 7, true)]
    );
    assert!(
        store
            .write(|tx| tx.save_psi_table_versions(&[row("cat", 32, false)]))
            .is_err()
    );
    // A-9: the kind encoding is defined, in Rust and in the schema.
    for bad in [
        "Pat",
        "pmt:",
        "pmt:0",
        "pmt:01",
        "pmt:65536",
        "pmt:1x",
        "nit",
        "",
    ] {
        assert!(!crate::store::psi_kind(bad), "{bad}");
        assert!(
            store
                .write(|tx| tx.save_psi_table_versions(&[row(bad, 1, false)]))
                .is_err(),
            "{bad}"
        );
        let raw_insert = raw(&dir).execute(
            "INSERT INTO psi_table_version (kind, version, bytes, retired) VALUES (?1, 1, x'00', 0)",
            [bad],
        );
        assert!(raw_insert.is_err(), "the schema accepted {bad:?}");
    }
    for good in ["pat", "cat", "pmt:1", "pmt:65535"] {
        assert!(crate::store::psi_kind(good), "{good}");
    }
    let c = class_of("psi_table_version").unwrap();
    assert_eq!(c.durability, Durability::Precious);
}

// ---- docs ------------------------------------------------------------------

#[test]
fn the_docs_page_carries_every_kind_reason_and_structural_rule() {
    let page = include_str!("../../../../docs/architecture/oob-dac-config.md");
    for (kind, schema) in doc::DOC_SCHEMA {
        let row = format!("| `{}` | {schema} |", kind.as_str());
        assert!(page.contains(&row), "kind row missing or stale: {row}");
    }
    for r in ConfigError::REASONS {
        assert!(
            page.contains(&format!("`{r}`")),
            "reason {r} is not documented"
        );
    }
    for r in STRUCTURAL_RULES {
        assert!(
            page.contains(&format!("`{r}`")),
            "rule {r} is not documented"
        );
    }
    for cmd in [
        "config.save",
        "config.rollback",
        "config.seed",
        "unit.enroll",
        "unit.update",
        "unit.retire",
        "unit.reinstate",
    ] {
        assert!(
            page.contains(&format!("`{cmd}`")),
            "journal command {cmd} missing"
        );
    }
}

// ---- review fixes (PR #87) ---------------------------------------------------

/// Writes revision `rev` of `key` straight to the table, as an older or
/// newer build would have (`schema` is its document schema).
fn raw_revision(store: &mut Store, key: ObjectKey, rev: u32, json: &str, schema: u32) {
    let json = json.to_owned();
    store
        .write_with(|tx| -> Result<(), ConfigError> {
            let seq = tx.append_audit(&crate::store::AuditEntry {
                actor: "other-build".into(),
                role: "Owner".into(),
                front_door: "Rest".into(),
                command: "config.save".into(),
                target: Some(key.target(rev)),
                outcome: "ok".into(),
                before: None,
                after: Some(json.clone()),
            })?;
            persist::insert(
                tx.conn(),
                &persist::NewRevision {
                    key,
                    revision: rev,
                    doc: &json,
                    sha: &doc::sha256_hex(json.as_bytes()),
                    schema,
                    origin: Origin::Save,
                    rolled_back_to: None,
                    actor: "other-build",
                    audit_seq: seq,
                },
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn qa3_an_unreadable_head_can_be_replaced_and_rolled_back_and_boot_isolates_it() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap();
    // A newer build saved mux_settings at document schema 2.
    raw_revision(
        &mut store,
        MUX,
        2,
        &Doc::MuxSettings(mux()).canonical_json(),
        2,
    );
    // Other kinds fail closed: no rule is skipped silently.
    let e = save(
        &mut store,
        &svc,
        LINEUP,
        lineup_named("X"),
        BaseRevision::At(1),
    )
    .unwrap_err();
    assert_eq!(e.reason(), "doc-schema-too-new");
    // The unreadable kind itself can be replaced and rolled back.
    let mut fix = mux();
    fix.pacer.max_pull = 32;
    assert_eq!(
        save(
            &mut store,
            &svc,
            MUX,
            Doc::MuxSettings(fix),
            BaseRevision::At(2)
        )
        .unwrap(),
        Saved::Revised { revision: 3 }
    );
    let mut newer = mux();
    newer.pacer.max_pull = 48;
    raw_revision(
        &mut store,
        MUX,
        4,
        &Doc::MuxSettings(newer).canonical_json(),
        2,
    );
    let w = store
        .write_with(|tx| svc.rollback(tx, &owner(), MUX, 1, BaseRevision::At(4)))
        .unwrap();
    assert_eq!(w.saved, Saved::Revised { revision: 5 });
    // Boot: only the unreadable kind is held back.
    raw_revision(
        &mut store,
        MUX,
        6,
        &Doc::MuxSettings(mux()).canonical_json(),
        2,
    );
    let (handle, _t) = store.spawn_writer(8).unwrap();
    let admin = ConfigAdmin::from_service(handle, structural(), Arc::new(NoApplier));
    for b in admin.boot().unwrap() {
        if b.key == MUX {
            assert!(
                matches!(&b.status, BootStatus::Error { reason } if reason.contains("schema")),
                "{b:?}"
            );
            assert_eq!(b.revision, Some(6));
        } else {
            assert_eq!(
                b.status,
                BootStatus::Applied(ApplyStatus::NotApplicable),
                "{b:?}"
            );
        }
    }
}

struct Panics;

impl ConfigValidator for Panics {
    fn validate(&self, _: &ConfigSnapshot<'_>, _: &Change<'_>) -> Result<(), Refusal> {
        panic!("validator bug");
    }
}

#[test]
fn qa4_a_validator_panic_refuses_the_change_and_the_writer_keeps_running() {
    let dir = TempDir::new();
    let store = open(&dir);
    let (handle, _t) = store.spawn_writer(8).unwrap();
    let admin = ConfigAdmin::from_service(
        handle.clone(),
        service(Arc::new(Panics)),
        Arc::new(NoApplier),
    );
    let e = admin
        .run(
            &owner(),
            &AdminOp::ConfigSave {
                key: LINEUP,
                doc: lineup_named("A"),
                base: BaseRevision::None,
            },
        )
        .unwrap_err();
    assert_eq!(e.reason(), "validator-panic");
    // The single writer still serves every caller.
    let audit = handle
        .call_blocking(|s| s.reader()?.audit_records())
        .unwrap();
    assert_eq!(
        audit.len(),
        1,
        "the refusal journal row was written by the live writer"
    );
    assert_eq!(audit[0].entry.outcome, "refused:validator-panic");
    handle
        .call_blocking(|s| s.write(|tx| tx.save_psi_table_versions(&[])))
        .unwrap();
    // Boot reports it per kind instead of failing.
    let svc = service(Arc::new(AllowAll));
    handle
        .call_blocking(move |s| {
            s.write_with(|tx| svc.seed_if_empty(tx, &bundle()))
                .map_err(|e| StoreError::Corrupt(e.to_string()))
        })
        .unwrap();
    for b in admin.boot().unwrap() {
        assert!(matches!(b.status, BootStatus::Error { .. }), "{b:?}");
    }
}

#[test]
fn qa6_a_batch_reports_items_in_submission_order() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap();
    let out = store
        .write_with(|tx| {
            svc.save_batch(
                tx,
                &owner(),
                &[
                    SaveChange {
                        key: LINEUP,
                        doc: lineup_named("CHANGED"),
                        base: BaseRevision::At(1),
                    },
                    SaveChange {
                        key: BUDGETS,
                        doc: Doc::ClassBudgets(budgets()),
                        base: BaseRevision::At(1),
                    },
                    SaveChange {
                        key: MUX,
                        doc: Doc::MuxSettings(mux()),
                        base: BaseRevision::At(1),
                    },
                ],
            )
        })
        .unwrap();
    let got: Vec<(ObjectKey, Saved)> = out.iter().map(|w| (w.key, w.saved)).collect();
    assert_eq!(
        got,
        [
            (LINEUP, Saved::Revised { revision: 2 }),
            (BUDGETS, Saved::Unchanged { revision: 1 }),
            (MUX, Saved::Unchanged { revision: 1 }),
        ]
    );
}

/// Records the order revisions reach it; the first call is slow.
struct SlowFirst {
    calls: Mutex<Vec<u32>>,
    first: AtomicU64,
}

impl ConfigApplier for SlowFirst {
    fn apply(&self, _key: ObjectKey, _doc: &Doc, revision: u32) -> ApplyStatus {
        if self.first.fetch_add(1, Ordering::SeqCst) == 0 {
            thread::sleep(std::time::Duration::from_millis(150));
        }
        self.calls.lock().unwrap().push(revision);
        ApplyStatus::Applied
    }
}

#[test]
fn qa2_revisions_reach_the_applier_in_commit_order() {
    for _ in 0..5 {
        let dir = TempDir::new();
        let store = open(&dir);
        let (handle, _t) = store.spawn_writer(8).unwrap();
        let applier = Arc::new(SlowFirst {
            calls: Mutex::new(Vec::new()),
            first: AtomicU64::new(0),
        });
        let admin = Arc::new(ConfigAdmin::from_service(
            handle,
            structural(),
            applier.clone(),
        ));
        let save_as = |name: &'static str, base| {
            let a = admin.clone();
            thread::spawn(move || {
                a.run(
                    &owner(),
                    &AdminOp::ConfigSave {
                        key: LINEUP,
                        doc: lineup_named(name),
                        base,
                    },
                )
            })
        };
        let first = save_as("A", BaseRevision::None);
        thread::sleep(std::time::Duration::from_millis(30));
        // Retries until revision 1 is visible, then saves revision 2.
        let second = {
            let a = admin.clone();
            thread::spawn(move || {
                loop {
                    match a.run(
                        &owner(),
                        &AdminOp::ConfigSave {
                            key: LINEUP,
                            doc: lineup_named("B"),
                            base: BaseRevision::At(1),
                        },
                    ) {
                        Err(ConfigError::Conflict { head: 0 }) => {
                            thread::sleep(std::time::Duration::from_millis(5))
                        }
                        other => return other,
                    }
                }
            })
        };
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        assert_eq!(*applier.calls.lock().unwrap(), [1, 2]);
    }
}

#[test]
fn qa7_every_refused_change_and_a_refused_seed_are_journaled() {
    let dir = TempDir::new();
    let store = open(&dir);
    let (handle, _t) = store.spawn_writer(8).unwrap();
    let admin = ConfigAdmin::from_service(handle.clone(), structural(), Arc::new(NoApplier));
    let mut u = new_unit(1);
    u.doc.box_type = "dct9999".into();
    assert_eq!(
        admin
            .run(&owner(), &AdminOp::UnitEnroll(u))
            .unwrap_err()
            .reason(),
        "unknown-box-type"
    );
    let mut s = new_unit(2);
    s.doc.sacrificial = true;
    assert_eq!(
        admin
            .run(&owner(), &AdminOp::UnitEnroll(s))
            .unwrap_err()
            .reason(),
        "sacrificial-at-enroll"
    );
    let audit = handle
        .call_blocking(|s| s.reader()?.audit_records())
        .unwrap();
    let outcomes: Vec<(&str, &str)> = audit
        .iter()
        .map(|a| (a.entry.command.as_str(), a.entry.outcome.as_str()))
        .collect();
    assert_eq!(
        outcomes,
        [
            ("unit.enroll", "refused:unknown-box-type"),
            ("unit.enroll", "refused:sacrificial-at-enroll"),
        ]
    );
    assert!(audit[0].entry.after.as_deref().unwrap().contains("dct9999"));
    // A refused seed (it bypasses the bus) leaves its own row.
    let refusing = ConfigAdmin::from_service(
        handle.clone(),
        service(Arc::new(Recording::new(|_, _| {
            vec![Violation {
                rule: "test-seed",
                objects: Vec::new(),
                detail: String::new(),
                hard: true,
            }]
        }))),
        Arc::new(NoApplier),
    );
    assert_eq!(
        refusing.seed_if_empty(&bundle()).unwrap_err().reason(),
        "config-refused"
    );
    let audit = handle
        .call_blocking(|s| s.reader()?.audit_records())
        .unwrap();
    let seed = audit.last().unwrap();
    assert_eq!(seed.entry.command, "config.seed");
    assert_eq!(seed.entry.outcome, "refused:test-seed");
    assert!(seed.entry.actor.starts_with("seed:"));
    assert_eq!(
        seed.entry.after.as_deref(),
        Some(doc::canonical(&bundle()).as_str())
    );
}

#[test]
fn qa8_a_rejected_serial_or_unit_address_is_never_echoed() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let mut u = new_unit(1);
    u.serial = "SECRET-SN-99".into();
    let e = store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), &u))
        .unwrap_err();
    let text = CommandError::Config(e).to_string();
    assert!(!text.contains("SECRET-SN-99"), "{text}");
    assert!(text.contains("serial"), "{text}");
    let e = "123-45678-90123-4567"
        .parse::<UnitAddress>()
        .unwrap_err()
        .to_string();
    assert!(!e.contains("123-45678"), "{e}");
}

#[test]
fn a1_boot_holds_back_only_the_kinds_a_violation_names() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    store
        .write_with(|tx| service(Arc::new(AllowAll)).seed_if_empty(tx, &bundle()))
        .unwrap();
    let (handle, _t) = store.spawn_writer(8).unwrap();
    // Ignores the Change: reports every violation on the candidate.
    let strict = service(Arc::new(Recording::new(|_, _| {
        vec![Violation {
            rule: "test-lineup-only",
            objects: vec![ObjectRef::new(LINEUP, "/rf_channels/0/freq_khz")],
            detail: String::new(),
            hard: true,
        }]
    })));
    let admin = ConfigAdmin::from_service(handle, strict, Arc::new(NoApplier));
    for b in admin.boot().unwrap() {
        if b.key == LINEUP {
            let BootStatus::Invalid(r) = &b.status else {
                panic!("{b:?}")
            };
            assert_eq!(r.rules(), ["test-lineup-only"]);
        } else {
            assert_eq!(
                b.status,
                BootStatus::Applied(ApplyStatus::NotApplicable),
                "{b:?}"
            );
        }
    }
}

#[test]
fn a3_box_identifying_rows_are_registered_and_empty_after_the_seed() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    store
        .write_with(|tx| svc.seed_if_empty(tx, &bundle()))
        .unwrap();
    let rows = |c: &Connection| -> i64 {
        crate::store::BOX_IDENTIFYING_ROWS
            .iter()
            .map(|(t, w)| {
                c.query_row(
                    &format!("SELECT count(*) FROM \"{t}\" WHERE {w}"),
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
            })
            .sum()
    };
    assert_eq!(rows(&raw(&dir)), 0, "a seeded store holds a box record");
    // The registry is not vacuous: an enrollment lands in both tables.
    let id = enroll(&mut store, &svc, 1);
    store
        .write_with(|tx| svc.set_retired(tx, &owner(), id, true))
        .unwrap();
    assert_eq!(rows(&raw(&dir)), 3);
}

// ---- QA-1: identity correction (per oob-architect ruling) -------------------

fn unit_with(address: u64, check: u16, serial: &str) -> NewUnit {
    NewUnit {
        unit_address: UnitAddress::new(address, check).unwrap(), // synthetic
        serial: serial.into(),
        doc: UnitDoc::new("One-Way", "dct2000"),
    }
}

fn try_enroll(
    store: &mut Store,
    svc: &ConfigService,
    u: &NewUnit,
) -> Result<TerminalId, ConfigError> {
    store
        .write_with(|tx| svc.enroll_unit(tx, &owner(), u))
        .map(|(id, _)| id)
}

fn retire(store: &mut Store, svc: &ConfigService, id: TerminalId) {
    assert!(
        store
            .write_with(|tx| svc.set_retired(tx, &owner(), id, true))
            .unwrap()
    );
}

#[test]
fn qa1_a_wrong_identity_is_corrected_by_retire_and_enroll_for_each_field() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    // (enrolled with a typo, the correct identity, the field that differs)
    let cases = [
        (
            unit_with(10, 1, "GI0000000010"),
            unit_with(10, 1, "GI0000000011"),
            "serial",
        ),
        (
            unit_with(20, 1, "GI0000000020"),
            unit_with(21, 1, "GI0000000020"),
            "unit address",
        ),
        (
            unit_with(30, 1, "GI0000000030"),
            unit_with(30, 2, "GI0000000030"),
            "check digits",
        ),
    ];
    for (wrong, right, field) in cases {
        let old = try_enroll(&mut store, &svc, &wrong).unwrap();
        // While the wrong record is live, the correction collides.
        assert!(try_enroll(&mut store, &svc, &right).is_err(), "{field}");
        retire(&mut store, &svc, old);
        let new = try_enroll(&mut store, &svc, &right).unwrap_or_else(|e| panic!("{field}: {e:?}"));
        assert!(new > old, "{field}: a re-enrollment is a new ID");
        // A new record: revision 1, non-sacrificial; nothing carried over.
        let h = head(&store, ObjectKey::unit(new)).unwrap();
        assert_eq!(h.meta.revision, 1, "{field}");
        assert!(!h.doc.as_unit().unwrap().sacrificial, "{field}");
        // The old record keeps its identity and history.
        let t = reader(&store).terminal(old).unwrap().unwrap();
        assert_eq!(t.record.unit_address, wrong.unit_address, "{field}");
        assert_eq!(t.record.serial, wrong.serial, "{field}");
        assert!(t.retired_utc.is_some());
    }
}

#[test]
fn qa1_reinstate_is_refused_while_a_live_unit_holds_the_identity() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let a = try_enroll(&mut store, &svc, &unit_with(1, 0, "GI0000000001")).unwrap();
    retire(&mut store, &svc, a);
    // Same unit address, different serial.
    let b = try_enroll(&mut store, &svc, &unit_with(1, 0, "GI0000000002")).unwrap();
    let e = store
        .write_with(|tx| svc.set_retired(tx, &owner(), a, false))
        .unwrap_err();
    assert_eq!(e.reason(), "duplicate-unit-address");
    retire(&mut store, &svc, b);
    // Same serial, different unit address.
    let c = try_enroll(&mut store, &svc, &unit_with(2, 0, "GI0000000001")).unwrap();
    let e = store
        .write_with(|tx| svc.set_retired(tx, &owner(), a, false))
        .unwrap_err();
    assert_eq!(e.reason(), "duplicate-serial");
    // Once nothing live holds either value, reinstating works.
    retire(&mut store, &svc, c);
    assert!(
        store
            .write_with(|tx| svc.set_retired(tx, &owner(), a, false))
            .unwrap()
    );
    assert!(
        reader(&store)
            .terminal(a)
            .unwrap()
            .unwrap()
            .retired_utc
            .is_none()
    );
}

#[test]
fn qa1_the_same_identity_can_be_retired_and_enrolled_twice() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let u = unit_with(5, 5, "GI0000000005");
    let first = try_enroll(&mut store, &svc, &u).unwrap();
    retire(&mut store, &svc, first);
    let second = try_enroll(&mut store, &svc, &u).unwrap();
    retire(&mut store, &svc, second);
    let third = try_enroll(&mut store, &svc, &u).unwrap();
    assert!(first < second && second < third);
    let live: Vec<TerminalId> = reader(&store)
        .terminals()
        .unwrap()
        .into_iter()
        .filter(|t| t.retired_utc.is_none())
        .map(|t| t.id)
        .collect();
    assert_eq!(live, [third]);
    // Targeting sees only the live record.
    let d = StoreDirectory::new(reader(&store));
    let ids: Vec<TerminalId> = d
        .resolve(&Target::All, &Filters::default())
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, [third]);
}

// ---- recommissioning: retirement is never permanent -------------------------

#[test]
fn a_retired_unit_can_always_be_recommissioned_with_its_record_intact() {
    let dir = TempDir::new();
    let mut store = open(&dir);
    let svc = structural();
    let id = try_enroll(&mut store, &svc, &unit_with(7, 0, "GI0000000007")).unwrap();
    let key = ObjectKey::unit(id);
    for round in 0..3u32 {
        // Change the record while in service, then retire and reinstate.
        let rev = head(&store, key).unwrap().meta.revision;
        let mut d = UnitDoc::new("Two-Way", "dct2000");
        d.account_number = Some(format!("ROUND{round}"));
        store
            .write_with(|tx| svc.update_unit(tx, &owner(), id, &d, BaseRevision::At(rev)))
            .unwrap();
        retire(&mut store, &svc, id);
        assert!(
            store
                .write_with(|tx| svc.set_retired(tx, &owner(), id, false))
                .unwrap(),
            "round {round}: reinstate refused"
        );
        // Back in service with the same ID, identity and full history.
        let t = reader(&store).terminal(id).unwrap().unwrap();
        assert!(t.retired_utc.is_none());
        assert_eq!(t.record.serial, "GI0000000007");
        assert_eq!(
            t.record.account_number.as_deref(),
            Some(format!("ROUND{round}").as_str())
        );
        assert_eq!(head(&store, key).unwrap().meta.revision, rev + 1);
        let d = StoreDirectory::new(reader(&store));
        assert_eq!(
            d.resolve(&Target::Single(id), &Filters::default())
                .unwrap()
                .len(),
            1
        );
    }
    // A collision with a live unit only defers it: retire that one and the
    // old record comes back.
    retire(&mut store, &svc, id);
    let other = try_enroll(&mut store, &svc, &unit_with(7, 0, "GI0000000008")).unwrap();
    let e = store
        .write_with(|tx| svc.set_retired(tx, &owner(), id, false))
        .unwrap_err();
    assert_eq!(e.reason(), "duplicate-unit-address");
    retire(&mut store, &svc, other);
    assert!(
        store
            .write_with(|tx| svc.set_retired(tx, &owner(), id, false))
            .unwrap()
    );
    // A box type that left the catalog while retired does not block it.
    let narrow = ConfigService::new(
        Arc::new(StructuralValidator),
        Arc::new(InMemoryCatalog::new(["dct1700"])),
        ConfigLimits::default(),
    );
    retire(&mut store, &narrow, id);
    assert!(
        store
            .write_with(|tx| narrow.set_retired(tx, &owner(), id, false))
            .unwrap()
    );
}

#[test]
fn no_code_path_deletes_a_unit_record() {
    // Retirement is never permanent because a unit row is never removed.
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if p.extension().is_none_or(|x| x != "rs") || name.ends_with("tests.rs") {
                continue;
            }
            let flat = std::fs::read_to_string(&p)
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            for bad in [
                "delete from terminal ",
                "delete from terminal\"",
                "delete from terminal;",
                "delete from \"terminal\"",
                "drop table terminal",
                "replace into terminal ",
            ] {
                assert!(!flat.contains(bad), "{}: {bad}", p.display());
            }
        }
    }
}

#[test]
fn a_misfiled_unit_document_is_named_not_journaled() {
    let dir = TempDir::new();
    let store = open(&dir);
    let (handle, _t) = store.spawn_writer(8).unwrap();
    let admin = ConfigAdmin::from_service(handle.clone(), structural(), Arc::new(NoApplier));
    let mut d = UnitDoc::new("One-Way", "dct2000");
    d.account_number = Some("ACCT-PRIVATE".into());
    let e = admin
        .run(
            &owner(),
            &AdminOp::ConfigSave {
                key: LINEUP,
                doc: Doc::Unit(d),
                base: BaseRevision::None,
            },
        )
        .unwrap_err();
    assert_eq!(e.reason(), "wrong-kind");
    let audit = handle
        .call_blocking(|s| s.reader()?.audit_records())
        .unwrap();
    assert_eq!(
        audit[0].entry.after.as_deref(),
        Some("{\"doc_kind\":\"unit\"}")
    );
}
