//! Black-box tests of the scheduler → packetizer → pacer pipeline through
//! the public API (issue #14 acceptance criteria).

use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use oob_mux::scheduler::{BudgetConfig, ClassBudget, ClassMap};
use oob_mux::{
    CarouselId, Clock, ConfigError, DropReason, ManualClock, MonoTime, Outcome, OutcomeReceiver,
    Pacer, PacerConfig, PacketRate, Payload, Pdu, Pid, PullError, PullSource, Scheduler,
    SchedulerConfig, SchedulerHandle, Submission, SubmitError, TimeStamper, TrafficClass, TsPacket,
    UtcTime,
};

// Test fixture values only; real values are runtime configuration.
const BPS: u64 = 1504 * 1000; // 1000 packets/s keeps slot arithmetic readable
const T0_UTC: u64 = 1_000_000;

/// A permissive budget over the whole fixture stream: the classes behave
/// as in strict class order unless a test says otherwise. Fixture values
/// only; the budget's own behavior is tested in `sim_floors.rs`.
fn budget_fixture(lab_mode: bool) -> BudgetConfig {
    let stream_kbps = u32::try_from(BPS / 1000).unwrap();
    BudgetConfig {
        insertion_kbps: stream_kbps,
        classes: ClassMap::from_fn(|c| ClassBudget {
            floor_kbps: if c == TrafficClass::C0System {
                stream_kbps / 2
            } else {
                0
            },
            cap_kbps: match c {
                TrafficClass::C2Eas | TrafficClass::C4Cdl | TrafficClass::C5Lab => {
                    Some(stream_kbps / 2)
                }
                _ => None,
            },
            burst_packets: 64,
        }),
        lab_mode,
    }
}

fn sched_config() -> SchedulerConfig {
    SchedulerConfig {
        queue_depth: 8,
        max_carousels: 4,
        outcome_capacity: 64,
        budget: budget_fixture(true),
    }
}

fn pacer_config() -> PacerConfig {
    PacerConfig {
        packet_rate: PacketRate::from_bits_per_second(BPS).unwrap(),
        max_pull: 128,
        pull_budget: Duration::from_millis(10),
    }
}

struct Rig {
    pacer: Pacer,
    handle: SchedulerHandle,
    outcomes: OutcomeReceiver,
    clock: Arc<ManualClock>,
}

fn rig_with(sc: SchedulerConfig) -> Rig {
    let clock = Arc::new(ManualClock::new(
        MonoTime::default(),
        UtcTime::from_unix(Duration::from_secs(T0_UTC)),
    ));
    let (scheduler, handle, outcomes) = Scheduler::new(sc).unwrap();
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    let pacer = Pacer::new(pacer_config(), scheduler, dyn_clock).unwrap();
    Rig {
        pacer,
        handle,
        outcomes,
        clock,
    }
}

fn rig() -> Rig {
    rig_with(sched_config())
}

fn pid(v: u16) -> Pid {
    Pid::new(v).unwrap()
}

/// A section of `len` bytes with a valid section_length and `tag` at
/// byte 3, so tests can tell PDUs apart on the wire.
fn section(len: usize, tag: u8) -> Arc<[u8]> {
    let mut s = vec![0x5Au8; len];
    let sl = u16::try_from(len - 3).unwrap();
    s[0] = 0xC0;
    s[1] = 0xB0 | (sl >> 8) as u8;
    s[2] = sl as u8;
    s[3] = tag;
    s.into()
}

fn pdu(class: TrafficClass, priority: u8, len: usize, tag: u8) -> Pdu {
    Pdu {
        class,
        priority,
        pid: pid(0x100 + u16::from(tag)),
        release_at: None,
        deadline: None,
        payload: Payload::Section(section(len, tag)),
        stamp: None,
    }
}

fn pull(p: &mut Pacer, n: usize) -> Vec<TsPacket> {
    let mut out = Vec::with_capacity(n);
    let r = p.pull(n, &mut out).unwrap();
    assert_eq!(out.len(), n, "exactly n");
    assert_eq!(r.served + r.nulls, n);
    assert_eq!(r.nulls, out.iter().filter(|k| k.is_null()).count());
    out
}

