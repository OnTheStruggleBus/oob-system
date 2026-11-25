//! Policy-layer tests: one or more per #23 acceptance criterion and design
//! note test-plan item. Box-free: the evidence is deterministic role,
//! token, dry-run, lock and audit tests (no box capture is claimed).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use oob_mux::scheduler::BudgetConfig;
use oob_mux::{
    ManualClock, MonoTime, Payload, Pdu, Pid, Scheduler, SchedulerConfig, TrafficClass, UtcTime,
};

use crate::command::catalog::{AutoOp, CatalogEntry, OperationKind, TerminalOp};
use crate::command::exec::{EncodeError, Encoder, Work};
use crate::command::policy::{Policy, PolicyDenied};
use crate::command::target::{Filters, InMemoryDirectory, Target, TerminalRef};
use crate::command::{
    Command, CommandBus, CommandContext, CommandError, ConfirmationToken, ControlCommand,
    DefineOperation, FrontDoor, LabInjection, Role, Submitted, TemplateId,
};
use crate::store::{
    AuditRecord, NewTerminal, Store, StoreHandle, StoreOptions, TerminalId, TerminalType,
    UnitAddress,
};

use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "oob-dac-policy-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct NullEncoder;

impl Encoder for NullEncoder {
    fn encode(&self, _w: &Work, _t: Option<&TerminalRef>) -> Result<Pdu, EncodeError> {
        Ok(Pdu {
            class: TrafficClass::C1Addressed,
            priority: 0,
            pid: Pid::new(0x0100).unwrap(),
            release_at: None,
            deadline: None,
            payload: Payload::Section(Arc::from(&[0xC0u8, 0x00, 0x02, 0x00, 0x00][..])),
            stamp: None,
        })
    }
}

/// Terminal 1 is the sacrificial unit; 2 and 3 are not.
struct Fix {
    _dir: TempDir,
    store: StoreHandle,
    policy: Arc<StorePolicy>,
    clock: Arc<ManualClock>,
    bus: CommandBus,
    _sched: Scheduler,
}

fn tid(n: u32) -> TerminalId {
    TerminalId(n)
}

fn refs(sac: &[u32], all: &[u32]) -> Vec<TerminalRef> {
    all.iter()
        .map(|&i| {
            let mut t = TerminalRef::new(tid(i), TerminalType::OneWay);
            t.sacrificial = sac.contains(&i);
            t
        })
        .collect()
}

fn fix_ttl(ttl: Duration) -> Fix {
    let dir = TempDir::new();
    let mut store = Store::open(&dir.0.join("oob-dac.sqlite"), &StoreOptions::default()).unwrap();
    for n in 1..=3u64 {
        let mut t = NewTerminal::new(
            UnitAddress::new(n, 0).unwrap(), // synthetic
            format!("GI{n:010}"),
            TerminalType::OneWay,
            "dct2000",
        );
        t.sacrificial = n == 1;
        store.write(|tx| tx.create_terminal("test", &t)).unwrap();
    }
    let (handle, _thread) = store.spawn_writer(16).unwrap();
    let clock = Arc::new(ManualClock::new(
        MonoTime::from_origin(Duration::from_secs(10_000)),
        UtcTime::from_unix(Duration::from_secs(1_000)),
    ));
    let policy = Arc::new(
        StorePolicy::new(handle.clone(), clock.clone(), PolicySettings::new(ttl)).unwrap(),
    );
    let (sched, shandle, _rx) = Scheduler::new(SchedulerConfig {
        queue_depth: 1000,
        max_carousels: 2,
        outcome_capacity: 64,
        budget: BudgetConfig::permissive(1000),
    })
    .unwrap();
    let bus = CommandBus::new(
        shandle,
        policy.clone(),
        Arc::new(InMemoryDirectory::new(refs(&[1], &[1, 2, 3]))),
        Arc::new(NullEncoder),
        clock.clone(),
    )
    .with_config(crate::command::BusConfig {
        release_window: Duration::from_secs(3600),
        ..Default::default()
    });
    Fix {
        _dir: dir,
        store: handle,
        policy,
        clock,
        bus,
        _sched: sched,
    }
}

fn fix() -> Fix {
    fix_ttl(TOKEN_TTL_DEFAULT)
}

fn ctx_as(role: Role, door: FrontDoor) -> CommandContext {
    CommandContext {
        actor: "tester".into(),
        role,
        front_door: door,
        confirmation: None,
    }
}

fn owner() -> CommandContext {
    ctx_as(Role::Owner, FrontDoor::Rest)
}

fn with(mut c: CommandContext, t: &ConfirmationToken) -> CommandContext {
    c.confirmation = Some(t.clone());
    c
}

impl Fix {
    fn issue(&self, op: DestructiveOp, target: TokenTarget) -> ConfirmationToken {
        let confirm_text = confirmation_phrase(op, &target);
        self.policy
            .issue_token(
                &owner(),
                &TokenRequest {
                    op,
                    target,
                    confirm_text,
                },
            )
            .unwrap()
            .token
    }

