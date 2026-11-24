//! Per-class queues, the carousel registry and the selection algorithm
//! (#16 design note §3).
//!
//! Selection per slot, all in packet slots, all under the one queue lock:
//!
//! 1. **Phase A, floors of C0 then C1.** The first guaranteed class with
//!    an eligible PDU and a floor token is served.
//! 2. **Phase B, preemption.** C2 (EAS) with an eligible PDU and cap
//!    headroom is served ahead of the C3–C5 floors.
//! 3. **Phase C, floors of C3, C4, C5.**
//! 4. **Phase D, excess.** In class order, the first class with an
//!    eligible PDU and cap headroom.
//!
//! Every serve also needs a token in the insertion-budget bucket. Within
//! a class the order is priority (high first), then release time, then
//! arrival. A PDU with `release_at` in the future is ineligible; the
//! scheduler never delays, reorders or spaces PDUs on its own (F6-11).

use std::time::Duration;

use crate::clock::MonoTime;
use crate::packet::PACKET_LEN;
use crate::packetizer::packets_for;
use crate::pdu::{CarouselId, DropReason, Pdu, PduId, TrafficClass};
use crate::rate::PacketRate;

use super::bucket::{Buckets, Phase};
use super::budget::BudgetConfig;
use super::class::{BELOW_PREEMPTOR, GUARANTEED, PREEMPTOR};

const CLASSES: usize = TrafficClass::ALL.len();

/// A one-shot PDU waiting for its first packet.
pub(super) struct OnceEntry {
    pub(super) id: PduId,
    pub(super) seq: u64,
    pub(super) pdu: Pdu,
}

/// A registered carousel.
pub(super) struct CarouselEntry {
    pub(super) id: CarouselId,
    pub(super) pdu_id: PduId,
    pub(super) seq: u64,
    pub(super) pdu: Pdu,
    pub(super) period: Duration,
    /// When the regular schedule makes it due.
    pub(super) next_due_slot: u64,
    /// A changed replacement may jump the schedule, at most once per
    /// period (#14 amendment 4): this is that pending jump.
    pub(super) urgent: bool,
    /// Slot of the last emission that jumped the schedule.
    pub(super) last_early: Option<u64>,
}

/// What the scheduler picked.
pub(super) enum Pick {
    Once(OnceEntry),
    Carousel { pdu_id: PduId, pdu: Pdu },
}

/// A PDU taken off a queue without being served.
pub(super) enum Reaped {
    Expired(PduId, TrafficClass),
    Dropped(PduId, TrafficClass, DropReason),
}

#[derive(Clone, Copy)]
enum Src {
    Once(usize),
    Carousel(usize),
}

/// The best eligible PDU of a class and its cost in slots.
#[derive(Clone, Copy)]
struct Cand {
    src: Src,
    packets: u64,
}

type Cache = [Option<Option<Cand>>; CLASSES];

/// The shared queue state, guarded by one `std::sync::Mutex`. Producers
/// hold the lock for a push; the pacer holds it for one dequeue.
pub(super) struct Queues {
    pub(super) once: [Vec<OnceEntry>; CLASSES],
    pub(super) carousels: Vec<CarouselEntry>,
    pub(super) seq: u64,
    pub(super) buckets: Buckets,
    /// One-shots dropped by a budget swap, reported at the next pick.
    pending_drops: Vec<(PduId, TrafficClass)>,
    /// A lower bound on the earliest queued one-shot deadline, so the
    /// expiry sweep runs only when something can have expired.
    earliest_deadline: Option<MonoTime>,
}

/// The wire cost of a carousel in bits per second.
fn carousel_bps(pdu: &Pdu, period: Duration) -> u128 {
    let bits = (packets_for(pdu.payload.as_bytes().len()) * PACKET_LEN * 8) as u128;
    bits * 1_000_000_000 / period.as_nanos().max(1)
}

impl Queues {
    /// Empty queues with each one-shot queue's capacity reserved, so a
    /// dequeue never allocates and a push up to `depth` never reallocates.
    pub(super) fn new(depth: usize, max_carousels: usize, budget: BudgetConfig) -> Self {
        Self {
            once: core::array::from_fn(|_| Vec::with_capacity(depth)),
            carousels: Vec::with_capacity(max_carousels),
            seq: 0,
            // The stream rate is bound at the first pick; the fill is
            // rescaled then.
            buckets: Buckets::new(budget, 1, 0),
            pending_drops: Vec::with_capacity(depth),
            earliest_deadline: None,
        }
    }