/// Tags (byte 3 of the section) of every section start in `pkts`.
fn starts(pkts: &[TsPacket]) -> Vec<u8> {
    pkts.iter()
        .filter(|k| k.pusi())
        .map(|k| k.payload_bytes()[1 + 3])
        .collect()
}

#[test]
fn empty_queues_give_n_nulls() {
    let mut r = rig();
    let out = pull(&mut r.pacer, 17);
    assert!(out.iter().all(TsPacket::is_null));
    let s = r.pacer.stats().snapshot();
    assert_eq!((s.slots, s.nulls_emitted, s.packets_served), (17, 17, 0));
    assert_eq!(r.pacer.slot(), 17);
    assert_eq!(r.pacer.stream_time(), Duration::from_millis(17));
}

#[test]
fn shortfall_is_null_filled_after_the_data() {
    let mut r = rig();
    r.handle
        .submit(Submission::Once(pdu(TrafficClass::C1Addressed, 0, 400, 1)))
        .unwrap();
    let out = pull(&mut r.pacer, 5);
    // 400 bytes = 3 packets, then 2 nulls.
    assert!(
        out[..3]
            .iter()
            .all(|k| !k.is_null() && k.pid_value() == 0x101)
    );
    assert!(out[3..].iter().all(TsPacket::is_null));
    let s = r.pacer.stats().snapshot();
    assert_eq!((s.packets_served, s.nulls_emitted), (3, 2));
    assert_eq!(s.served_by_class[TrafficClass::C1Addressed.index()], 3);
}

#[test]
fn a_pdu_continues_across_pulls() {
    let mut r = rig();
    r.handle
        .submit(Submission::Once(pdu(TrafficClass::C0System, 0, 1000, 7)))
        .unwrap();
    let mut all = Vec::new();
    for n in [2, 1, 4] {
        all.extend(pull(&mut r.pacer, n));
    }
    // 1000 bytes = 6 packets, then a null.
    assert!(all[..6].iter().all(|k| k.pid_value() == 0x107));
    assert!(all[6].is_null());
    assert_eq!(starts(&all), vec![7]);
    let ccs: Vec<u8> = all[..6].iter().map(TsPacket::continuity_counter).collect();
    assert_eq!(ccs, vec![0, 1, 2, 3, 4, 5]);
}

/// xorshift64*: a dependency-free PRNG for the randomized test.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn pull_returns_exactly_n_over_random_queue_states() {
    // QA-8: one-shots, carousels (new, replaced, withdrawn), release times,
    // deadlines and panicking stampers, all mixed at random.
    let mut r = rig();
    let max = r.pacer.config().max_pull;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut total = 0u64;
    let mut out = Vec::with_capacity(max);
    for _ in 0..3000 {
        for _ in 0..rng.below(3) {
            let class = TrafficClass::ALL[rng.below(6) as usize];
            let len = 8 + rng.below(1200) as usize;
            let tag = rng.below(200) as u8;
            let mut p = pdu(class, rng.below(4) as u8, len, tag);
            let now = r.clock.mono();
            if rng.below(4) == 0 {
                p.release_at = Some(now.saturating_add(Duration::from_millis(rng.below(20))));
            }
            match rng.below(10) {
                0 => p.stamp = Some(Arc::new(PanickingStamper)),
                1 => p.stamp = Some(Arc::new(SecondsStamper)),
                _ => {}
            }
            // Refusals (full queue, carousel limit) are legitimate states;
            // they must not disturb pulls.
            match rng.below(5) {
                0 => {
                    let _ = r.handle.submit(Submission::Cyclic {
                        id: CarouselId(rng.below(6) as u32),
                        pdu: p,
                        period: Duration::from_millis(1 + rng.below(50)),
                    });
                }
                1 => {
                    let _ = r.handle.withdraw(CarouselId(rng.below(6) as u32));
                }
                _ => {
                    if rng.below(3) == 0 {
                        p.deadline = Some(now.saturating_add(Duration::from_millis(rng.below(10))));
                    }
                    let _ = r.handle.submit(Submission::Once(p));
                }
            }
        }
        r.clock.advance(Duration::from_millis(rng.below(3)));
        let n = 1 + rng.below(max as u64) as usize;
        out.clear();
        let rep = r.pacer.pull(n, &mut out).unwrap();
        assert_eq!(out.len(), n);
        assert_eq!(rep.served + rep.nulls, n);
        assert_eq!(rep.nulls, out.iter().filter(|k| k.is_null()).count());
        total += n as u64;
    }
    let s = r.pacer.stats().snapshot();
    assert_eq!(s.slots, total);
    assert_eq!(s.packets_served + s.nulls_emitted, s.slots);
    assert_eq!(s.served_by_class.iter().sum::<u64>(), s.packets_served);
    assert_eq!(r.pacer.slot(), total);
    assert!(
        s.stamper_panics > 0 && s.expired > 0,
        "paths exercised: {s:?}"
    );
}