    fn install(&self) -> TokenTarget {
        TokenTarget::Install(self.policy.install_id().unwrap())
    }

    fn audit(&self) -> Vec<AuditRecord> {
        self.store
            .call_blocking(|s| s.reader()?.audit_records())
            .unwrap()
    }

    fn rows(&self, command: &str) -> Vec<AuditRecord> {
        self.audit()
            .into_iter()
            .filter(|r| r.entry.command == command)
            .collect()
    }

    fn lock_off(&self) {
        let t = self.issue(DestructiveOp::CdlLockOff, self.install());
        self.policy.set_cdl_lock(&with(owner(), &t), false).unwrap();
    }
}

fn rule(r: Result<impl std::fmt::Debug, PolicyDenied>) -> &'static str {
    r.unwrap_err().reason
}

fn term(op: TerminalOp, id: u32) -> Command {
    Command::Terminal {
        op,
        terminal: tid(id),
    }
}

fn bus_rule<T: std::fmt::Debug>(r: Result<T, CommandError>) -> &'static str {
    r.unwrap_err().reason()
}

// ---- classes ---------------------------------------------------------

#[test]
fn every_destructive_class_is_enumerated_once_and_the_issue_list_is_covered() {
    let names: Vec<_> = CLASSES.iter().map(|r| r.name).collect();
    let mut dedup = names.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(names.len(), dedup.len());
    assert_eq!(DestructiveOp::all().count(), CLASSES.len());
    for want in [
        "cold-initialize",
        "factory-init",
        "initialize",
        "disconnect",
        "code-download-enable",
        "cdl-campaign",
        "cdl-lock-off",
        "unit-creation",
        "mark-sacrificial",
        "store-restore",
    ] {
        assert!(names.contains(&want), "{want}");
    }
    for r in CLASSES {
        assert_eq!(r.op.row().name, r.name);
        assert!(!r.targets.is_empty());
    }
    // The terminal ops' class names are the catalog names (the token binding).
    for op in TerminalOp::ALL {
        if let Some(c) = classify_terminal_op(op) {
            assert_eq!(c.name(), op.name());
            assert!(op.is_destructive());
        } else {
            assert!(!op.is_destructive());
        }
    }
    assert_eq!(DestructiveOp::Disconnect.tier(), Tier::Reversible);
}

#[test]
fn the_docs_page_carries_every_class_and_rule() {
    let page = include_str!("../../../../docs/architecture/oob-dac-policy.md");
    for r in CLASSES {
        let row = format!("| `{}` | {} | `{}` |", r.name, r.tier.label(), r.phrase);
        assert!(page.contains(&row), "class row missing or stale: {row}");
    }
    for r in RuleId::ALL {
        assert!(
            page.contains(&format!("`{}`", r.as_str())),
            "rule {} is not documented",
            r.as_str()
        );
    }
    let codes: Vec<_> = RuleId::ALL.iter().map(|r| r.as_str()).collect();
    let mut d = codes.clone();
    d.sort_unstable();
    d.dedup();
    assert_eq!(codes.len(), d.len());
}

// ---- roles -----------------------------------------------------------

#[test]
fn a_guest_is_refused_every_command_kind_by_name() {
    let f = fix();
    let guest = ctx_as(Role::Guest, FrontDoor::Rest);
    let cmds = [
        term(TerminalOp::Refresh, 2),
        term(TerminalOp::FactoryInit, 2),
        Command::Operation(DefineOperation::new(OperationKind::Refresh, Target::All)),
        Command::Auto(AutoOp::InsertSystemTime),
        Command::Control(ControlCommand::Ping),
        Command::Lab(LabInjection {
            template: TemplateId("t".into()),
            target: Target::All,
            filters: Filters::default(),
        }),
    ];
    for c in &cmds {
        assert_eq!(rule(f.policy.check(&guest, c, &[])), "guest-read-only");
    }
    // Through the bus too, and it never reaches the scheduler.
    assert_eq!(
        bus_rule(f.bus.submit(&guest, term(TerminalOp::Refresh, 2))),
        "guest-read-only"
    );
    // A guest can neither obtain a token nor change a setting.
    let req = TokenRequest {
        op: DestructiveOp::CdlLockOff,
        target: f.install(),
        confirm_text: confirmation_phrase(DestructiveOp::CdlLockOff, &f.install()),
    };
    assert_eq!(rule(f.policy.issue_token(&guest, &req)), "guest-read-only");
    assert_eq!(rule(f.policy.set_cdl_lock(&guest, true)), "guest-read-only");
    assert_eq!(
        rule(f.policy.mark_sacrificial(&guest, tid(2), false)),
        "guest-read-only"
    );
}

