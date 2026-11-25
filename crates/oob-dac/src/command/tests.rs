//! Command-layer tests: one or more per #22 acceptance criterion and
//! design-note test-plan item. Box-free: the evidence is deterministic
//! state-machine, targeting and pacing tests (no box capture is claimed).
//!
//! The scheduler is a real `oob-mux` scheduler (so `PduId`s are real); the
//! sink wraps it to record what was submitted and to script refusals.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oob_mux::scheduler::BudgetConfig;
use oob_mux::{
    CarouselId, Clock, ManualClock, MonoTime, Outcome, OutcomeEvent, Payload, Pdu, PduId, Pid,
    Scheduler, SchedulerConfig, Submission, SubmitError, TrafficClass, UtcTime,
};

use crate::store::{NewTerminal, Store, StoreOptions, TerminalId, TerminalType, UnitAddress};

use super::catalog::{AutoOp, CatalogEntry, OperationKind, Support, TerminalOp};
use super::exec::{EncodeError, Encoder, Work};
use super::lifecycle::{OpStatus, Overlay};
use super::policy::{AllowAllPolicy, AuditNote, Grant, Policy, PolicyDenied};
use super::sink::{PduSink, SchedulerSink};
use super::target::{
    Filters, InMemoryDirectory, StoreDirectory, Target, TerminalDirectory, TerminalRef,
};
use super::{
    BusConfig, Command, CommandBus, CommandContext, CommandError, ConfirmationToken,
    ControlCommand, DefineOperation, FrontDoor, LabInjection, NonPduHandler, OperationId, Role,
    Submitted, TemplateId,
};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn tid(n: u32) -> TerminalId {
    TerminalId(n)
}

fn ctx() -> CommandContext {
    CommandContext {
        actor: "tester".into(),
        role: Role::Owner,
        front_door: FrontDoor::Rest,
        confirmation: None,
    }
}

// ---- doubles ---------------------------------------------------------

#[derive(Default)]
struct Rec {
    subs: Mutex<Vec<(PduId, Submission)>>,
    script: Mutex<VecDeque<SubmitError>>,
    always: Mutex<Option<SubmitError>>,
}

impl Rec {
    fn pdus(&self) -> Vec<(PduId, Pdu)> {
        self.subs
            .lock()
            .unwrap()
            .iter()
            .map(|(id, s)| match s {
                Submission::Once(p) | Submission::Cyclic { pdu: p, .. } => (*id, p.clone()),
            })
            .collect()
    }
    fn count(&self) -> usize {
        self.subs.lock().unwrap().len()
    }
}

struct RecSink {
    rec: Arc<Rec>,
    real: SchedulerSink,
}

impl PduSink for RecSink {
    fn submit(&self, s: Submission) -> Result<PduId, SubmitError> {
        if let Some(e) = *self.rec.always.lock().unwrap() {
            return Err(e);
        }
        if let Some(e) = self.rec.script.lock().unwrap().pop_front() {
            return Err(e);
        }
        let id = self.real.submit(s.clone())?;
        self.rec.subs.lock().unwrap().push((id, s));
        Ok(id)
    }
    fn withdraw(&self, id: CarouselId) -> Result<(), SubmitError> {
        self.real.withdraw(id)
    }
}

/// Phone panics, General errors, everything else encodes.
struct TestEncoder;

impl Encoder for TestEncoder {
    fn encode(&self, work: &Work, _t: Option<&TerminalRef>) -> Result<Pdu, EncodeError> {
        match work {
            Work::Terminal(TerminalOp::Phone) => panic!("boom: phone encoder"),
            Work::Terminal(TerminalOp::General) => Err(EncodeError {
                reason: "no template".into(),
            }),
            _ => Ok(Pdu {
                class: TrafficClass::C1Addressed,
                priority: 0,
                pid: Pid::new(0x0100).unwrap(),
                release_at: None,
                deadline: None,
                payload: Payload::Section(Arc::from(&[0xC0u8, 0x00, 0x02, 0x00, 0x00][..])),
                stamp: None,
            }),
        }
    }
}

struct Fix {
    bus: CommandBus,
    clock: Arc<ManualClock>,
    rec: Arc<Rec>,
    _sched: Scheduler,
}

fn terminals(n: u32) -> Vec<TerminalRef> {
    (1..=n)
        .map(|i| {
            let mut t = TerminalRef::new(
                tid(i),
                match i % 3 {
                    0 => TerminalType::TwoWay,
                    1 => TerminalType::OneWay,
                    _ => TerminalType::FoneWay,
                },
            );
            t.downstream_plant = Some(if i <= n / 2 { "plant-a" } else { "plant-b" }.into());
            t.upstream_plant = Some(if i % 4 == 0 { "up-2" } else { "up-1" }.into());
            t.business_system = Some(if i % 2 == 0 { "bsi-even" } else { "bsi-odd" }.into());
            t.radd = Some(format!("radd-{}", i % 2));
            t.rpd_channel = Some(if i % 5 == 0 { "rpd-2" } else { "rpd-1" }.into());
            t
        })
        .collect()
}

/// A release window wide enough that a whole campaign is handed over at
/// once; the just-in-time tests use their own small window.
fn wide() -> BusConfig {
    BusConfig {
        release_window: Duration::from_secs(3600),
        ..BusConfig::default()
    }
}

fn fix_with(n: u32, policy: Arc<dyn Policy>) -> Fix {
    fix_cfg(n, policy, wide())
}

fn fix_cfg(n: u32, policy: Arc<dyn Policy>, cfg: BusConfig) -> Fix {
    fix_full(
        n,
        policy,
        cfg,
        SchedulerConfig {
            queue_depth: 10_000,
            max_carousels: 4,
            outcome_capacity: 1024,
            budget: BudgetConfig::permissive(1000),
        },
    )
}

fn fix_sched(n: u32, cfg: BusConfig, sc: SchedulerConfig) -> Fix {
    fix_full(n, Arc::new(AllowAllPolicy), cfg, sc)
}