/// Writes the UTC seconds into bytes 4..8 of the section.
struct SecondsStamper;
impl TimeStamper for SecondsStamper {
    fn stamp(&self, s: &mut [u8], at: UtcTime) {
        let secs = u32::try_from(at.since_unix_epoch().as_secs()).unwrap_or(u32::MAX);
        s[4..8].copy_from_slice(&secs.to_be_bytes());
    }
}

#[test]
fn time_bearing_pdus_are_stamped_at_pull_not_at_enqueue() {
    let mut r = rig();
    let mut p = pdu(TrafficClass::C0System, 0, 32, 1);
    p.stamp = Some(Arc::new(SecondsStamper));
    let id = r.handle.submit(Submission::Once(p)).unwrap(); // enqueued at t0
    // Not stamped while it waits: no outcome before a pull.
    assert_eq!(r.outcomes.try_recv(), Err(TryRecvError::Empty));
    r.clock.advance(Duration::from_secs(42)); // pulled at t1 = t0 + 42 s
    let out = pull(&mut r.pacer, 3);
    assert_eq!(starts(&out), vec![1]);
    let ev = r.outcomes.try_recv().unwrap();
    assert_eq!(ev.id, id);
    let t1 = UtcTime::from_unix(Duration::from_secs(T0_UTC + 42));
    assert_eq!(
        ev.outcome,
        Outcome::Emitted {
            first_packet_slot: 0,
            utc: t1
        }
    );
}

#[test]
fn the_stamped_field_holds_the_pull_time() {
    let mut r = rig();
    let mut p = pdu(TrafficClass::C0System, 0, 32, 1);
    p.stamp = Some(Arc::new(SecondsStamper));
    r.handle.submit(Submission::Once(p)).unwrap();
    r.clock.advance(Duration::from_secs(42));
    let out = pull(&mut r.pacer, 1);
    let sec = &out[0].payload_bytes()[1..]; // after pointer_field
    let stamped = u32::from_be_bytes([sec[4], sec[5], sec[6], sec[7]]);
    assert_eq!(u64::from(stamped), T0_UTC + 42);
    // CRC32 (MPEG-2, no reflection) over the whole section checks to zero.
    let crc = sec[..32].iter().fold(0xFFFF_FFFFu32, |mut c, &b| {
        c ^= u32::from(b) << 24;
        for _ in 0..8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ 0x04C1_1DB7
            } else {
                c << 1
            };
        }
        c
    });
    assert_eq!(crc, 0, "CRC recomputed after stamping");
}

#[test]
fn a_missed_deadline_expires_and_nothing_goes_out() {
    let mut r = rig();
    let mut p = pdu(TrafficClass::C1Addressed, 0, 100, 3);
    p.deadline = Some(MonoTime::from_origin(Duration::from_millis(5)));
    let id = r.handle.submit(Submission::Once(p)).unwrap();
    r.clock.advance(Duration::from_millis(6));
    let out = pull(&mut r.pacer, 4);
    assert!(out.iter().all(TsPacket::is_null));
    let ev = r.outcomes.try_recv().unwrap();
    assert_eq!((ev.id, ev.outcome), (id, Outcome::Expired));
    assert_eq!(r.pacer.stats().snapshot().expired, 1);
}