#[test]
fn destructive_classes_need_the_owner_role_and_operators_keep_plain_writes() {
    let f = fix();
    let op = ctx_as(Role::Operator, FrontDoor::Rest);
    let t = f.issue(DestructiveOp::FactoryInit, TokenTarget::Terminal(tid(2)));
    assert_eq!(
        bus_rule(
            f.bus
                .submit(&with(op.clone(), &t), term(TerminalOp::FactoryInit, 2))
        ),
        "owner-required"
    );
    // The refusal did not spend the token: the owner can still use it.
    assert!(matches!(
        f.bus
            .submit(&with(owner(), &t), term(TerminalOp::FactoryInit, 2)),
        Ok(Submitted::Queued(_))
    ));
    // Operators run non-destructive operations and cannot issue tokens.
    assert!(matches!(
        f.bus.submit(&op, term(TerminalOp::Refresh, 2)),
        Ok(Submitted::Queued(_))
    ));
    let req = TokenRequest {
        op: DestructiveOp::FactoryInit,
        target: TokenTarget::Terminal(tid(2)),
        confirm_text: "FACTORY INIT 2".into(),
    };
    assert_eq!(rule(f.policy.issue_token(&op, &req)), "owner-required");
    // The Lab front door injects as owner only.
    let lab = Command::Lab(LabInjection {
        template: TemplateId("t".into()),
        target: Target::Single(tid(1)),
        filters: Filters::default(),
    });
    assert_eq!(rule(f.policy.check(&op, &lab, &[])), "owner-required");
    assert!(f.policy.check(&owner(), &lab, &[]).is_ok());
}

#[test]
fn billing_and_wirelink_cannot_disconnect_or_get_a_token_and_timers_cannot_originate() {
    let f = fix();
    let t = f.issue(DestructiveOp::Disconnect, TokenTarget::Terminal(tid(2)));
    for door in [FrontDoor::Billing, FrontDoor::WireLinkWire] {
        let c = with(ctx_as(Role::Owner, door), &t);
        // Marshal's ruling (2026-11-23): Disconnect stays owner + token.
        assert_eq!(
            rule(f.policy.check(&c, &term(TerminalOp::Disconnect, 2), &[])),
            "owner-required"
        );
        let req = TokenRequest {
            op: DestructiveOp::Disconnect,
            target: TokenTarget::Terminal(tid(2)),
            confirm_text: "DISCONNECT 2".into(),
        };
        assert_eq!(rule(f.policy.issue_token(&c, &req)), "owner-required");
        // They keep non-destructive operations.
        assert!(
            f.policy
                .check(&c, &term(TerminalOp::Authorize, 2), &[])
                .is_ok()
        );
    }
    // The owner over REST still can.
    assert!(
        f.bus
            .submit(&with(owner(), &t), term(TerminalOp::Disconnect, 2))
            .is_ok()
    );

    // The DAC's own timers: auto operations yes, destructive never.
    let auto = ctx_as(Role::Owner, FrontDoor::Auto);
    assert!(
        f.policy
            .check(&auto, &Command::Auto(AutoOp::InsertSystemTime), &[])
            .is_ok()
    );
    assert_eq!(
        rule(
            f.policy
                .check(&auto, &Command::Auto(AutoOp::CodeDownload), &[])
        ),
        "system-cannot-originate"
    );
    assert_eq!(
        rule(
            f.policy
                .check(&auto, &term(TerminalOp::ColdInitialize, 2), &[])
        ),
        "system-cannot-originate"
    );
    assert_eq!(
        rule(f.policy.check(&auto, &term(TerminalOp::Refresh, 2), &[])),
        "owner-required"
    );
}

// ---- tokens ----------------------------------------------------------

#[test]
fn a_destructive_operation_without_a_token_is_refused_and_a_valid_one_runs_once() {
    let f = fix();
    assert_eq!(
        bus_rule(f.bus.submit(&owner(), term(TerminalOp::ColdInitialize, 2))),
        "token-missing"
    );
    let t = f.issue(DestructiveOp::ColdInitialize, TokenTarget::Terminal(tid(2)));
    let c = with(owner(), &t);
    assert!(matches!(
        f.bus.submit(&c, term(TerminalOp::ColdInitialize, 2)),
        Ok(Submitted::Queued(_))
    ));
    // Single use: the same token again is refused as reused.
    assert_eq!(
        bus_rule(f.bus.submit(&c, term(TerminalOp::ColdInitialize, 2))),
        "token-reused"
    );
}

#[test]
fn an_unknown_token_is_refused() {
    let f = fix();
    let c = with(owner(), &ConfirmationToken("oobtok-forged".into()));
    assert_eq!(
        bus_rule(f.bus.submit(&c, term(TerminalOp::FactoryInit, 2))),
        "token-unknown"
    );
}

#[test]
fn a_token_for_another_operation_is_refused_and_not_spent() {
    let f = fix();
    let t = f.issue(DestructiveOp::ColdInitialize, TokenTarget::Terminal(tid(2)));
    let c = with(owner(), &t);
    assert_eq!(
        bus_rule(f.bus.submit(&c, term(TerminalOp::FactoryInit, 2))),
        "token-op-mismatch"
    );
    assert!(
        f.bus
            .submit(&c, term(TerminalOp::ColdInitialize, 2))
            .is_ok()
    );
}

