//! Deterministic class-budget simulation (issue #16, F6-3): a virtual slot
//! clock (the pacer counts slots, not host time), every class saturated,
//! and assertions about what each class was delivered over sliding
//! windows. Box-free: this is class-budget evidence, not a wire claim.

mod common;

use std::time::Duration;

use common::*;
use oob_mux::scheduler::{BudgetConfig, BudgetViolation, validate};
use oob_mux::{
    CarouselId, DropReason, MonoTime, Outcome, Payload, Pdu, Submission, SubmitError,
    TrafficClass::{self, C0System, C1Addressed, C2Eas, C3Guide, C4Cdl, C5Lab},
};

const STEP: usize = 16;

/// Cumulative non-null packets per class after each `STEP`-slot pull, with
/// every class in `saturated` kept full by its producer.
fn simulate(cfg: BudgetConfig, saturated: &[TrafficClass], slots: usize) -> Vec<[u64; 6]> {
    let mut r = rig(cfg);
    let stats = r.pacer.stats();
    let mut cumulative = vec![[0u64; 6]];
    for _ in 0..slots / STEP {
        for &c in saturated {
            saturate(&r.handle, c);
        }
        pull(&mut r.pacer, STEP);
        cumulative.push(stats.snapshot().served_by_class);
    }
    cumulative
}

/// Expected packets of a `kbps` rate over `slots` slots.
fn expect(kbps: u32, slots: usize) -> f64 {
    f64::from(kbps) * 1000.0 * slots as f64 / STREAM_BPS as f64
}

/// The packets class `c` got in the window starting at pull `i`.
fn window(cum: &[[u64; 6]], c: TrafficClass, i: usize, steps: usize) -> f64 {
    (cum[i + steps][c.index()] - cum[i][c.index()]) as f64
}

/// Every sliding window of `slots` slots: the class got at least
/// `floor_kbps` and at most `cap_kbps`, each within one burst and one
/// packet of rounding.
fn assert_band(
    cum: &[[u64; 6]],
    c: TrafficClass,
    floor_kbps: u32,
    cap_kbps: Option<u32>,
    burst: u16,
    slots: usize,
) {
    let steps = slots / STEP;
    let tol = f64::from(burst) + 2.0;
    for i in (0..cum.len() - steps).step_by(7) {
        let got = window(cum, c, i, steps);
        assert!(
            got >= expect(floor_kbps, slots) - tol,
            "{c:?} below floor in window {i}: {got} < {}",
            expect(floor_kbps, slots)
        );
        if let Some(cap) = cap_kbps {
            assert!(
                got <= expect(cap, slots) + tol,
                "{c:?} above cap in window {i}: {got} > {}",
                expect(cap, slots)
            );
        }
    }
}

#[test]
fn saturated_classes_stay_inside_their_floor_and_cap_bands() {
    let cfg = roomy(true);
    let cum = simulate(cfg.clone(), &TrafficClass::ALL, 100_000);
    for (c, cb) in cfg.classes.iter() {
        assert_band(&cum, c, cb.floor_kbps, cb.cap_kbps, cb.burst_packets, 5_000);
    }
    // The budget itself is never exceeded either.
    let total = |i: usize| cum[i].iter().sum::<u64>() as f64;
    let steps = 5_000 / STEP;
    for i in (0..cum.len() - steps).step_by(11) {
        let got = total(i + steps) - total(i);
        assert!(got <= expect(cfg.insertion_kbps, 5_000) + 10.0, "budget");
    }
}

#[test]
fn c2_preempts_c3_to_c5_and_runs_at_its_cap() {
    let cfg = roomy(true);
    let cum = simulate(cfg, &TrafficClass::ALL, 60_000);
    // C2 has no floor, yet while saturated it is served ahead of the C3
    // floor up to its cap.
    let got = window(&cum, C2Eas, 0, cum.len() - 1);
    assert!(got >= expect(300, 60_000) - 10.0, "C2 held at its cap");
}

#[test]
fn c2_is_served_before_a_c3_pdu_that_holds_floor_tokens() {
    let mut r = rig(roomy(true));
    r.handle
        .submit(Submission::Once(one_packet(C3Guide, 0, 1)))
        .unwrap();
    r.handle
        .submit(Submission::Once(one_packet(C2Eas, 0, 2)))
        .unwrap();
    let out = pull(&mut r.pacer, 2);
    assert_eq!(out[0].pid_value(), pid_of(C2Eas));
    assert_eq!(out[1].pid_value(), pid_of(C3Guide));
}