#[test]
fn release_time_holds_a_pdu_back() {
    let mut r = rig();
    let mut p = pdu(TrafficClass::C1Addressed, 0, 10, 4);
    p.release_at = Some(MonoTime::from_origin(Duration::from_millis(250)));
    r.handle.submit(Submission::Once(p)).unwrap();
    assert!(pull(&mut r.pacer, 2).iter().all(TsPacket::is_null));
    r.clock.advance(Duration::from_millis(250));
    assert_eq!(starts(&pull(&mut r.pacer, 2)), vec![4]);
}

#[test]
fn class_order_then_priority_then_arrival() {
    let mut r = rig();
    let h = &r.handle;
    h.submit(Submission::Once(pdu(TrafficClass::C5Lab, 9, 10, 1)))
        .unwrap();
    h.submit(Submission::Once(pdu(TrafficClass::C3Guide, 0, 10, 2)))
        .unwrap();
    h.submit(Submission::Once(pdu(TrafficClass::C3Guide, 5, 10, 3)))
        .unwrap();
    h.submit(Submission::Once(pdu(TrafficClass::C3Guide, 5, 10, 4)))
        .unwrap();
    h.submit(Submission::Once(pdu(TrafficClass::C0System, 0, 10, 5)))
        .unwrap();
    assert_eq!(starts(&pull(&mut r.pacer, 6)), vec![5, 3, 4, 2, 1]);
}

/// Advances the shared clock past the pull budget while stamping.
struct SlowStamper(Arc<ManualClock>);
impl TimeStamper for SlowStamper {
    fn stamp(&self, _: &mut [u8], _: UtcTime) {
        self.0.advance(Duration::from_millis(11));
    }
}

#[test]
fn a_pull_over_budget_is_reported_late() {
    let mut r = rig();
    assert!(!r.pacer.pull(4, &mut Vec::new()).unwrap().late);
    let mut p = pdu(TrafficClass::C0System, 0, 16, 1);
    p.stamp = Some(Arc::new(SlowStamper(r.clock.clone())));
    r.handle.submit(Submission::Once(p)).unwrap();
    let mut out = Vec::new();
    let rep = r.pacer.pull(4, &mut out).unwrap();
    assert!(rep.late);
    assert_eq!(out.len(), 4);
    assert_eq!(r.pacer.stats().snapshot().late_pulls, 1);
}

struct PanickingStamper;
impl TimeStamper for PanickingStamper {
    fn stamp(&self, _: &mut [u8], _: UtcTime) {
        panic!("deliberate test panic");
    }
}

#[test]
fn a_panicking_stamper_drops_its_pdu_and_the_stream_continues() {
    let mut r = rig();
    let mut bad = pdu(TrafficClass::C0System, 1, 16, 1);
    bad.stamp = Some(Arc::new(PanickingStamper));
    let bad_id = r.handle.submit(Submission::Once(bad)).unwrap();
    r.handle
        .submit(Submission::Once(pdu(TrafficClass::C0System, 0, 16, 2)))
        .unwrap();
    let out = pull(&mut r.pacer, 2);
    assert_eq!(starts(&out), vec![2]);
    assert!(out[1].is_null());
    let ev = r.outcomes.try_recv().unwrap();
    assert_eq!(
        (ev.id, ev.outcome),
        (bad_id, Outcome::Dropped(DropReason::StamperPanicked))
    );
    assert_eq!(r.pacer.stats().snapshot().stamper_panics, 1);
}