#[test]
fn a_token_for_another_target_is_refused_and_not_spent() {
    let f = fix();
    let t = f.issue(DestructiveOp::FactoryInit, TokenTarget::Terminal(tid(2)));
    let c = with(owner(), &t);
    assert_eq!(
        bus_rule(f.bus.submit(&c, term(TerminalOp::FactoryInit, 3))),
        "token-target-mismatch"
    );
    assert!(f.bus.submit(&c, term(TerminalOp::FactoryInit, 2)).is_ok());
}

#[test]
fn a_token_expires_exactly_at_its_ttl() {
    let f = fix_ttl(Duration::from_secs(1));
    let early = f.issue(DestructiveOp::Initialize, TokenTarget::Terminal(tid(2)));
    let late = f.issue(DestructiveOp::Initialize, TokenTarget::Terminal(tid(3)));
    f.clock.advance(Duration::from_millis(999));
    assert!(
        f.policy
            .check(
                &with(owner(), &early),
                &term(TerminalOp::Initialize, 2),
                &[]
            )
            .is_ok(),
        "valid one millisecond before the TTL"
    );
    f.clock.advance(Duration::from_millis(1));
    assert_eq!(
        rule(
            f.policy
                .check(&with(owner(), &late), &term(TerminalOp::Initialize, 3), &[])
        ),
        "token-expired"
    );
}

#[test]
fn the_ttl_is_server_capped_and_defaults_to_two_minutes() {
    assert_eq!(
        PolicySettings::default().token_ttl(),
        Duration::from_secs(120)
    );
    assert_eq!(
        PolicySettings::new(Duration::from_secs(3600)).token_ttl(),
        TOKEN_TTL_CAP
    );
    assert_eq!(TOKEN_TTL_CAP, Duration::from_secs(600));
    let f = fix_ttl(Duration::from_secs(3600));
    let r = f
        .policy
        .issue_token(
            &owner(),
            &TokenRequest {
                op: DestructiveOp::FactoryInit,
                target: TokenTarget::Terminal(tid(2)),
                confirm_text: "FACTORY INIT 2".into(),
            },
        )
        .unwrap();
    assert_eq!(r.ttl, TOKEN_TTL_CAP);
}

#[test]
fn issuing_checks_the_typed_phrase_and_the_target_on_the_server() {
    let f = fix();
    let bad = |op, target, text: &str| {
        f.policy
            .issue_token(
                &owner(),
                &TokenRequest {
                    op,
                    target,
                    confirm_text: text.into(),
                },
            )
            .unwrap_err()
            .reason
    };
    let t2 = TokenTarget::Terminal(tid(2));
    assert_eq!(
        bad(DestructiveOp::FactoryInit, t2.clone(), "factory init 2"),
        "confirm-text-mismatch"
    );
    assert_eq!(
        bad(DestructiveOp::FactoryInit, t2.clone(), "FACTORY INIT 3"),
        "confirm-text-mismatch"
    );
    assert_eq!(
        bad(DestructiveOp::FactoryInit, t2.clone(), ""),
        "confirm-text-mismatch"
    );
    // A target kind the class cannot take, and an install ID that is not this one.
    assert_eq!(
        bad(DestructiveOp::CdlLockOff, t2, "CDL LOCK OFF 2"),
        "token-target-mismatch"
    );
    assert_eq!(
        bad(
            DestructiveOp::CdlLockOff,
            TokenTarget::Install("deadbeefdeadbeef".into()),
            "CDL LOCK OFF deadbeefdeadbeef"
        ),
        "token-target-mismatch"
    );
}

#[test]
fn tokens_are_stored_only_as_a_hash_and_never_audited() {
    let f = fix();
    let t = f.issue(DestructiveOp::FactoryInit, TokenTarget::Terminal(tid(2)));
    let raw = t.0.clone();
    assert!(raw.starts_with("oobtok-") && raw.len() >= 7 + 32);
    let stored: Vec<String> = f
        .store
        .call_blocking(|s| {
            s.write(|tx| {
                let mut st = tx
                    .conn()
                    .prepare("SELECT token_hash FROM confirmation_token")?;
                Ok(st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?)
            })
        })
        .unwrap();
    assert_eq!(stored, [token::token_hash(&raw)]);
    assert_ne!(stored[0], raw);
    let _ = f
        .bus
        .submit(&with(owner(), &t), term(TerminalOp::FactoryInit, 2));
    for r in f.audit() {
        let e = r.entry;
        let all = format!("{e:?}");
        assert!(!all.contains(&raw), "audit row leaks the token");
    }
    // Two tokens differ (128 random bits).
    let t2 = f.issue(DestructiveOp::FactoryInit, TokenTarget::Terminal(tid(2)));
    assert_ne!(t.0, t2.0);
}