fn fix_full(n: u32, policy: Arc<dyn Policy>, cfg: BusConfig, sc: SchedulerConfig) -> Fix {
    let (sched, handle, _rx) = Scheduler::new(sc).unwrap();
    let clock = Arc::new(ManualClock::new(
        MonoTime::from_origin(ms(10_000)),
        UtcTime::from_unix(Duration::from_secs(1_000)),
    ));
    let rec = Arc::new(Rec::default());
    let sink = RecSink {
        rec: Arc::clone(&rec),
        real: SchedulerSink(handle),
    };
    let bus = CommandBus::from_sink(
        Box::new(sink),
        policy,
        Arc::new(InMemoryDirectory::new(terminals(n))),
        Arc::new(TestEncoder),
        clock.clone(),
    )
    .with_config(cfg);
    Fix {
        bus,
        clock,
        rec,
        _sched: sched,
    }
}

fn fix(n: u32) -> Fix {
    fix_with(n, Arc::new(AllowAllPolicy))
}

fn op(kind: OperationKind, target: Target, delay: u64) -> DefineOperation {
    DefineOperation {
        kind,
        target,
        filters: Filters::default(),
        delay_between: ms(delay),
        repeat_every: None,
        run_for: None,
        dry_run: false,
    }
}

fn queued(r: Result<Submitted, CommandError>) -> OperationId {
    match r {
        Ok(Submitted::Queued(id)) => id,
        other => panic!("expected Queued, got {other:?}"),
    }
}

fn emitted(id: PduId) -> OutcomeEvent {
    OutcomeEvent {
        id,
        outcome: Outcome::Emitted {
            first_packet_slot: 0,
            utc: UtcTime::default(),
        },
    }
}

// ---- catalog ---------------------------------------------------------

#[test]
fn every_manual_entry_is_present_and_names_are_unique() {
    assert_eq!(TerminalOp::ALL.len(), 19);
    assert_eq!(OperationKind::OWN.len(), 9);
    assert_eq!(OperationKind::all().len(), 9 + 19);
    assert_eq!(AutoOp::ALL.len(), 15);
    let mut names: Vec<&str> = TerminalOp::ALL.iter().map(CatalogEntry::name).collect();
    names.extend(OperationKind::OWN.iter().map(CatalogEntry::name));
    names.extend(AutoOp::ALL.iter().map(CatalogEntry::name));
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), n, "catalog names collide");
}

#[test]
fn enrollment_ops_come_first_in_the_confirmed_order() {
    assert_eq!(
        &TerminalOp::ALL[..3],
        &[
            TerminalOp::FactoryInit,
            TerminalOp::UnitCreation,
            TerminalOp::Connect
        ]
    );
}

#[test]
fn the_unsupported_set_is_exactly_the_documented_one() {
    let mut un: Vec<&str> = Vec::new();
    for o in TerminalOp::ALL {
        if !o.support().is_supported() {
            un.push(o.name());
        }
    }
    for k in OperationKind::OWN {
        if !k.support().is_supported() {
            un.push(k.name());
        }
    }
    for a in AutoOp::ALL {
        if !a.support().is_supported() {
            un.push(a.name());
        }
    }
    assert_eq!(
        un,
        [
            "network-dvb",
            "category-epoch-transition",
            "resource-ocam",
            "synchronize-cpms",
            "analog-rekey",
            "analog-clear-service",
            "macrovision",
            "cablecard-snt",
        ]
    );
}

#[test]
fn each_entry_is_accepted_or_rejected_by_name_with_its_reason() {
    let f = fix(5);
    for o in TerminalOp::ALL {
        // Phone panics and General errors in the test encoder; those are
        // execution outcomes, not catalog refusals.
        let r = f.bus.submit(
            &ctx(),
            Command::Terminal {
                op: o,
                terminal: tid(1),
            },
        );
        match o.support() {
            Support::Unsupported { reason } => assert!(
                matches!(&r, Err(CommandError::Unsupported { what, reason: why })
                    if *what == o.name() && *why == reason),
                "{o:?}: {r:?}"
            ),
            Support::Supported => assert!(
                !matches!(&r, Err(CommandError::Unsupported { .. })),
                "{o:?}: {r:?}"
            ),
        }
    }
    for k in OperationKind::all() {
        let mut d = op(k, Target::All, 250);
        d.dry_run = true;
        let r = f.bus.submit(&ctx(), Command::Operation(d));
        match k.support() {
            Support::Unsupported { reason } => assert!(
                matches!(&r, Err(CommandError::Unsupported { what, reason: why })
                    if *what == k.name() && *why == reason),
                "{k:?}: {r:?}"
            ),
            Support::Supported => assert!(matches!(r, Ok(Submitted::DryRun(_))), "{k:?}: {r:?}"),
        }
    }
    for a in AutoOp::ALL {
        let r = f.bus.submit(&ctx(), Command::Auto(a));
        match a.support() {
            Support::Unsupported { reason } => assert!(
                matches!(&r, Err(CommandError::Unsupported { what, reason: why })
                    if *what == a.name() && *why == reason),
                "{a:?}: {r:?}"
            ),
            Support::Supported => assert!(matches!(r, Ok(Submitted::Queued(_))), "{a:?}: {r:?}"),
        }
    }
}

#[test]
fn unsupported_reasons_are_stable_codes_and_never_reach_the_scheduler() {
    let f = fix(3);
    let e = f
        .bus
        .submit(
            &ctx(),
            Command::Terminal {
                op: TerminalOp::NetworkDvb,
                terminal: tid(1),
            },
        )
        .unwrap_err();
    assert_eq!(e.reason(), "unsupported");
    assert!(e.to_string().contains("DVB only"));
    assert_eq!(f.rec.count(), 0);
}

// ---- targeting -------------------------------------------------------

fn ids(r: &[TerminalRef]) -> Vec<u32> {
    r.iter().map(|t| t.id.get()).collect()
}

#[test]
fn all_range_and_single() {
    let d = InMemoryDirectory::new(terminals(10));
    let none = Filters::default();
    assert_eq!(
        ids(&d.resolve(&Target::All, &none).unwrap()),
        (1..=10).collect::<Vec<_>>()
    );
    assert_eq!(
        ids(&d.resolve(&Target::IdRange(tid(3)..=tid(6)), &none).unwrap()),
        [3, 4, 5, 6]
    );
    assert_eq!(
        ids(&d.resolve(&Target::Single(tid(7)), &none).unwrap()),
        [7]
    );
    assert!(matches!(
        d.resolve(&Target::Single(tid(99)), &none),
        Err(CommandError::UnknownTerminal(t)) if t.get() == 99
    ));
    assert!(matches!(
        d.resolve(&Target::IdRange(tid(50)..=tid(60)), &none),
        Err(CommandError::EmptyTarget)
    ));
    assert!(matches!(
        d.resolve(&Target::IdRange(tid(6)..=tid(3)), &none),
        Err(CommandError::EmptyTarget)
    ));
}