#[test]
fn carousels_repeat_every_period_in_stream_slots() {
    let mut r = rig();
    // 1000 packets/s: 10 ms = 10 slots.
    r.handle
        .submit(Submission::Cyclic {
            id: CarouselId(1),
            pdu: pdu(TrafficClass::C0System, 0, 10, 9),
            period: Duration::from_millis(10),
        })
        .unwrap();
    let out = pull(&mut r.pacer, 30);
    let at: Vec<usize> = (0..30).filter(|&i| !out[i].is_null()).collect();
    assert_eq!(at, vec![0, 10, 20]);
    // Carousel emissions are not reported per emission.
    assert_eq!(r.outcomes.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn replacing_a_carousel_never_cuts_the_pdu_in_flight() {
    let mut r = rig();
    let cyc = |tag| Submission::Cyclic {
        id: CarouselId(1),
        pdu: pdu(TrafficClass::C0System, 0, 1000, tag),
        period: Duration::from_secs(1),
    };
    r.handle.submit(cyc(1)).unwrap();
    let mut out = pull(&mut r.pacer, 2); // version 1 is mid-flight (6 packets)
    r.handle.submit(cyc(2)).unwrap();
    out.extend(pull(&mut r.pacer, 10));
    // v1's six packets complete, then v2 (new content) starts at once.
    assert_eq!(starts(&out), vec![1, 2]);
    assert!(out[..6].iter().all(|k| k.pid_value() == 0x101));
    assert!(out[6].pusi() && out[6].pid_value() == 0x102);
}

#[test]
fn withdrawing_a_carousel_stops_it() {
    let mut r = rig();
    r.handle
        .submit(Submission::Cyclic {
            id: CarouselId(5),
            pdu: pdu(TrafficClass::C3Guide, 0, 10, 1),
            period: Duration::from_millis(2),
        })
        .unwrap();
    assert_eq!(starts(&pull(&mut r.pacer, 4)), vec![1, 1]);
    r.handle.withdraw(CarouselId(5)).unwrap();
    assert!(pull(&mut r.pacer, 4).iter().all(TsPacket::is_null));
    assert_eq!(
        r.handle.withdraw(CarouselId(5)),
        Err(SubmitError::UnknownCarousel(CarouselId(5)))
    );
}

#[test]
fn submission_errors() {
    let r = rig_with(SchedulerConfig {
        queue_depth: 1,
        max_carousels: 1,
        outcome_capacity: 1,
        budget: budget_fixture(false),
    });
    let h = &r.handle;
    let ok = || pdu(TrafficClass::C3Guide, 0, 10, 1);
    h.submit(Submission::Once(ok())).unwrap();
    assert_eq!(
        h.submit(Submission::Once(ok())),
        Err(SubmitError::QueueFull {
            class: TrafficClass::C3Guide,
            depth: 1
        })
    );
    assert_eq!(
        h.submit(Submission::Once(pdu(TrafficClass::C5Lab, 0, 10, 1))),
        Err(SubmitError::ClassDisabled(TrafficClass::C5Lab))
    );

    let mut big = ok();
    big.payload = Payload::Section(vec![0u8; 4097].into());
    assert_eq!(
        h.submit(Submission::Once(big)),
        Err(SubmitError::PayloadTooLarge { len: 4097 })
    );
    let mut bad_len = ok();
    bad_len.payload = Payload::Section(vec![0x00, 0xB0, 0x20, 0x00].into());
    assert_eq!(
        h.submit(Submission::Once(bad_len)),
        Err(SubmitError::MalformedSection { len: 4 })
    );
    let mut short_stamped = ok();
    short_stamped.payload = Payload::Section(section(5, 0));
    short_stamped.stamp = Some(Arc::new(SecondsStamper));
    assert_eq!(
        h.submit(Submission::Once(short_stamped)),
        Err(SubmitError::MalformedSection { len: 5 })
    );

    let mut with_deadline = ok();
    with_deadline.deadline = Some(MonoTime::default());
    let cyc = |id, pdu, period| Submission::Cyclic {
        id: CarouselId(id),
        pdu,
        period,
    };
    assert_eq!(
        h.submit(cyc(1, with_deadline, Duration::from_secs(1))),
        Err(SubmitError::DeadlineOnCyclic)
    );
    assert_eq!(
        h.submit(cyc(1, ok(), Duration::ZERO)),
        Err(SubmitError::ZeroPeriod)
    );
    h.submit(cyc(1, ok(), Duration::from_secs(1))).unwrap();
    h.submit(cyc(1, ok(), Duration::from_secs(2))).unwrap(); // replacement fits
    assert_eq!(
        h.submit(cyc(2, ok(), Duration::from_secs(1))),
        Err(SubmitError::CarouselLimit { max: 1 })
    );

    let handle = r.handle.clone();
    drop(r);
    assert_eq!(
        handle.submit(Submission::Once(ok())),
        Err(SubmitError::Closed)
    );
    assert_eq!(handle.withdraw(CarouselId(1)), Err(SubmitError::Closed));
}

#[test]
fn oversized_pulls_are_refused_without_touching_out() {
    // QA-1: a garbled n from the seam must not panic or allocate.
    let mut r = rig();
    let max = r.pacer.config().max_pull;
    let mut out = Vec::new();
    for n in [max + 1, usize::MAX] {
        assert_eq!(
            r.pacer.pull(n, &mut out),
            Err(PullError::Oversized { n, max })
        );
    }
    assert!(out.is_empty() && out.capacity() == 0);
    assert_eq!(r.pacer.slot(), 0, "the stream did not advance");
    let s = r.pacer.stats().snapshot();
    assert_eq!((s.oversized_pulls, s.slots), (2, 0));
    pull(&mut r.pacer, max); // exactly max_pull is fine
}

#[test]
fn zero_config_values_are_refused() {
    // QA-7.
    let sc = |queue_depth, outcome_capacity| SchedulerConfig {
        queue_depth,
        outcome_capacity,
        ..sched_config()
    };
    assert_eq!(
        Scheduler::new(sc(0, 1)).err(),
        Some(ConfigError::ZeroQueueDepth)
    );
    assert_eq!(
        Scheduler::new(sc(1, 0)).err(),
        Some(ConfigError::ZeroOutcomeCapacity)
    );
    let zero_pull = PacerConfig {
        max_pull: 0,
        ..pacer_config()
    };
    let zero_budget = PacerConfig {
        pull_budget: Duration::ZERO,
        ..pacer_config()
    };
    let new_pacer = |c| {
        let (s, _, _) = Scheduler::new(sched_config()).unwrap();
        Pacer::new(c, s, Arc::new(ManualClock::default())).err()
    };
    assert_eq!(new_pacer(zero_pull), Some(ConfigError::ZeroMaxPull));
    assert_eq!(new_pacer(zero_budget), Some(ConfigError::ZeroPullBudget));
    let mut r = rig();
    assert_eq!(
        r.pacer.apply_config(zero_pull),
        Err(ConfigError::ZeroMaxPull)
    );
    assert_eq!(*r.pacer.config(), pacer_config(), "old settings kept");
}

/// A panic payload whose Drop panics again.
struct PanicOnDrop;
impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        panic!("payload drop");
    }
}