#[test]
fn an_alert_takes_the_guide_floor_but_never_more_than_its_cap_and_never_c0_or_c1() {
    // Marshal's ruling on the #16 design note (2026-11-23): C2 may dip C3
    // below its floor, bounded by C2's cap; C0 and C1 floors always hold.
    let cfg = tight(true);
    let slots = 5_000;
    let all = simulate(cfg.clone(), &TrafficClass::ALL, 100_000);
    let c0 = cfg.classes[C0System];
    let c1 = cfg.classes[C1Addressed];
    assert_band(&all, C0System, c0.floor_kbps, c0.cap_kbps, 8, slots);
    assert_band(&all, C1Addressed, c1.floor_kbps, c1.cap_kbps, 8, slots);
    assert_band(&all, C2Eas, 0, Some(300), 8, slots);
    // C3 gets the rest of the budget: below its floor, but only by what C2 took (<= its cap).
    let left = 1000 - 400 - 100 - 300;
    let steps = slots / STEP;
    // From a warm-up on: at the start every class's bucket is full, and the
    // other classes' initial bursts come out of what C3 is left.
    for i in (40..all.len() - steps).step_by(13) {
        let got = window(&all, C3Guide, i, steps);
        assert!(got < expect(250, slots) - 20.0, "C3 does dip while C2 runs");
        assert!(
            got >= expect(left, slots) - 10.0,
            "C3 gets what is left: {got} vs {} (window {i})",
            expect(left, slots)
        );
    }
    // Without an alert, the same config serves C3 its full floor.
    let calm = simulate(
        cfg,
        &[C0System, C1Addressed, C3Guide, C4Cdl, C5Lab],
        100_000,
    );
    assert_band(&calm, C3Guide, 250, Some(250), 8, slots);
    assert_eq!(calm.last().unwrap()[C2Eas.index()], 0);
}

#[test]
fn a_cap_limited_class_alone_runs_at_its_cap_not_below_it() {
    // The idle hint must wake the scheduler when a bucket refills.
    let cum = simulate(roomy(true), &[C4Cdl], 60_000);
    assert_band(&cum, C4Cdl, 200, Some(200), 8, 5_000);
}

#[test]
fn c5_emits_nothing_with_lab_mode_off() {
    let cum = simulate(roomy(false), &TrafficClass::ALL, 100_000);
    assert_eq!(cum.last().unwrap()[C5Lab.index()], 0);
    let r = rig(roomy(false));
    assert_eq!(
        r.handle.submit(Submission::Once(one_packet(C5Lab, 0, 1))),
        Err(SubmitError::ClassDisabled(C5Lab))
    );
    // The other classes are untouched by the switch.
    assert!(cum.last().unwrap()[C3Guide.index()] > 0);
}

#[test]
fn switching_lab_mode_off_drops_queued_c5_work_and_refuses_more() {
    let mut r = rig(roomy(true));
    for tag in 0..3 {
        r.handle
            .submit(Submission::Once(one_packet(C5Lab, 0, tag)))
            .unwrap();
    }
    assert_eq!(r.handle.queue_depths()[C5Lab.index()], 3);
    r.handle.apply_budget(roomy(false)).unwrap();
    assert_eq!(r.handle.queue_depths()[C5Lab.index()], 0);
    let out = pull(&mut r.pacer, 8);
    assert!(out.iter().all(oob_mux::TsPacket::is_null));
    let dropped = std::iter::from_fn(|| r.outcomes.try_recv().ok())
        .filter(|e| e.outcome == Outcome::Dropped(DropReason::LabModeOff))
        .count();
    assert_eq!(dropped, 3);
    let s = r.pacer.stats().snapshot();
    assert_eq!(s.dropped_by_class[C5Lab.index()], 3);
    assert_eq!(
        r.handle.submit(Submission::Once(one_packet(C5Lab, 0, 9))),
        Err(SubmitError::ClassDisabled(C5Lab))
    );
    // And back on: C5 is accepted and served again.
    r.handle.apply_budget(roomy(true)).unwrap();
    r.handle
        .submit(Submission::Once(one_packet(C5Lab, 0, 9)))
        .unwrap();
    assert_eq!(pull(&mut r.pacer, 1)[0].pid_value(), pid_of(C5Lab));
}

#[test]
fn c1_is_never_released_before_its_release_time() {
    let mut r = rig(roomy(true));
    let ms = |n| MonoTime::from_origin(Duration::from_millis(n));
    // Submitted out of release order, with a priority-0 PDU releasing
    // first and a priority-1 one later.
    let plan = [(1u8, 0u8, 30u64), (2, 0, 10), (3, 1, 20), (4, 0, 50)];
    for (tag, prio, at) in plan {
        let mut p = one_packet(C1Addressed, prio, tag);
        p.release_at = Some(ms(at));
        r.handle.submit(Submission::Once(p)).unwrap();
    }
    // One slot per pull, 1 ms per slot: record when each tag first appears.
    let mut seen: Vec<(u8, u64)> = Vec::new();
    for slot in 0..80u64 {
        let out = pull(&mut r.pacer, 1);
        if !out[0].is_null() {
            seen.push((out[0].payload_bytes()[1 + 3], slot));
        }
        r.clock.advance(Duration::from_millis(1));
    }
    assert_eq!(seen.len(), 4);
    for (tag, prio, at) in plan {
        let (_, slot) = seen.iter().find(|(t, _)| *t == tag).unwrap();
        assert!(
            *slot >= at,
            "tag {tag} (prio {prio}) left at {slot} before {at}"
        );
    }
    // Released ones go by (priority, release_at, seq): 10 ms first, then
    // at 30 ms tag 1 (prio 0) is out before... the prio-1 one (20 ms) was
    // already served at 20 ms.
    let order: Vec<u8> = seen.iter().map(|(t, _)| *t).collect();
    assert_eq!(order, vec![2, 3, 1, 4]);
}