#[test]
fn each_filter_narrows_every_target_kind() {
    let d = InMemoryDirectory::new(terminals(12));
    type Pred = Box<dyn Fn(&TerminalRef) -> bool>;
    let cases: Vec<(Filters, Pred)> = vec![
        (
            Filters {
                radd: Some("radd-1".into()),
                ..Filters::default()
            },
            Box::new(|t| t.radd.as_deref() == Some("radd-1")),
        ),
        (
            Filters {
                downstream_plant: Some("plant-a".into()),
                ..Filters::default()
            },
            Box::new(|t| t.downstream_plant.as_deref() == Some("plant-a")),
        ),
        (
            Filters {
                upstream_plant: Some("up-1".into()),
                ..Filters::default()
            },
            Box::new(|t| t.upstream_plant.as_deref() == Some("up-1")),
        ),
        (
            Filters {
                terminal_type: Some(TerminalType::TwoWay),
                ..Filters::default()
            },
            Box::new(|t| t.terminal_type == TerminalType::TwoWay),
        ),
        (
            Filters {
                rpd_channel: Some("rpd-1".into()),
                ..Filters::default()
            },
            Box::new(|t| t.rpd_channel.as_deref() == Some("rpd-1")),
        ),
        (
            Filters {
                business_system: Some("bsi-even".into()),
                ..Filters::default()
            },
            Box::new(|t| t.business_system.as_deref() == Some("bsi-even")),
        ),
    ];
    let all = terminals(12);
    for (f, pred) in &cases {
        let want: Vec<u32> = all.iter().filter(|t| pred(t)).map(|t| t.id.get()).collect();
        assert!(
            !want.is_empty() && want.len() < 12,
            "fixture must discriminate: {f:?}"
        );
        assert_eq!(
            ids(&d.resolve(&Target::All, f).unwrap()),
            want,
            "All + {f:?}"
        );
        let ranged: Vec<u32> = want
            .iter()
            .copied()
            .filter(|i| (4..=9).contains(i))
            .collect();
        assert_eq!(
            ids(&d
                .resolve(&Target::IdRange(tid(4)..=tid(9)), f)
                .unwrap_or_default()),
            ranged,
            "range + {f:?}"
        );
        let first = want[0];
        assert_eq!(
            ids(&d.resolve(&Target::Single(tid(first)), f).unwrap()),
            [first]
        );
    }
}

#[test]
fn filters_combine_with_and_and_a_filtered_out_single_is_empty_not_unknown() {
    let d = InMemoryDirectory::new(terminals(12));
    let f = Filters {
        terminal_type: Some(TerminalType::OneWay),
        downstream_plant: Some("plant-b".into()),
        ..Filters::default()
    };
    let got = d.resolve(&Target::All, &f).unwrap();
    assert!(got.iter().all(|t| t.terminal_type == TerminalType::OneWay
        && t.downstream_plant.as_deref() == Some("plant-b")));
    // Terminal 2 exists (FoneWay): excluded by the filter -> EmptyTarget.
    assert!(matches!(
        d.resolve(&Target::Single(tid(2)), &f),
        Err(CommandError::EmptyTarget)
    ));
}

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "oob-dac-command-{}-{}",
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

#[test]
fn the_store_backed_directory_resolves_and_refuses_unknown_filters_by_name() {
    let dir = TempDir::new();
    let mut store = Store::open(&dir.0.join("t.sqlite"), &StoreOptions::default()).unwrap();
    let mut made = Vec::new();
    for n in 1..=6u64 {
        let mut t = NewTerminal::new(
            UnitAddress::new(n, 0).unwrap(), // synthetic
            format!("GI{n:010}"),
            if n % 2 == 0 {
                TerminalType::TwoWay
            } else {
                TerminalType::OneWay
            },
            "dct2000",
        );
        t.downstream_plant = Some(if n <= 3 { "p1" } else { "p2" }.into());
        t.bsi_name = Some("bsi".into());
        made.push(store.write(|tx| tx.create_terminal("test", &t)).unwrap());
    }
    let d = StoreDirectory::new(store.reader().unwrap());
    assert_eq!(
        d.resolve(&Target::All, &Filters::default()).unwrap().len(),
        6
    );
    let f = Filters {
        downstream_plant: Some("p2".into()),
        terminal_type: Some(TerminalType::TwoWay),
        ..Filters::default()
    };
    assert_eq!(
        ids(&d.resolve(&Target::All, &f).unwrap()),
        [made[3].get(), made[5].get()]
    );
    assert_eq!(
        ids(&d
            .resolve(
                &Target::IdRange(made[1]..=made[2]),
                &Filters {
                    business_system: Some("bsi".into()),
                    ..Filters::default()
                }
            )
            .unwrap()),
        [made[1].get(), made[2].get()]
    );
    assert!(matches!(
        d.resolve(&Target::Single(tid(999)), &Filters::default()),
        Err(CommandError::UnknownTerminal(_))
    ));
    let e = d
        .resolve(
            &Target::All,
            &Filters {
                radd: Some("r".into()),
                ..Filters::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        e,
        CommandError::Unsupported {
            what: "filter:radd",
            ..
        }
    ));
    let e = d
        .resolve(
            &Target::All,
            &Filters {
                rpd_channel: Some("r".into()),
                ..Filters::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        e,
        CommandError::Unsupported {
            what: "filter:rpd-channel",
            ..
        }
    ));
}

// ---- pacing ----------------------------------------------------------