    /// Notes a queued one-shot's deadline for the sweep bound.
    pub(super) fn note_deadline(&mut self, deadline: Option<MonoTime>) {
        if let Some(d) = deadline {
            self.earliest_deadline = Some(self.earliest_deadline.map_or(d, |e| e.min(d)));
        }
    }

    pub(super) fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// The demand of the registered C0 carousels in kbps, truncated, with
    /// carousel `replacing` left out and `adding` counted instead.
    pub(super) fn c0_demand_kbps(
        &self,
        replacing: Option<CarouselId>,
        adding: Option<(&Pdu, Duration)>,
    ) -> u32 {
        let mut bps: u128 = self
            .carousels
            .iter()
            .filter(|c| c.pdu.class == TrafficClass::C0System && Some(c.id) != replacing)
            .map(|c| carousel_bps(&c.pdu, c.period))
            .sum();
        if let Some((p, period)) = adding.filter(|(p, _)| p.class == TrafficClass::C0System) {
            bps += carousel_bps(p, period);
        }
        u32::try_from(bps / 1000).unwrap_or(u32::MAX)
    }

    /// Swaps the budget in. Lab mode off drops queued C5 work: one-shots
    /// are reported at the next pick, C5 carousels are removed.
    pub(super) fn apply_budget(&mut self, cfg: BudgetConfig) {
        let lab = cfg.lab_mode;
        self.buckets.apply(cfg);
        if !lab {
            let c5 = TrafficClass::C5Lab;
            for e in self.once[c5.index()].drain(..) {
                self.pending_drops.push((e.id, c5));
            }
            self.carousels.retain(|c| c.pdu.class != c5);
        }
    }

    fn candidate(
        &self,
        class: TrafficClass,
        now: MonoTime,
        slot: u64,
        rate: PacketRate,
    ) -> Option<Cand> {
        let released = |p: &Pdu| p.release_at.is_none_or(|r| r <= now);
        // Order key: priority high first, then earlier release (none
        // first), then earlier arrival.
        let key = |p: &Pdu, seq: u64| {
            (
                p.priority,
                core::cmp::Reverse(p.release_at),
                core::cmp::Reverse(seq),
            )
        };
        let best_once = self.once[class.index()]
            .iter()
            .enumerate()
            .filter(|(_, e)| released(&e.pdu))
            .max_by_key(|(_, e)| key(&e.pdu, e.seq))
            .map(|(i, e)| (i, key(&e.pdu, e.seq)));
        let best_car = self
            .carousels
            .iter()
            .enumerate()
            .filter(|(_, c)| c.pdu.class == class && released(&c.pdu) && c.is_due(slot, rate))
            .max_by_key(|(_, c)| key(&c.pdu, c.seq))
            .map(|(i, c)| (i, key(&c.pdu, c.seq)));
        let (src, len) = match (best_once, best_car) {
            (None, None) => return None,
            (Some((i, _)), None) => (
                Src::Once(i),
                self.once[class.index()][i].pdu.payload.as_bytes().len(),
            ),
            (Some((i, ko)), Some((_, kc))) if ko > kc => (
                Src::Once(i),
                self.once[class.index()][i].pdu.payload.as_bytes().len(),
            ),
            (_, Some((i, _))) => (
                Src::Carousel(i),
                self.carousels[i].pdu.payload.as_bytes().len(),
            ),
        };
        Some(Cand {
            src,
            packets: packets_for(len) as u64,
        })
    }