#[test]
fn the_scheduler_infers_no_campaign_spacing() {
    // Five addressed PDUs submitted together leave in consecutive slots in
    // submission order: any per-terminal 250 ms spacing is the command
    // layer's job through release times (F6-11), never the shaper's.
    let mut r = rig(roomy(true));
    for tag in 1..=5 {
        r.handle
            .submit(Submission::Once(one_packet(C1Addressed, 0, tag)))
            .unwrap();
    }
    let out = pull(&mut r.pacer, 8);
    let tags: Vec<u8> = out
        .iter()
        .filter(|p| !p.is_null())
        .map(|p| p.payload_bytes()[1 + 3])
        .collect();
    assert_eq!(tags, vec![1, 2, 3, 4, 5]);
    assert!(out[..5].iter().all(|p| !p.is_null()), "back to back");
    let slots: Vec<u64> = std::iter::from_fn(|| r.outcomes.try_recv().ok())
        .filter_map(|e| match e.outcome {
            Outcome::Emitted {
                first_packet_slot, ..
            } => Some(first_packet_slot),
            _ => None,
        })
        .collect();
    assert_eq!(slots, vec![0, 1, 2, 3, 4]);
}

#[test]
fn counters_report_bytes_depth_and_expiry_per_class() {
    let mut r = rig(roomy(true));
    for tag in 0..4 {
        r.handle
            .submit(Submission::Once(one_packet(C3Guide, 0, tag)))
            .unwrap();
    }
    let mut late = one_packet(C4Cdl, 0, 9);
    late.deadline = Some(MonoTime::from_origin(Duration::from_millis(1)));
    r.handle.submit(Submission::Once(late)).unwrap();
    assert_eq!(r.handle.queue_depths()[C3Guide.index()], 4);
    r.clock.advance(Duration::from_millis(5));
    pull(&mut r.pacer, 10);
    let s = r.pacer.stats().snapshot();
    assert_eq!(s.served_by_class[C3Guide.index()], 4);
    assert_eq!(s.served_bytes_by_class[C3Guide.index()], 4 * 188);
    assert_eq!(s.expired_by_class[C4Cdl.index()], 1);
    assert_eq!(r.handle.queue_depths(), [0; 6]);
}

fn c0_carousel(len: usize, tag: u8) -> Pdu {
    let mut p = one_packet(C0System, 0, tag);
    let mut s = vec![0x5Au8; len];
    let sl = u16::try_from(len - 3).unwrap();
    s[0] = 0xC0;
    s[1] = 0xB0 | (sl >> 8) as u8;
    s[2] = sl as u8;
    s[3] = tag;
    p.payload = Payload::Section(s.into());
    p
}

#[test]
fn registering_c0_demand_above_the_floor_is_refused_and_so_is_lowering_the_floor() {
    let r = rig(roomy(true)); // C0 floor 200 kbps
    // One packet per 4 ms = 188 * 8 / 0.004 = 376 kbps: above the floor.
    let too_fast = Submission::Cyclic {
        id: CarouselId(1),
        pdu: c0_carousel(40, 1),
        period: Duration::from_millis(4),
    };
    assert_eq!(
        r.handle.submit(too_fast),
        Err(SubmitError::C0DemandExceedsFloor {
            floor_kbps: 200,
            demand_kbps: 376
        })
    );
    // One packet per 10 ms = 150 kbps: fits.
    let ok = |period_ms| Submission::Cyclic {
        id: CarouselId(1),
        pdu: c0_carousel(40, 1),
        period: Duration::from_millis(period_ms),
    };
    r.handle.submit(ok(10)).unwrap();
    // Replacing it with a faster period is refused too, and leaves the old one.
    assert!(matches!(
        r.handle.submit(ok(4)),
        Err(SubmitError::C0DemandExceedsFloor { .. })
    ));
    // The reverse door: a floor under the demand is refused with its reason.
    let mut lower = roomy(true);
    lower.classes[C0System].floor_kbps = 100;
    lower.classes[C0System].cap_kbps = Some(250);
    let errs = r.handle.apply_budget(lower).unwrap_err();
    assert_eq!(
        errs,
        vec![BudgetViolation::C0FloorBelowDemand {
            floor_kbps: 100,
            demand_kbps: 150
        }]
    );
    assert_eq!(r.handle.budget(), roomy(true), "the old budget stays");
}