#[test]
fn a_50_unit_campaign_releases_pdus_at_least_250_ms_apart() {
    let f = fix(50);
    let t0 = f.clock.mono();
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    let pdus = f.rec.pdus();
    assert_eq!(pdus.len(), 50);
    let times: Vec<MonoTime> = pdus.iter().map(|(_, p)| p.release_at.unwrap()).collect();
    assert_eq!(times[0], t0, "the first releases now");
    for w in times.windows(2) {
        assert!(
            w[1].saturating_since(w[0]) >= ms(250),
            "spacing below 250 ms"
        );
        assert_eq!(
            w[1].saturating_since(w[0]),
            ms(250),
            "exactly delay_between"
        );
    }
    assert!(
        pdus.iter()
            .all(|(_, p)| p.class == TrafficClass::C1Addressed)
    );
    assert!(
        f.rec
            .subs
            .lock()
            .unwrap()
            .iter()
            .all(|(_, s)| matches!(s, Submission::Once(_))),
        "spacing is a release time on one-shot PDUs and nothing else"
    );
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
    assert_eq!(f.bus.status(id).unwrap().outstanding, 50);
}

#[test]
fn spacing_follows_delay_between_when_it_is_larger() {
    let f = fix(5);
    queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::DiagnosticPoll, Target::All, 1_000)),
    ));
    let times: Vec<_> = f
        .rec
        .pdus()
        .iter()
        .map(|(_, p)| p.release_at.unwrap())
        .collect();
    for w in times.windows(2) {
        assert_eq!(w[1].saturating_since(w[0]), ms(1_000));
    }
}

#[test]
fn a_delay_of_249_ms_is_rejected_before_anything_is_sent() {
    let f = fix(5);
    let e = f
        .bus
        .submit(
            &ctx(),
            Command::Operation(op(OperationKind::Refresh, Target::All, 249)),
        )
        .unwrap_err();
    assert!(matches!(e, CommandError::DelayBelowMinimum { .. }));
    assert_eq!(e.reason(), "delay-below-minimum");
    assert_eq!(f.rec.count(), 0);
    assert!(f.bus.operations().is_empty());
}

#[test]
fn a_single_terminal_operation_releases_now() {
    let f = fix(5);
    let t0 = f.clock.mono();
    queued(f.bus.submit(
        &ctx(),
        Command::Terminal {
            op: TerminalOp::Refresh,
            terminal: tid(2),
        },
    ));
    let p = f.rec.pdus();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].1.release_at, Some(t0));
    assert_eq!(
        p[0].1.deadline,
        Some(t0.saturating_add(Duration::from_secs(60)))
    );
}

#[test]
fn repeat_every_replans_the_whole_set_each_period() {
    let f = fix(4);
    let mut d = op(OperationKind::Refresh, Target::All, 250);
    d.repeat_every = Some(ms(2_000));
    let t0 = f.clock.mono();
    let id = queued(f.bus.submit(&ctx(), Command::Operation(d)));
    assert_eq!(f.rec.count(), 4);
    // Emit repetition 1 entirely.
    for (pid, _) in f.rec.pdus() {
        f.bus.on_outcome(emitted(pid));
    }
    let v = f.bus.status(id).unwrap();
    assert_eq!(
        v.status,
        OpStatus::Downloaded,
        "between repetitions: Active -> Downloaded"
    );
    assert_eq!(v.repetition, 1);
    // Before the period: nothing new.
    f.clock.advance(ms(1_999));
    f.bus.tick();
    assert_eq!(f.rec.count(), 4);
    // At the period: repetition 2, planned from the slot (t0 + 2 s).
    f.clock.advance(ms(1));
    f.bus.tick();
    assert_eq!(f.rec.count(), 8);
    assert_eq!(f.bus.status(id).unwrap().repetition, 2);
    let second: Vec<_> = f.rec.pdus()[4..]
        .iter()
        .map(|(_, p)| p.release_at.unwrap())
        .collect();
    assert_eq!(second[0], t0.saturating_add(ms(2_000)));
    for w in second.windows(2) {
        assert_eq!(w[1].saturating_since(w[0]), ms(250));
    }
    // And back-to-back repetitions keep the gap.
    let first_last = f.rec.pdus()[3].1.release_at.unwrap();
    assert!(second[0].saturating_since(first_last) >= ms(250));
}

#[test]
fn a_repeat_period_shorter_than_one_campaign_is_rejected() {
    let f = fix(10);
    let mut d = op(OperationKind::Refresh, Target::All, 250);
    d.repeat_every = Some(ms(2_499));
    let e = f.bus.submit(&ctx(), Command::Operation(d)).unwrap_err();
    assert!(matches!(e, CommandError::RepeatTooShort { .. }));
    assert_eq!(f.rec.count(), 0);
}

#[test]
fn a_dry_run_reports_and_sends_nothing() {
    let f = fix(10);
    let t0 = f.clock.mono();
    let mut d = op(OperationKind::Refresh, Target::All, 300);
    d.dry_run = true;
    let r = f.bus.submit(&ctx(), Command::Operation(d)).unwrap();
    let Submitted::DryRun(rep) = r else {
        panic!("{r:?}")
    };
    assert_eq!(rep.terminals, 10);
    assert_eq!(rep.first_release, t0);
    assert_eq!(rep.last_release, t0.saturating_add(ms(2_700)));
    assert_eq!(rep.class, TrafficClass::C1Addressed);
    assert_eq!(f.rec.count(), 0);
    assert!(f.bus.operations().is_empty());
}

#[test]
fn an_empty_target_is_an_error_not_a_noop() {
    let f = fix(5);
    let mut d = op(OperationKind::Refresh, Target::All, 250);
    d.filters.business_system = Some("nobody".into());
    assert!(matches!(
        f.bus.submit(&ctx(), Command::Operation(d)),
        Err(CommandError::EmptyTarget)
    ));
    assert!(matches!(
        f.bus.submit(
            &ctx(),
            Command::Terminal {
                op: TerminalOp::Refresh,
                terminal: tid(77)
            }
        ),
        Err(CommandError::UnknownTerminal(_))
    ));
}

// ---- lifecycle through the bus ---------------------------------------