struct PanicAnyStamper;
impl TimeStamper for PanicAnyStamper {
    fn stamp(&self, _: &mut [u8], _: UtcTime) {
        std::panic::panic_any(PanicOnDrop);
    }
}

#[test]
fn a_panic_payload_that_panics_on_drop_is_contained() {
    // QA-3.
    let mut r = rig();
    let mut bad = pdu(TrafficClass::C0System, 0, 16, 1);
    bad.stamp = Some(Arc::new(PanicAnyStamper));
    r.handle.submit(Submission::Once(bad)).unwrap();
    let out = pull(&mut r.pacer, 2);
    assert!(out.iter().all(TsPacket::is_null));
    assert_eq!(r.pacer.stats().snapshot().stamper_panics, 1);
}

#[test]
fn expired_pdus_in_lower_classes_are_reaped_while_higher_ones_have_work() {
    // QA-4.
    let mut r = rig_with(SchedulerConfig {
        queue_depth: 4,
        ..sched_config()
    });
    r.handle
        .submit(Submission::Cyclic {
            id: CarouselId(1),
            // C1: a 1 ns C0 carousel would exceed any C0 floor (#16).
            pdu: pdu(TrafficClass::C1Addressed, 0, 10, 1),
            period: Duration::from_nanos(1), // always due
        })
        .unwrap();
    for tag in 10..14 {
        let mut p = pdu(TrafficClass::C5Lab, 0, 10, tag);
        p.deadline = Some(MonoTime::from_origin(Duration::from_millis(5)));
        r.handle.submit(Submission::Once(p)).unwrap();
    }
    r.clock.advance(Duration::from_millis(10));
    assert!(pull(&mut r.pacer, 32).iter().all(|k| !k.is_null()));
    assert_eq!(r.pacer.stats().snapshot().expired, 4);
    let expired = std::iter::from_fn(|| r.outcomes.try_recv().ok())
        .filter(|e| e.outcome == Outcome::Expired)
        .count();
    assert_eq!(expired, 4);
    r.handle
        .submit(Submission::Once(pdu(TrafficClass::C5Lab, 0, 10, 20)))
        .unwrap();
}