#[test]
fn tokens_do_not_survive_a_policy_restart() {
    let f = fix();
    let t = f.issue(DestructiveOp::FactoryInit, TokenTarget::Terminal(tid(2)));
    let again =
        StorePolicy::new(f.store.clone(), f.clock.clone(), PolicySettings::default()).unwrap();
    assert_eq!(
        rule(again.check(&with(owner(), &t), &term(TerminalOp::FactoryInit, 2), &[])),
        "token-unknown"
    );
}

#[test]
fn of_two_racing_spends_exactly_one_wins() {
    let f = fix();
    let t = f.issue(DestructiveOp::ColdInitialize, TokenTarget::Terminal(tid(2)));
    let n = 8;
    let gate = Arc::new(Barrier::new(n));
    let handles: Vec<_> = (0..n)
        .map(|_| {
            let (p, g, t) = (f.policy.clone(), gate.clone(), t.clone());
            thread::spawn(move || {
                g.wait();
                p.check(
                    &with(owner(), &t),
                    &term(TerminalOp::ColdInitialize, 2),
                    &[],
                )
                .is_ok()
            })
        })
        .collect();
    let wins = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(wins, 1);
}

// ---- dry run and bulk -----------------------------------------------

#[test]
fn a_bulk_operation_is_a_dry_run_unless_the_caller_opts_in() {
    let f = fix();
    let def = DefineOperation::new(OperationKind::Refresh, Target::All);
    assert!(def.dry_run);
    let r = f
        .bus
        .submit(&owner(), Command::Operation(def.clone()))
        .unwrap();
    let Submitted::DryRun(rep) = r else {
        panic!("expected a dry run, got {r:?}");
    };
    assert_eq!(rep.terminals, 3);
    assert!(f.bus.operations().is_empty(), "nothing was started");
    assert!(
        matches!(
            f.bus.submit(&owner(), Command::Operation(def.execute())),
            Ok(Submitted::Queued(_))
        ),
        "execute is an explicit opt-in"
    );
}

#[test]
fn a_destructive_bulk_dry_run_needs_no_token_and_execute_needs_one_bound_to_the_plan() {
    let f = fix();
    let kind = OperationKind::Terminal(TerminalOp::ColdInitialize);
    let dry = DefineOperation::new(kind, Target::All);
    let Submitted::DryRun(rep) = f
        .bus
        .submit(&owner(), Command::Operation(dry.clone()))
        .unwrap()
    else {
        panic!("dry run");
    };
    // Execute without a token.
    assert_eq!(
        bus_rule(
            f.bus
                .submit(&owner(), Command::Operation(dry.clone().execute()))
        ),
        "token-missing"
    );
    // A token bound to the plan the dry run reported.
    let plan = TokenTarget::Plan(rep.plan_digest.clone());
    let t = f.issue(DestructiveOp::ColdInitialize, plan.clone());
    // The set changed between the dry run and the execute.
    let narrower = DefineOperation::new(kind, Target::IdRange(tid(1)..=tid(2))).execute();
    assert_eq!(
        bus_rule(
            f.bus
                .submit(&with(owner(), &t), Command::Operation(narrower))
        ),
        "token-plan-changed"
    );
    // The same plan executes, once.
    assert!(
        f.bus
            .submit(
                &with(owner(), &t),
                Command::Operation(dry.clone().execute())
            )
            .is_ok()
    );
    assert_eq!(
        bus_rule(
            f.bus
                .submit(&with(owner(), &t), Command::Operation(dry.execute()))
        ),
        "token-reused"
    );
    // A single-terminal token does not authorize a bulk set.
    let one = f.issue(DestructiveOp::ColdInitialize, TokenTarget::Terminal(tid(2)));
    let all = DefineOperation::new(kind, Target::All).execute();
    assert_eq!(
        bus_rule(f.bus.submit(&with(owner(), &one), Command::Operation(all))),
        "token-target-mismatch"
    );
}

// ---- the sacrificial-only CDL lock ----------------------------------

#[test]
fn a_fresh_store_has_the_cdl_lock_on() {
    let f = fix();
    assert!(f.policy.cdl_lock_on().unwrap());
    assert_eq!(f.policy.install_id().unwrap().len(), 16);
}