#[test]
fn a_one_way_operation_completes_when_its_last_pdu_is_emitted() {
    let f = fix(3); // terminal 1 is One-Way (i % 3 == 1)
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    let pdus = f.rec.pdus();
    let s = |f: &Fix| f.bus.status(id).unwrap();
    assert_eq!(s(&f).status, OpStatus::Downloaded);
    f.bus.on_outcome(emitted(pdus[0].0));
    assert_eq!(s(&f).status, OpStatus::Active, "first emission");
    f.bus.on_outcome(emitted(pdus[1].0));
    assert_eq!(s(&f).status, OpStatus::Active);
    f.bus.on_outcome(emitted(pdus[2].0));
    assert_eq!(
        s(&f).status,
        OpStatus::Completed,
        "no terminal reply involved"
    );
    assert_eq!(s(&f).emitted, 3);
}

#[test]
fn a_single_pdu_operation_walks_request_sent_to_completed() {
    let f = fix(2);
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Terminal {
            op: TerminalOp::Initialize,
            terminal: tid(1),
        },
    ));
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
    f.bus.on_outcome(emitted(f.rec.pdus()[0].0));
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Completed);
}

#[test]
fn an_expired_pdu_fails_the_operation_and_late_outcomes_are_ignored() {
    let f = fix(3);
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    let pdus = f.rec.pdus();
    f.bus.on_outcome(emitted(pdus[0].0));
    f.bus.on_outcome(OutcomeEvent {
        id: pdus[1].0,
        outcome: Outcome::Expired,
    });
    let v = f.bus.status(id).unwrap();
    assert_eq!(
        v.status,
        OpStatus::Failed,
        "the generic failure after Downloaded"
    );
    assert!(v.failure.is_some());
    f.bus.on_outcome(emitted(pdus[2].0));
    assert_eq!(
        f.bus.status(id).unwrap().status,
        OpStatus::Failed,
        "final stays final"
    );
    assert!(f.bus.log().iter().any(|l| l.event == "pdu-expired"));
}

#[test]
fn an_active_operation_past_its_end_time_is_marked_completed() {
    let f = fix(4);
    let mut d = op(OperationKind::Refresh, Target::All, 250);
    d.run_for = Some(ms(500));
    let id = queued(f.bus.submit(&ctx(), Command::Operation(d)));
    f.bus.on_outcome(emitted(f.rec.pdus()[0].0));
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Active);
    f.clock.advance(ms(499));
    f.bus.tick();
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Active);
    f.clock.advance(ms(1));
    f.bus.tick();
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::MarkedCompleted);
    // Late emissions of the PDUs still queued do not reopen it.
    f.bus.on_outcome(emitted(f.rec.pdus()[1].0));
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::MarkedCompleted);
}

#[test]
fn a_repeating_operation_between_repetitions_is_closed_out_at_its_end_time() {
    let f = fix(2);
    let mut d = op(OperationKind::Refresh, Target::All, 250);
    d.repeat_every = Some(ms(1_000));
    d.run_for = Some(ms(1_500));
    let id = queued(f.bus.submit(&ctx(), Command::Operation(d)));
    for (pid, _) in f.rec.pdus() {
        f.bus.on_outcome(emitted(pid));
    }
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
    f.clock.advance(ms(1_600));
    f.bus.tick();
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::MarkedCompleted);
    assert_eq!(f.rec.count(), 2, "no further repetition after the end time");
}

#[test]
fn cancel_fails_a_non_final_operation_once() {
    let f = fix(3);
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    f.bus.cancel(&ctx(), id).unwrap();
    let v = f.bus.status(id).unwrap();
    assert_eq!(
        (v.status, v.failure.as_deref()),
        (OpStatus::Failed, Some("cancelled"))
    );
    assert!(matches!(
        f.bus.cancel(&ctx(), id),
        Err(CommandError::IllegalTransition {
            from: OpStatus::Failed,
            ..
        })
    ));
    assert!(matches!(
        f.bus.cancel(&ctx(), OperationId(999)),
        Err(CommandError::UnknownOperation(_))
    ));
}

#[test]
fn device_offline_shows_while_the_link_is_down_and_clears_on_reconnect() {
    let f = fix(3);
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    assert_eq!(f.bus.status(id).unwrap().overlay, None);
    f.bus.set_link(false);
    let v = f.bus.status(id).unwrap();
    assert_eq!(
        (v.status, v.overlay),
        (OpStatus::Downloaded, Some(Overlay::DeviceOffline))
    );
    // An operation created while down shows it too.
    let id2 = queued(f.bus.submit(
        &ctx(),
        Command::Terminal {
            op: TerminalOp::Refresh,
            terminal: tid(1),
        },
    ));
    assert_eq!(
        f.bus.status(id2).unwrap().overlay,
        Some(Overlay::DeviceOffline)
    );
    f.bus.set_link(true);
    assert_eq!(f.bus.status(id).unwrap().overlay, None);
    assert_eq!(f.bus.status(id2).unwrap().overlay, None);
    assert_eq!(
        f.bus.status(id).unwrap().status,
        OpStatus::Downloaded,
        "prior status kept"
    );
}

#[test]
fn retrieve_status_is_transient_and_finds_unknown() {
    let f = fix(3);
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    let v = f.bus.retrieve_status(id).unwrap();
    assert_eq!(
        (v.status, v.overlay),
        (OpStatus::Downloaded, None),
        "Retrieving Status is never read back"
    );
    // Simulate a restart that lost the scheduler's PDUs: none outstanding, none emitted.
    f.bus.lock().ops.get_mut(&id).unwrap().outstanding = 0;
    let v = f.bus.retrieve_status(id).unwrap();
    assert_eq!(v.overlay, Some(Overlay::Unknown));
    assert!(matches!(
        f.bus.retrieve_status(OperationId(9)),
        Err(CommandError::UnknownOperation(_))
    ));
    // A final operation has no overlay.
    f.bus.cancel(&ctx(), id).unwrap();
    assert_eq!(f.bus.retrieve_status(id).unwrap().overlay, None);
}

// ---- failure containment ---------------------------------------------