#[test]
fn a_pdu_is_still_eligible_at_exactly_its_deadline() {
    // QA-8: "by this instant".
    let mut r = rig();
    let mut p = pdu(TrafficClass::C1Addressed, 0, 10, 3);
    p.deadline = Some(MonoTime::from_origin(Duration::from_millis(5)));
    r.handle.submit(Submission::Once(p)).unwrap();
    r.clock.advance(Duration::from_millis(5));
    assert_eq!(starts(&pull(&mut r.pacer, 1)), vec![3]);
    assert_eq!(r.pacer.stats().snapshot().expired, 0);
}

#[test]
fn equal_carousels_that_are_always_due_alternate() {
    // QA-8: round-robin among equal-priority carousels.
    let mut r = rig();
    for (id, tag) in [(1, 1), (2, 2)] {
        r.handle
            .submit(Submission::Cyclic {
                id: CarouselId(id),
                pdu: pdu(TrafficClass::C3Guide, 0, 10, tag),
                period: Duration::from_nanos(1),
            })
            .unwrap();
    }
    assert_eq!(starts(&pull(&mut r.pacer, 6)), vec![1, 2, 1, 2, 1, 2]);
}

#[test]
fn a_higher_priority_one_shot_beats_a_due_carousel_in_its_class() {
    // QA-8.
    let mut r = rig();
    r.handle
        .submit(Submission::Cyclic {
            id: CarouselId(1),
            pdu: pdu(TrafficClass::C0System, 1, 10, 1),
            period: Duration::from_secs(1),
        })
        .unwrap();
    r.handle
        .submit(Submission::Once(pdu(TrafficClass::C0System, 2, 10, 2)))
        .unwrap();
    r.handle
        .submit(Submission::Once(pdu(TrafficClass::C0System, 0, 10, 3)))
        .unwrap();
    assert_eq!(starts(&pull(&mut r.pacer, 3)), vec![2, 1, 3]);
}

#[test]
fn re_submitting_an_unchanged_carousel_keeps_its_rate() {
    // QA-5: 100 ms period, re-submitted unchanged every 10-slot pull for
    // 1 s of stream: still 10 emissions, not 100.
    let mut r = rig();
    let cyc = || Submission::Cyclic {
        id: CarouselId(1),
        pdu: pdu(TrafficClass::C0System, 0, 10, 1),
        period: Duration::from_millis(100),
    };
    let mut emitted = 0;
    for _ in 0..100 {
        r.handle.submit(cyc()).unwrap();
        emitted += starts(&pull(&mut r.pacer, 10)).len();
    }
    assert_eq!(emitted, 10);
}

/// Pulls 2 slots after `first` is emitted at slot 0, replaces carousel 1
/// with `second`, and returns the slots of section starts in the next 50.
fn replacement_emissions(first: Submission, second: Submission) -> Vec<usize> {
    let mut r = rig();
    r.handle.submit(first).unwrap();
    assert_eq!(starts(&pull(&mut r.pacer, 2)).len(), 1);
    r.handle.submit(second).unwrap();
    let out = pull(&mut r.pacer, 50);
    (0..out.len()).filter(|&i| out[i].pusi()).collect()
}