    fn take(&mut self, class: TrafficClass, cand: Cand, slot: u64, rate: PacketRate) -> Pick {
        match cand.src {
            Src::Once(i) => Pick::Once(self.once[class.index()].swap_remove(i)),
            Src::Carousel(i) => {
                let seq = self.next_seq();
                let period_slots = rate.slots_for(self.carousels[i].period).max(1);
                let c = &mut self.carousels[i];
                if slot < c.next_due_slot {
                    c.last_early = Some(slot);
                }
                c.urgent = false;
                c.seq = seq;
                c.next_due_slot = slot.saturating_add(period_slots);
                Pick::Carousel {
                    pdu_id: c.pdu_id,
                    pdu: c.pdu.clone(),
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn try_class(
        &mut self,
        class: TrafficClass,
        phase: Phase,
        now: MonoTime,
        slot: u64,
        rate: PacketRate,
        cache: &mut Cache,
    ) -> Option<Pick> {
        if !self.buckets.enabled(class) {
            return None;
        }
        let cand =
            (*cache[class.index()].get_or_insert_with(|| self.candidate(class, now, slot, rate)))?;
        if !self.buckets.allows(class, phase) {
            return None;
        }
        self.buckets.charge(class, phase, cand.packets);
        Some(self.take(class, cand, slot, rate))
    }

    /// Removes every one-shot PDU whose deadline has passed, in every
    /// class (QA-4), calling `reaped` for each; reports one-shots a budget
    /// swap dropped; then picks the next PDU by the phases above.
    /// Allocation-free.
    ///
    /// A PDU is still eligible at exactly its deadline ("by this instant").
    ///
    /// When nothing is served, returns the earliest slot at which that
    /// could change: a carousel coming due, or a bucket holding a token
    /// for a class that has work.
    pub(super) fn pick(
        &mut self,
        now: MonoTime,
        slot: u64,
        rate: PacketRate,
        mut reaped: impl FnMut(Reaped),
    ) -> Result<Pick, Option<u64>> {
        for (id, class) in self.pending_drops.drain(..) {
            reaped(Reaped::Dropped(id, class, DropReason::LabModeOff));
        }
        if self.earliest_deadline.is_some_and(|d| d < now) {
            let mut earliest: Option<MonoTime> = None;
            for class in TrafficClass::ALL {
                let q = &mut self.once[class.index()];
                let mut i = 0;
                while i < q.len() {
                    match q[i].pdu.deadline {
                        Some(d) if d < now => {
                            reaped(Reaped::Expired(q.swap_remove(i).id, class));
                        }
                        d => {
                            if let Some(d) = d {
                                earliest = Some(earliest.map_or(d, |e| e.min(d)));
                            }
                            i += 1;
                        }
                    }
                }
            }
            self.earliest_deadline = earliest;
        }
        self.buckets.advance(slot, rate.bits_per_second());

        let mut cache: Cache = [None; CLASSES];
        let phases = GUARANTEED
            .into_iter()
            .map(|c| (c, Phase::Floor))
            .chain([(PREEMPTOR, Phase::Preempt)])
            .chain(BELOW_PREEMPTOR.into_iter().map(|c| (c, Phase::Floor)))
            .chain(TrafficClass::ALL.into_iter().map(|c| (c, Phase::Excess)));
        for (class, phase) in phases {
            if let Some(p) = self.try_class(class, phase, now, slot, rate, &mut cache) {
                return Ok(p);
            }
        }

        // Nothing served. Within one pull `now` is fixed and only the
        // slot moves, so what can change before the next pull is a
        // carousel reaching its due slot or a bucket refilling for a class
        // that has work.
        let mut wake: Option<u64> = None;
        let mut note = |at: u64| wake = Some(wake.map_or(at, |w| w.min(at)));
        for class in TrafficClass::ALL {
            if !self.buckets.enabled(class) {
                continue;
            }
            let has_work = (*cache[class.index()]
                .get_or_insert_with(|| self.candidate(class, now, slot, rate)))
            .is_some();
            let w = self.buckets.wait_slots(class);
            if has_work && w != u64::MAX {
                note(slot.saturating_add(w));
            }
        }
        for c in &self.carousels {
            if self.buckets.enabled(c.pdu.class) && c.pdu.release_at.is_none_or(|r| r <= now) {
                note(c.due_slot(rate));
            }
        }
        Err(wake)
    }
}

impl CarouselEntry {
    /// The first slot at which this carousel is eligible.
    fn due_slot(&self, rate: PacketRate) -> u64 {
        match (self.urgent, self.last_early) {
            (true, Some(l)) => self
                .next_due_slot
                .min(l.saturating_add(rate.slots_for(self.period).max(1))),
            (true, None) => 0,
            (false, _) => self.next_due_slot,
        }
    }

    fn is_due(&self, slot: u64, rate: PacketRate) -> bool {
        self.due_slot(rate) <= slot
    }
}