#[test]
fn apply_budget_refuses_what_validate_refuses() {
    let r = rig(roomy(true));
    let mut starve = roomy(true);
    starve.classes[C0System].floor_kbps = 0;
    let errs = r.handle.apply_budget(starve).unwrap_err();
    assert_eq!(errs, vec![BudgetViolation::C0FloorZero]);
    assert_eq!(errs[0].reason(), "c0_floor_zero");
    // Above the stream: refused once the pacer has bound the rate.
    let mut big = roomy(true);
    big.insertion_kbps = 5000;
    let errs = r.handle.apply_budget(big).unwrap_err();
    assert!(
        errs.iter().any(|e| e.reason() == "budget_exceeds_stream"),
        "{errs:?}"
    );
}

#[test]
fn a_budget_swap_grants_no_burst() {
    let mut r = rig(roomy(true));
    // Drain C4's cap bucket, then swap in the same budget: no refill.
    for _ in 0..20 {
        saturate(&r.handle, C4Cdl);
        pull(&mut r.pacer, 64);
    }
    let before = r.pacer.stats().snapshot().served_by_class[C4Cdl.index()];
    r.handle.apply_budget(roomy(true)).unwrap();
    saturate(&r.handle, C4Cdl);
    let out = pull(&mut r.pacer, 64);
    let c4 = out
        .iter()
        .filter(|p| p.pid_value() == pid_of(C4Cdl))
        .count();
    // 64 slots at 200 kbps is ~6 packets; a refilled burst would add 8.
    assert!(c4 <= 8, "{c4}");
    assert!(before > 0);
}

#[test]
fn changed_carousel_replacements_cannot_outrun_the_period() {
    // #14 amendment 4: a producer that keeps changing a C0 carousel emits
    // at most one extra, schedule-jumping copy per period.
    let mut r = rig(roomy(true));
    let period = Duration::from_millis(50); // ~68 slots
    let period_slots = rate().slots_for(period) as usize;
    let mut emitted = 0usize;
    let total = 2000usize;
    for slot in 0..total {
        // Change the section every slot.
        r.handle
            .submit(Submission::Cyclic {
                id: CarouselId(1),
                pdu: c0_carousel(40, (slot % 250) as u8),
                period,
            })
            .unwrap();
        emitted += pull(&mut r.pacer, 1)
            .iter()
            .filter(|p| !p.is_null())
            .count();
    }
    let bound = total / period_slots + 2;
    assert!(
        emitted <= 2 * bound,
        "{emitted} emissions, bound {}",
        2 * bound
    );
    assert!(emitted >= total / period_slots, "{emitted}");
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
    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + u32::try_from(self.next() % u64::from(hi - lo + 1)).unwrap()
    }
}

#[test]
fn every_config_validate_accepts_keeps_c0_at_its_floor_under_saturation() {
    // Validator soundness: accepted configs pass the simulation's C0
    // assertion, whatever the other classes do.
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let stream = oob_mux::scheduler::StreamParams {
        packet_rate: rate(),
    };
    let (mut accepted, mut rejected) = (0, 0);
    for _ in 0..400 {
        let insertion = rng.range(300, 1500);
        let mut rows: [Row; 6] = [(0, None); 6];
        for (i, row) in rows.iter_mut().enumerate() {
            let floor = rng.range(0, insertion / 4);
            let cap = if rng.range(0, 3) == 0 && i != 2 && i != 4 && i != 5 {
                None
            } else {
                Some(rng.range(0, insertion))
            };
            *row = (
                if i == 0 {
                    rng.range(1, insertion / 2)
                } else {
                    floor
                },
                cap,
            );
        }
        let burst = u16::try_from(rng.range(1, 16)).unwrap();
        let cfg = budget(insertion, rows, burst, rng.range(0, 1) == 1);
        if validate(&cfg, stream).is_err() {
            rejected += 1;
            continue;
        }
        accepted += 1;
        let slots = 20_000;
        let cum = simulate(cfg.clone(), &TrafficClass::ALL, slots);
        let c0 = cfg.classes[C0System];
        assert_band(&cum, C0System, c0.floor_kbps, c0.cap_kbps, burst, 4_000);
        // And every class stays under its cap.
        for (c, cb) in cfg.classes.iter() {
            assert_band(&cum, c, 0, cb.cap_kbps, burst, 4_000);
        }
    }
    assert!(accepted >= 10 && rejected >= 10, "{accepted}/{rejected}");
}