#[test]
fn an_encoder_panic_fails_that_command_is_logged_and_the_process_continues() {
    let f = fix(5);
    let e = f
        .bus
        .submit(
            &ctx(),
            Command::Terminal {
                op: TerminalOp::Phone,
                terminal: tid(1),
            },
        )
        .unwrap_err();
    assert!(matches!(e.root(), CommandError::EncoderPanic { work } if work == "phone"));
    assert_eq!(
        e.operation(),
        Some(f.bus.operations()[0].id),
        "the failed id is carried"
    );
    assert_eq!(e.reason(), "encoder-panic");
    assert_eq!(f.rec.count(), 0, "nothing reached the scheduler");
    let ops = f.bus.operations();
    assert_eq!(ops[0].status, OpStatus::RequestSentFailed);
    assert!(ops[0].failure.as_deref().unwrap().contains("boom"));
    let log = f.bus.log();
    assert!(
        log.iter()
            .any(|l| l.event == "encoder-panic" && l.detail.contains("boom"))
    );
    // The bus and the process carry on: the next command works.
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Terminal {
            op: TerminalOp::Refresh,
            terminal: tid(2),
        },
    ));
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
}

#[test]
fn a_panicking_command_does_not_disturb_a_concurrent_one() {
    let f = fix(60);
    std::thread::scope(|s| {
        let bad = s.spawn(|| {
            for _ in 0..20 {
                let r = f.bus.submit(
                    &ctx(),
                    Command::Terminal {
                        op: TerminalOp::Phone,
                        terminal: tid(1),
                    },
                );
                assert!(matches!(
                    r.as_ref().map_err(CommandError::root),
                    Err(CommandError::EncoderPanic { .. })
                ));
            }
        });
        let good = s.spawn(|| {
            queued(f.bus.submit(
                &ctx(),
                Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
            ))
        });
        bad.join().unwrap();
        let id = good.join().unwrap();
        assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
    });
    let pdus = f.rec.pdus();
    assert_eq!(pdus.len(), 60);
    let failed = f
        .bus
        .operations()
        .iter()
        .filter(|o| o.status == OpStatus::RequestSentFailed)
        .count();
    assert_eq!(failed, 20);
}

#[test]
fn an_encoder_error_is_request_sent_failed_and_named() {
    let f = fix(3);
    let e = f
        .bus
        .submit(
            &ctx(),
            Command::Terminal {
                op: TerminalOp::General,
                terminal: tid(1),
            },
        )
        .unwrap_err();
    assert!(
        matches!(e.root(), CommandError::Encode { work, reason } if work == "general" && reason == "no template")
    );
    assert_eq!(f.bus.operations()[0].status, OpStatus::RequestSentFailed);
    assert!(f.bus.log().iter().any(|l| l.event == "encode-error"));
}

#[test]
fn a_scheduler_refusal_is_request_sent_failed_and_a_full_queue_times_out() {
    let f = fix(3);
    *f.rec.always.lock().unwrap() = Some(SubmitError::ClassDisabled(TrafficClass::C1Addressed));
    let e = f
        .bus
        .submit(
            &ctx(),
            Command::Terminal {
                op: TerminalOp::Refresh,
                terminal: tid(1),
            },
        )
        .unwrap_err();
    assert_eq!(e.reason(), "scheduler-refused");
    assert_eq!(f.bus.operations()[0].status, OpStatus::RequestSentFailed);

    // Queue full: retries (3) are exhausted -> Timeout.
    *f.rec.always.lock().unwrap() = Some(SubmitError::QueueFull {
        class: TrafficClass::C1Addressed,
        depth: 1,
    });
    let e = f
        .bus
        .submit(
            &ctx(),
            Command::Terminal {
                op: TerminalOp::Refresh,
                terminal: tid(1),
            },
        )
        .unwrap_err();
    assert_eq!(e.reason(), "scheduler-refused");
    assert_eq!(f.bus.operations()[1].status, OpStatus::Timeout);
}

#[test]
fn a_transient_queue_full_is_retried_with_the_release_time_preserved() {
    let f = fix(1);
    {
        let mut s = f.rec.script.lock().unwrap();
        for _ in 0..3 {
            s.push_back(SubmitError::QueueFull {
                class: TrafficClass::C1Addressed,
                depth: 1,
            });
        }
    }
    let t0 = f.clock.mono();
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Terminal {
            op: TerminalOp::Refresh,
            terminal: tid(1),
        },
    ));
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
    assert_eq!(f.rec.pdus()[0].1.release_at, Some(t0));
}

// ---- policy, lab, auto ------------------------------------------------

#[derive(Default)]
struct Spy {
    deny: bool,
    seen: Mutex<Vec<(&'static str, usize)>>,
    audits: Mutex<Vec<AuditNote>>,
}

impl Policy for Spy {
    fn check(
        &self,
        _c: &CommandContext,
        cmd: &Command,
        resolved: &[TerminalRef],
    ) -> Result<Grant, PolicyDenied> {
        self.seen
            .lock()
            .unwrap()
            .push((cmd.kind_name(), resolved.len()));
        if self.deny {
            Err(PolicyDenied {
                reason: "guest-refused",
                detail: "guests cannot write".into(),
            })
        } else {
            Ok(Grant::new())
        }
    }
    fn audit(&self, _c: &CommandContext, n: &AuditNote) {
        self.audits.lock().unwrap().push(n.clone());
    }
}

#[test]
fn policy_sees_every_command_with_its_resolved_targets_and_can_refuse() {
    let spy = Arc::new(Spy::default());
    let f = fix_with(8, spy.clone());
    queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    queued(
        f.bus
            .submit(&ctx(), Command::Auto(AutoOp::InsertSystemTime)),
    );
    f.bus
        .submit(&ctx(), Command::Control(ControlCommand::Ping))
        .unwrap();
    assert_eq!(
        *spy.seen.lock().unwrap(),
        [("operation", 8), ("auto", 0), ("control", 0)]
    );
    assert_eq!(
        spy.audits
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.outcome)
            .collect::<Vec<_>>(),
        ["ok", "ok", "ok"]
    );

    let deny = Arc::new(Spy {
        deny: true,
        ..Spy::default()
    });
    let f = fix_with(8, deny.clone());
    let e = f
        .bus
        .submit(
            &ctx(),
            Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
        )
        .unwrap_err();
    assert!(matches!(&e, CommandError::Policy(p) if p.reason == "guest-refused"));
    assert_eq!(e.reason(), "guest-refused");
    assert_eq!(
        f.rec.count(),
        0,
        "a refused command never reaches the scheduler"
    );
    assert!(f.bus.operations().is_empty());
    // The refusal is audited too; so is a validation failure that never reached policy.
    let _ = f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 1)),
    );
    let outs: Vec<_> = deny
        .audits
        .lock()
        .unwrap()
        .iter()
        .map(|a| a.outcome)
        .collect();
    assert_eq!(outs, ["guest-refused", "delay-below-minimum"]);
}

