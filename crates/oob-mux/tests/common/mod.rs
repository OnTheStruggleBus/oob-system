//! Shared fixtures for the class-budget tests and bench (#16). Fixture
//! values only: real budgets are runtime configuration (ADR-003 Layer 1).
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use oob_mux::scheduler::{BudgetConfig, ClassBudget, ClassMap};
use oob_mux::{
    Clock, ManualClock, MonoTime, Pacer, PacerConfig, PacketRate, Payload, Pdu, Pid, PullSource,
    Scheduler, SchedulerConfig, SchedulerHandle, Submission, SubmitError, TrafficClass, TsPacket,
    UtcTime,
};

/// The stream the fixtures run on: 2.048 Mb/s (ADR-001's OOB rate).
pub const STREAM_BPS: u64 = 2_048_000;
pub const QUEUE_DEPTH: usize = 24;

pub fn rate() -> PacketRate {
    PacketRate::from_bits_per_second(STREAM_BPS).unwrap()
}

/// `[floor, cap]` per class in kbps, C0 first; `None` is uncapped.
pub type Row = (u32, Option<u32>);

pub fn budget(insertion_kbps: u32, rows: [Row; 6], burst: u16, lab_mode: bool) -> BudgetConfig {
    BudgetConfig {
        insertion_kbps,
        classes: ClassMap::from_fn(|c| ClassBudget {
            floor_kbps: rows[c.index()].0,
            cap_kbps: rows[c.index()].1,
            burst_packets: burst,
        }),
        lab_mode,
    }
}

/// Roomy: every floor can hold while C2 sits at its cap.
pub fn roomy(lab_mode: bool) -> BudgetConfig {
    budget(
        1000,
        [
            (200, Some(250)),
            (100, Some(150)),
            (0, Some(300)),
            (250, Some(300)),
            (0, Some(200)),
            (0, Some(100)),
        ],
        8,
        lab_mode,
    )
}

/// Tight: C0 + C1 + C2's cap leave C3 below its floor while an alert
/// runs, by at most C2's cap.
pub fn tight(lab_mode: bool) -> BudgetConfig {
    budget(
        1000,
        [
            (400, Some(400)),
            (100, Some(100)),
            (0, Some(300)),
            (250, Some(250)),
            (0, Some(200)),
            (0, Some(100)),
        ],
        8,
        lab_mode,
    )
}

pub struct Rig {
    pub pacer: Pacer,
    pub handle: SchedulerHandle,
    pub outcomes: oob_mux::OutcomeReceiver,
    pub clock: Arc<ManualClock>,
}

pub fn rig(budget: BudgetConfig) -> Rig {
    rig_depth(budget, QUEUE_DEPTH)
}

pub fn rig_depth(budget: BudgetConfig, queue_depth: usize) -> Rig {
    let clock = Arc::new(ManualClock::new(
        MonoTime::default(),
        UtcTime::from_unix(Duration::from_secs(1_000_000)),
    ));
    let (scheduler, handle, outcomes) = Scheduler::new(SchedulerConfig {
        queue_depth,
        max_carousels: 8,
        outcome_capacity: 4096,
        budget,
    })
    .unwrap();
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    let pacer = Pacer::new(
        PacerConfig {
            packet_rate: rate(),
            max_pull: 64,
            pull_budget: Duration::from_secs(1),
        },
        scheduler,
        dyn_clock,
    )
    .unwrap();
    Rig {
        pacer,
        handle,
        outcomes,
        clock,
    }
}

pub fn pid_of(class: TrafficClass) -> u16 {
    0x100 + class.index() as u16
}

/// A one-packet section on the class's own PID.
pub fn one_packet(class: TrafficClass, priority: u8, tag: u8) -> Pdu {
    let len = 40usize;
    let mut s = vec![0x5Au8; len];
    let sl = u16::try_from(len - 3).unwrap();
    s[0] = 0xC0;
    s[1] = 0xB0 | (sl >> 8) as u8;
    s[2] = sl as u8;
    s[3] = tag;
    Pdu {
        class,
        priority,
        pid: Pid::new(pid_of(class)).unwrap(),
        release_at: None,
        deadline: None,
        payload: Payload::Section(s.into()),
        stamp: None,
    }
}

/// Tops a class's queue up until it is full: a saturated producer.
pub fn saturate(handle: &SchedulerHandle, class: TrafficClass) {
    loop {
        match handle.submit(Submission::Once(one_packet(class, 0, 0))) {
            Ok(_) => {}
            Err(SubmitError::QueueFull { .. } | SubmitError::ClassDisabled(_)) => break,
            Err(e) => panic!("{e}"),
        }
    }
}

pub fn pull(p: &mut Pacer, n: usize) -> Vec<TsPacket> {
    let mut out = Vec::with_capacity(n);
    p.pull(n, &mut out).unwrap();
    out
}