#[test]
fn while_the_lock_is_on_a_cdl_campaign_with_any_non_sacrificial_unit_is_refused() {
    let f = fix();
    let cdl = Command::Auto(AutoOp::CodeDownload);
    let digest = |set: &[TerminalRef]| plan_digest(AutoOp::CodeDownload.name(), set);

    let only_sac = refs(&[1], &[1]);
    let t = f.issue(
        DestructiveOp::CdlCampaign,
        TokenTarget::Plan(digest(&only_sac)),
    );
    assert!(
        f.policy.check(&with(owner(), &t), &cdl, &only_sac).is_ok(),
        "the sacrificial unit alone passes"
    );

    let mixed = refs(&[1], &[1, 2, 3]);
    let t = f.issue(
        DestructiveOp::CdlCampaign,
        TokenTarget::Plan(digest(&mixed)),
    );
    let e = f
        .policy
        .check(&with(owner(), &t), &cdl, &mixed)
        .unwrap_err();
    assert_eq!(e.reason, "cdl-lock-non-sacrificial");
    assert!(
        e.detail.contains("2, 3"),
        "names the offending terminal IDs: {}",
        e.detail
    );
    // The lock refused before the token was touched, so it is unspent.
    f.lock_off();
    assert!(f.policy.check(&with(owner(), &t), &cdl, &mixed).is_ok());
}

#[test]
fn the_bus_cannot_resolve_a_cdl_target_set_so_the_lock_fails_closed() {
    let f = fix();
    let t = f.issue(
        DestructiveOp::CdlCampaign,
        TokenTarget::Plan(plan_digest(AutoOp::CodeDownload.name(), &[])),
    );
    assert_eq!(
        bus_rule(
            f.bus
                .submit(&with(owner(), &t), Command::Auto(AutoOp::CodeDownload))
        ),
        "cdl-lock-non-sacrificial"
    );
}

#[test]
fn turning_the_lock_off_needs_owner_and_a_token_and_nothing_else() {
    let f = fix();
    // Nothing else is consulted: no sacrificial unit is needed, no evidence.
    f.store
        .call_blocking(|s| {
            s.write(|tx| {
                tx.conn()
                    .execute("UPDATE terminal SET sacrificial = 0", [])?;
                Ok(())
            })
        })
        .unwrap();
    assert_eq!(
        rule(f.policy.set_cdl_lock(&owner(), false)),
        "token-missing"
    );
    assert!(
        f.policy.cdl_lock_on().unwrap(),
        "still on after the refusal"
    );

    let operator = ctx_as(Role::Operator, FrontDoor::Rest);
    let t = f.issue(DestructiveOp::CdlLockOff, f.install());
    assert_eq!(
        rule(f.policy.set_cdl_lock(&with(operator, &t), false)),
        "owner-required"
    );
    // Wrong operation (a mark-sacrificial token) and wrong class of binding.
    let wrong = f.issue(
        DestructiveOp::MarkSacrificial,
        TokenTarget::Terminal(tid(2)),
    );
    assert_eq!(
        rule(f.policy.set_cdl_lock(&with(owner(), &wrong), false)),
        "token-op-mismatch"
    );
    assert!(f.policy.cdl_lock_on().unwrap());

    // The owner with the token and nothing else turns it off, once.
    f.policy.set_cdl_lock(&with(owner(), &t), false).unwrap();
    assert!(!f.policy.cdl_lock_on().unwrap());
    assert_eq!(
        rule(f.policy.set_cdl_lock(&with(owner(), &t), false)),
        "token-reused"
    );

    // With the lock off, a CDL campaign to a non-sacrificial unit still
    // needs the owner and a token, but not the sacrificial unit.
    let set = refs(&[], &[2]);
    let c = f.issue(
        DestructiveOp::CdlCampaign,
        TokenTarget::Plan(plan_digest(AutoOp::CodeDownload.name(), &set)),
    );
    assert!(
        f.policy
            .check(
                &with(owner(), &c),
                &Command::Auto(AutoOp::CodeDownload),
                &set
            )
            .is_ok()
    );
    assert_eq!(
        rule(
            f.policy
                .check(&owner(), &Command::Auto(AutoOp::CodeDownload), &set)
        ),
        "token-missing"
    );
}

#[test]
fn turning_the_lock_on_needs_the_owner_only() {
    let f = fix();
    f.lock_off();
    assert!(!f.policy.cdl_lock_on().unwrap());
    let operator = ctx_as(Role::Operator, FrontDoor::Rest);
    assert_eq!(
        rule(f.policy.set_cdl_lock(&operator, true)),
        "owner-required"
    );
    f.policy.set_cdl_lock(&owner(), true).unwrap();
    assert!(f.policy.cdl_lock_on().unwrap());
}