#[test]
fn the_confirmation_token_never_prints() {
    let c = ConfirmationToken("s3cret-value".into());
    assert!(!format!("{c:?}").contains("s3cret"));
}

#[test]
fn lab_injection_needs_lab_mode_and_travels_in_c5() {
    let f = fix(4);
    let inj = LabInjection {
        template: TemplateId("probe-1".into()),
        target: Target::IdRange(tid(1)..=tid(3)),
        filters: Filters::default(),
    };
    let e = f.bus.submit(&ctx(), Command::Lab(inj.clone())).unwrap_err();
    assert!(matches!(e, CommandError::LabModeOff));
    assert_eq!(f.rec.count(), 0);
    f.bus.set_lab_mode(true);
    let id = queued(f.bus.submit(&ctx(), Command::Lab(inj)));
    let pdus = f.rec.pdus();
    assert_eq!(pdus.len(), 3);
    assert!(pdus.iter().all(|(_, p)| p.class == TrafficClass::C5Lab));
    let t: Vec<_> = pdus.iter().map(|(_, p)| p.release_at.unwrap()).collect();
    assert!(
        t.windows(2).all(|w| w[1].saturating_since(w[0]) >= ms(250)),
        "lab spacing too"
    );
    assert_eq!(f.bus.status(id).unwrap().family, "lab:probe-1");
}

#[test]
fn auto_operations_pick_their_class_and_carry_no_release_time() {
    let f = fix(2);
    queued(
        f.bus
            .submit(&ctx(), Command::Auto(AutoOp::InsertSystemTime)),
    );
    queued(f.bus.submit(&ctx(), Command::Auto(AutoOp::CodeDownload)));
    queued(
        f.bus
            .submit(&ctx(), Command::Auto(AutoOp::GlobalAuthRefresh)),
    );
    let p = f.rec.pdus();
    assert_eq!(
        p.iter().map(|(_, p)| p.class).collect::<Vec<_>>(),
        [
            TrafficClass::C0System,
            TrafficClass::C4Cdl,
            TrafficClass::C1Addressed
        ]
    );
    assert!(
        p.iter()
            .all(|(_, p)| p.release_at.is_none() && p.deadline.is_none())
    );
}

// ---- the non-PDU dispatch seam (#25) ----------------------------------

#[derive(Default)]
struct Recorder {
    got: Mutex<Vec<String>>,
}

impl NonPduHandler for Recorder {
    fn handle(&self, c: &CommandContext, cmd: &Command) -> Result<Submitted, CommandError> {
        self.got
            .lock()
            .unwrap()
            .push(format!("{}:{cmd:?}", c.actor));
        Ok(Submitted::Done)
    }
}

#[test]
fn a_non_pdu_command_is_dispatched_through_the_seam_after_policy() {
    let rec = Arc::new(Recorder::default());
    let spy = Arc::new(Spy::default());
    let f = fix_with(2, spy.clone());
    let bus = f.bus.with_non_pdu_handler(rec.clone());
    let r = bus
        .submit(&ctx(), Command::Control(ControlCommand::Ping))
        .unwrap();
    assert_eq!(r, Submitted::Done);
    assert_eq!(rec.got.lock().unwrap().len(), 1);
    assert!(rec.got.lock().unwrap()[0].contains("Ping"));
    assert_eq!(spy.seen.lock().unwrap()[0].0, "control", "policy ran first");
    assert_eq!(f.rec.count(), 0, "no PDU for a non-PDU command");

    // A denying policy stops it before the handler.
    let rec2 = Arc::new(Recorder::default());
    let f2 = fix_with(
        2,
        Arc::new(Spy {
            deny: true,
            ..Spy::default()
        }),
    );
    let bus2 = f2.bus.with_non_pdu_handler(rec2.clone());
    assert!(matches!(
        bus2.submit(&ctx(), Command::Control(ControlCommand::Ping)),
        Err(CommandError::Policy(_))
    ));
    assert!(rec2.got.lock().unwrap().is_empty());
}

#[test]
fn the_default_handler_answers_ping() {
    let f = fix(1);
    assert_eq!(
        f.bus
            .submit(&ctx(), Command::Control(ControlCommand::Ping))
            .unwrap(),
        Submitted::Done
    );
}

// ---- the scheduler is reachable only through the command layer --------

#[test]
fn the_scheduler_is_named_only_in_the_command_layer() {
    // `SchedulerHandle` is `Clone` and public in oob-mux, so this is a
    // convention guarded by a source scan, not by the type: nothing outside
    // `command/` may name oob-mux or a scheduler type.
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src.clone()];
    let mut offenders = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let rel = p
                    .strip_prefix(&src)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = std::fs::read_to_string(&p).unwrap();
                if !rel.starts_with("command/")
                    && (text.contains("oob_mux") || text.contains("Scheduler"))
                {
                    offenders.push(rel);
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "oob_mux or Scheduler named outside command/: {offenders:?}"
    );
}

// ---- just-in-time release (QA-1, QA-2, QA-3) --------------------------

fn jit() -> BusConfig {
    BusConfig {
        release_window: ms(500),
        ..BusConfig::default()
    }
}

impl Fix {
    /// Advances the clock by `step` and ticks, `n` times.
    fn run_ticks(&self, n: u32, step: Duration) {
        for _ in 0..n {
            self.clock.advance(step);
            self.bus.tick();
        }
    }
}

#[test]
fn pdus_are_handed_to_the_scheduler_only_as_their_window_arrives() {
    let f = fix_cfg(10, Arc::new(AllowAllPolicy), jit());
    let t0 = f.clock.mono();
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    // Window 500 ms: releases at +0, +250, +500 only.
    assert_eq!(f.rec.count(), 3);
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
    f.run_ticks(1, ms(250));
    assert_eq!(f.rec.count(), 4);
    f.run_ticks(20, ms(250));
    let times: Vec<_> = f
        .rec
        .pdus()
        .iter()
        .map(|(_, p)| p.release_at.unwrap())
        .collect();
    assert_eq!(times.len(), 10);
    assert_eq!(times[0], t0);
    for w in times.windows(2) {
        assert_eq!(w[1].saturating_since(w[0]), ms(250));
    }
}