#[test]
fn a_new_section_version_on_the_same_pid_is_due_at_once() {
    // QA-10: the #17 new-PSI-version case: only the section bytes change.
    let cyc = |tag| {
        let mut p = pdu(TrafficClass::C0System, 0, 10, 1);
        p.payload = Payload::Section(section(10, tag));
        Submission::Cyclic {
            id: CarouselId(1),
            pdu: p,
            period: Duration::from_secs(60),
        }
    };
    assert_eq!(replacement_emissions(cyc(1), cyc(2)), vec![0]);
    assert_eq!(replacement_emissions(cyc(1), cyc(1)), Vec::<usize>::new());
}

#[test]
fn a_new_pid_alone_is_due_at_once() {
    // QA-10: only the PID changes.
    let cyc = |pid_value| {
        let mut p = pdu(TrafficClass::C0System, 0, 10, 1);
        p.pid = pid(pid_value);
        Submission::Cyclic {
            id: CarouselId(1),
            pdu: p,
            period: Duration::from_secs(60),
        }
    };
    assert_eq!(replacement_emissions(cyc(0x30), cyc(0x31)), vec![0]);
}

#[test]
fn a_period_change_alone_is_due_at_once() {
    // QA-9: 60 s → 10 ms must not wait out the old 60 s schedule.
    let cyc = |period| Submission::Cyclic {
        id: CarouselId(1),
        pdu: pdu(TrafficClass::C0System, 0, 10, 1),
        period,
    };
    assert_eq!(
        replacement_emissions(cyc(Duration::from_secs(60)), cyc(Duration::from_millis(10))),
        vec![0, 10, 20, 30, 40]
    );
}

#[test]
fn config_validate_is_public_and_matches_the_constructors() {
    // ARCH-2: oob-dac's save path validates without building anything.
    assert_eq!(sched_config().validate(), Ok(()));
    assert_eq!(pacer_config().validate(), Ok(()));
    let bad_sched = SchedulerConfig {
        outcome_capacity: 0,
        ..sched_config()
    };
    let bad_pacer = PacerConfig {
        pull_budget: Duration::ZERO,
        ..pacer_config()
    };
    assert_eq!(bad_sched.validate(), Err(ConfigError::ZeroOutcomeCapacity));
    assert_eq!(bad_pacer.validate(), Err(ConfigError::ZeroPullBudget));
    assert_eq!(
        Scheduler::new(bad_sched).err(),
        Some(ConfigError::ZeroOutcomeCapacity)
    );
}

#[test]
fn a_full_outcome_channel_never_blocks_the_pull() {
    let mut r = rig_with(SchedulerConfig {
        outcome_capacity: 1,
        ..sched_config()
    });
    for tag in 1..=3 {
        r.handle
            .submit(Submission::Once(pdu(TrafficClass::C1Addressed, 0, 10, tag)))
            .unwrap();
    }
    assert_eq!(starts(&pull(&mut r.pacer, 3)), vec![1, 2, 3]);
    assert_eq!(r.pacer.stats().snapshot().outcomes_lost, 2);
    assert!(r.outcomes.try_recv().is_ok());
}

#[test]
fn the_rate_is_a_runtime_parameter() {
    let mut r = rig();
    pull(&mut r.pacer, 100);
    assert_eq!(r.pacer.stream_time(), Duration::from_millis(100));
    let mut c = *r.pacer.config();
    c.packet_rate = PacketRate::from_bits_per_second(BPS * 2).unwrap();
    r.pacer.apply_config(c).unwrap();
    // QA-2: time accrued so far is kept, later slots accrue at the new rate.
    assert_eq!(r.pacer.stream_time(), Duration::from_millis(100));
    pull(&mut r.pacer, 100);
    assert_eq!(r.pacer.stream_time(), Duration::from_millis(150));
    assert_eq!(r.pacer.slot(), 200);
}