#[test]
fn marking_a_unit_sacrificial_needs_a_token_and_unmarking_the_owner_only() {
    let f = fix();
    assert_eq!(
        rule(f.policy.mark_sacrificial(&owner(), tid(2), true)),
        "token-missing"
    );
    let t = f.issue(
        DestructiveOp::MarkSacrificial,
        TokenTarget::Terminal(tid(2)),
    );
    let other = f.issue(
        DestructiveOp::MarkSacrificial,
        TokenTarget::Terminal(tid(3)),
    );
    assert_eq!(
        rule(
            f.policy
                .mark_sacrificial(&with(owner(), &other), tid(2), true)
        ),
        "token-target-mismatch"
    );
    f.policy
        .mark_sacrificial(&with(owner(), &t), tid(2), true)
        .unwrap();
    // Terminal 2 is now CDL-eligible under the lock: authorize passes.
    let e = f.issue(
        DestructiveOp::CodeDownloadEnable,
        TokenTarget::Terminal(tid(2)),
    );
    f.policy
        .authorize(
            &with(owner(), &e),
            DestructiveOp::CodeDownloadEnable,
            TokenTarget::Terminal(tid(2)),
        )
        .unwrap();
    // Unmarking: owner only, no token.
    f.policy.mark_sacrificial(&owner(), tid(2), false).unwrap();
    assert_eq!(
        rule(f.policy.mark_sacrificial(&owner(), tid(99), false)),
        "unknown-terminal"
    );
    // Back to non-sacrificial: code-download enable is refused by the lock.
    let e = f.issue(
        DestructiveOp::CodeDownloadEnable,
        TokenTarget::Terminal(tid(2)),
    );
    assert_eq!(
        rule(f.policy.authorize(
            &with(owner(), &e),
            DestructiveOp::CodeDownloadEnable,
            TokenTarget::Terminal(tid(2)),
        )),
        "cdl-lock-non-sacrificial"
    );
}

#[test]
fn authorize_gates_store_restore_by_install_token() {
    let f = fix();
    assert_eq!(
        rule(
            f.policy
                .authorize(&owner(), DestructiveOp::StoreRestore, f.install())
        ),
        "token-missing"
    );
    let t = f.issue(DestructiveOp::StoreRestore, f.install());
    f.policy
        .authorize(&with(owner(), &t), DestructiveOp::StoreRestore, f.install())
        .unwrap();
}

// ---- audit -----------------------------------------------------------

#[test]
fn every_command_outcome_is_audited_including_refusals_and_dry_runs() {
    let f = fix();
    let t = f.issue(DestructiveOp::FactoryInit, TokenTarget::Terminal(tid(2)));
    let guest = ctx_as(Role::Guest, FrontDoor::Rest);
    let dry = Command::Operation(DefineOperation::new(OperationKind::Refresh, Target::All));
    let submitted: Vec<(CommandContext, Command)> = vec![
        (owner(), term(TerminalOp::Refresh, 2)),               // ok
        (owner(), dry),                                        // dry-run
        (guest, term(TerminalOp::Refresh, 2)),                 // guest-read-only
        (owner(), term(TerminalOp::ColdInitialize, 2)),        // token-missing
        (with(owner(), &t), term(TerminalOp::FactoryInit, 2)), // ok
        (with(owner(), &t), term(TerminalOp::FactoryInit, 2)), // token-reused
        (owner(), term(TerminalOp::Refresh, 77)),              // unknown-terminal, before policy
        (owner(), Command::Control(ControlCommand::Ping)),     // ok
    ];
    for (c, cmd) in &submitted {
        let _ = f.bus.submit(c, cmd.clone());
    }
    // The in-transaction "authorized" row (QA-3) is not a command outcome.
    let rows: Vec<_> = f
        .audit()
        .into_iter()
        .filter(|r| !r.entry.command.starts_with("issue-token") && r.entry.outcome != "authorized")
        .collect();
    assert_eq!(rows.len(), submitted.len(), "one row per command submitted");
    let outcomes: Vec<_> = rows.iter().map(|r| r.entry.outcome.as_str()).collect();
    assert_eq!(
        outcomes,
        [
            "ok",
            "dry-run",
            "guest-read-only",
            "token-missing",
            "ok",
            "token-reused",
            "unknown-terminal",
            "ok"
        ]
    );
    // QA-1: each row names the operation and the target (a terminal ID or a
    // plan digest, never a serial).
    let named: Vec<_> = rows
        .iter()
        .map(|r| (r.entry.command.as_str(), r.entry.target.as_deref()))
        .collect();
    let plan = format!(
        "plan:{}",
        plan_digest("op-refresh", &refs(&[1], &[1, 2, 3]))
    );
    assert_eq!(
        named,
        [
            ("refresh", Some("terminal:2")),
            ("op-refresh", Some(plan.as_str())),
            ("refresh", Some("terminal:2")),
            ("cold-initialize", Some("terminal:2")),
            ("factory-init", Some("terminal:2")),
            ("factory-init", Some("terminal:2")),
            ("refresh", Some("terminal:77")),
            ("ping", None),
        ]
    );
    // The token spend is journaled in its own transaction (QA-3).
    let auth: Vec<_> = f
        .audit()
        .into_iter()
        .filter(|r| r.entry.outcome == "authorized")
        .collect();
    assert_eq!(auth.len(), 1);
    assert_eq!(auth[0].entry.command, "factory-init");
    for r in f.audit() {
        let t = r.entry.target.unwrap_or_default();
        assert!(!t.contains("GI0000"), "no serial in a target: {t}");
    }
    // Who, role, front door, when.
    let r = &rows[2];
    assert_eq!(r.entry.actor, "tester");
    assert_eq!(r.entry.role, "Guest");
    assert_eq!(r.entry.front_door, "Rest");
    assert!(!r.ts_utc.is_empty());
    assert_eq!(f.policy.audit_failures(), 0);
}