#[test]
fn a_50_unit_campaign_through_the_default_window_keeps_250_ms() {
    let f = fix_cfg(50, Arc::new(AllowAllPolicy), BusConfig::default());
    queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    f.run_ticks(60, ms(250));
    let times: Vec<_> = f
        .rec
        .pdus()
        .iter()
        .map(|(_, p)| p.release_at.unwrap())
        .collect();
    assert_eq!(times.len(), 50);
    assert!(
        times
            .windows(2)
            .all(|w| w[1].saturating_since(w[0]) >= ms(250))
    );
}

#[test]
fn a_late_tick_retimes_pdus_so_spacing_never_shrinks() {
    let f = fix_cfg(10, Arc::new(AllowAllPolicy), jit());
    queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    f.clock.advance(Duration::from_secs(10)); // the daemon stalled
    f.bus.tick();
    let now = f.clock.mono();
    let times: Vec<_> = f
        .rec
        .pdus()
        .iter()
        .map(|(_, p)| p.release_at.unwrap())
        .collect();
    assert_eq!(
        times.len(),
        10,
        "every planned release is past, so all are handed over"
    );
    assert!(
        times[3..].iter().all(|r| *r >= now),
        "late PDUs are re-timed to now or later"
    );
    assert!(
        times
            .windows(2)
            .all(|w| w[1].saturating_since(w[0]) >= ms(250))
    );
}

#[test]
fn a_campaign_larger_than_the_scheduler_queue_is_accepted_and_a_stalled_queue_stops_the_feed() {
    // QA-1: real scheduler, queue_depth 4, 10 terminals, nothing drains it.
    let f = fix_sched(
        10,
        jit(),
        SchedulerConfig {
            queue_depth: 4,
            max_carousels: 1,
            outcome_capacity: 8,
            budget: BudgetConfig::permissive(1000),
        },
    );
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    assert_eq!(
        f.rec.count(),
        3,
        "10 > depth 4, yet accepted: only the window is queued"
    );
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
    f.run_ticks(1, ms(250));
    assert_eq!(f.rec.count(), 4, "the queue is now full");
    // The queue never drains: after the bounded retries the operation fails...
    f.run_ticks(6, ms(250));
    let v = f.bus.status(id).unwrap();
    assert_eq!(v.status, OpStatus::Failed);
    assert!(v.failure.as_deref().unwrap().contains("full"));
    // ...and the 6 PDUs still pending are never handed over.
    let n = f.rec.count();
    assert_eq!(n, 4);
    f.run_ticks(40, ms(250));
    assert_eq!(f.rec.count(), n, "nothing is submitted after the failure");
}

#[test]
fn cancel_stops_the_feed() {
    let f = fix_cfg(10, Arc::new(AllowAllPolicy), jit());
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Operation(op(OperationKind::Refresh, Target::All, 250)),
    ));
    let before = f.rec.count();
    assert_eq!(before, 3);
    f.bus.cancel(&ctx(), id).unwrap();
    f.run_ticks(40, ms(250));
    assert_eq!(f.rec.count(), before, "no PDU is submitted after cancel");
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Failed);
}

#[test]
fn an_end_time_drops_the_pending_pdus() {
    let f = fix_cfg(10, Arc::new(AllowAllPolicy), jit());
    let mut d = op(OperationKind::Refresh, Target::All, 250);
    d.run_for = Some(ms(750));
    let id = queued(f.bus.submit(&ctx(), Command::Operation(d)));
    f.bus.on_outcome(emitted(f.rec.pdus()[0].0));
    f.run_ticks(3, ms(250)); // reaches the end time
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::MarkedCompleted);
    let n = f.rec.count();
    f.run_ticks(40, ms(250));
    assert_eq!(f.rec.count(), n, "the rest of the campaign is never sent");
}

#[test]
fn a_lost_outcome_fails_the_operation_instead_of_wedging_it() {
    // QA-3: deadline = release + 60 s, grace 5 s.
    let f = fix(2);
    let id = queued(f.bus.submit(
        &ctx(),
        Command::Terminal {
            op: TerminalOp::Refresh,
            terminal: tid(1),
        },
    ));
    f.clock.advance(ms(64_999));
    f.bus.tick();
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Downloaded);
    f.clock.advance(ms(1));
    f.bus.tick();
    let v = f.bus.status(id).unwrap();
    assert_eq!(v.status, OpStatus::Failed);
    assert!(v.failure.as_deref().unwrap().contains("lost"));
    assert!(f.bus.log().iter().any(|l| l.event == "outcome-lost"));
    // A late real outcome for it changes nothing.
    f.bus.on_outcome(emitted(f.rec.pdus()[0].0));
    assert_eq!(f.bus.status(id).unwrap().status, OpStatus::Failed);
}

#[test]
fn a_failed_command_names_its_operation() {
    let f = fix(3);
    let e = f
        .bus
        .submit(
            &ctx(),
            Command::Terminal {
                op: TerminalOp::General,
                terminal: tid(1),
            },
        )
        .unwrap_err();
    let id = e.operation().expect("the id is carried");
    assert_eq!(
        f.bus.status(id).unwrap().status,
        OpStatus::RequestSentFailed
    );
    assert_eq!(
        e.reason(),
        "encode-failed",
        "reason looks through the wrapper"
    );
    assert!(e.to_string().contains("failed"));
}

#[test]
fn the_default_handler_fails_safe_on_what_it_does_not_know() {
    // Anything routed to the seam that the handler does not recognise is
    // refused by name (the stand-in for a variant #25 has not registered).
    use super::DefaultHandler;
    let r = DefaultHandler.handle(
        &ctx(),
        &Command::Control(ControlCommand::Cancel(OperationId(1))),
    );
    assert!(matches!(
        r,
        Err(CommandError::Unsupported {
            what: "non-pdu-command",
            ..
        })
    ));
    assert_eq!(
        DefaultHandler
            .handle(&ctx(), &Command::Control(ControlCommand::Ping))
            .unwrap(),
        Submitted::Done
    );
}