#[test]
fn token_issuance_refusals_and_setting_changes_are_audited() {
    let f = fix();
    let _ = f.policy.issue_token(
        &owner(),
        &TokenRequest {
            op: DestructiveOp::FactoryInit,
            target: TokenTarget::Terminal(tid(2)),
            confirm_text: "nope".into(),
        },
    );
    let t = f.issue(DestructiveOp::CdlLockOff, f.install());
    f.policy.set_cdl_lock(&with(owner(), &t), false).unwrap();
    let _ = f.policy.set_cdl_lock(&owner(), false); // token-missing

    let issue = f.rows("issue-token:factory-init");
    assert_eq!(issue[0].entry.outcome, "confirm-text-mismatch");
    assert_eq!(f.rows("issue-token:cdl-lock-off")[0].entry.outcome, "ok");
    let lock = f.rows("set-cdl-lock");
    assert_eq!(lock.len(), 2);
    assert_eq!(lock[0].entry.outcome, "ok");
    assert_eq!(
        lock[0].entry.before.as_deref(),
        Some("{\"cdl_sacrificial_only\":true}")
    );
    assert_eq!(
        lock[0].entry.after.as_deref(),
        Some("{\"cdl_sacrificial_only\":false}")
    );
    assert_eq!(lock[1].entry.outcome, "token-missing");
}

#[test]
fn the_audit_log_stays_append_only() {
    let f = fix();
    let _ = f.bus.submit(&owner(), term(TerminalOp::Refresh, 2));
    let e = f
        .store
        .call_blocking(|s| {
            s.write(|tx| {
                tx.conn().execute("DELETE FROM audit_log", [])?;
                Ok(())
            })
        })
        .unwrap_err();
    assert!(
        matches!(e, crate::store::StoreError::AppendOnly { .. }),
        "{e:?}"
    );
}

#[test]
fn a_store_failure_fails_closed() {
    let f = fix();
    f.store
        .call_blocking(|s| {
            s.write(|tx| {
                tx.conn().execute_batch("DROP TABLE confirmation_token")?;
                Ok(())
            })
        })
        .unwrap();
    let c = with(owner(), &ConfirmationToken("x".into()));
    assert_eq!(
        rule(f.policy.check(&c, &term(TerminalOp::FactoryInit, 2), &[])),
        "policy-store-error"
    );
}

#[test]
fn a_destructive_bulk_token_does_not_cover_repeat_or_run_for() {
    let f = fix();
    let kind = OperationKind::Terminal(TerminalOp::ColdInitialize);
    let base = DefineOperation::new(kind, Target::All).execute();
    let Submitted::DryRun(rep) = f
        .bus
        .submit(
            &owner(),
            Command::Operation(DefineOperation::new(kind, Target::All)),
        )
        .unwrap()
    else {
        panic!("dry run");
    };
    let t = f.issue(
        DestructiveOp::ColdInitialize,
        TokenTarget::Plan(rep.plan_digest),
    );
    let mut repeating = base.clone();
    repeating.repeat_every = Some(Duration::from_secs(3600));
    let mut bounded = base.clone();
    bounded.run_for = Some(Duration::from_secs(3600));
    for def in [repeating, bounded] {
        assert_eq!(
            bus_rule(f.bus.submit(&with(owner(), &t), Command::Operation(def))),
            "destructive-bulk-no-repeat"
        );
    }
    // Refused before the token was touched: the one-shot still executes.
    assert!(
        f.bus
            .submit(&with(owner(), &t), Command::Operation(base))
            .is_ok()
    );
    // A non-destructive bulk may still repeat.
    let mut r = DefineOperation::new(OperationKind::Refresh, Target::All).execute();
    r.repeat_every = Some(Duration::from_secs(3600));
    assert!(f.bus.submit(&owner(), Command::Operation(r)).is_ok());
}

#[test]
fn authorize_names_the_right_rule_for_a_cdl_plan_in_both_lock_states() {
    let f = fix();
    let plan = TokenTarget::Plan(plan_digest("cdl-campaign", &[]));
    let t = f.issue(DestructiveOp::CdlCampaign, plan.clone());
    assert_eq!(
        rule(
            f.policy
                .authorize(&with(owner(), &t), DestructiveOp::CdlCampaign, plan.clone())
        ),
        "cdl-lock-non-sacrificial",
        "lock on: fails closed"
    );
    f.lock_off();
    assert_eq!(
        rule(
            f.policy
                .authorize(&owner(), DestructiveOp::CdlCampaign, plan.clone())
        ),
        "token-missing",
        "lock off: the lock is not the reason"
    );
    f.policy
        .authorize(&with(owner(), &t), DestructiveOp::CdlCampaign, plan)
        .unwrap();
}
