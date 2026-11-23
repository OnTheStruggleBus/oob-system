//! Per-class queues: a one-shot queue per class plus the carousel
//! registry. Class budgets, floors and caps land in #16; today the
//! selection is strict class order, then priority, then arrival.

use std::time::Duration;

use crate::clock::MonoTime;
use crate::pdu::{CarouselId, Pdu, PduId, TrafficClass};
use crate::rate::PacketRate;

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
    pub(super) next_due_slot: u64,
}

/// What the scheduler picked.
pub(super) enum Pick {
    Once(OnceEntry),
    Carousel { pdu_id: PduId, pdu: Pdu },
}

/// The shared queue state, guarded by one `std::sync::Mutex`. Producers
/// hold the lock for a push; the pacer holds it for one dequeue.
pub(super) struct Queues {
    pub(super) once: [Vec<OnceEntry>; CLASSES],
    pub(super) carousels: Vec<CarouselEntry>,
    pub(super) seq: u64,
}

impl Queues {
    /// Empty queues with each one-shot queue's capacity reserved, so a
    /// dequeue never allocates and a push up to `depth` never reallocates.
    pub(super) fn new(depth: usize, max_carousels: usize) -> Self {
        Self {
            once: core::array::from_fn(|_| Vec::with_capacity(depth)),
            carousels: Vec::with_capacity(max_carousels),
            seq: 0,
        }
    }

    pub(super) fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Removes every one-shot PDU whose deadline has passed, calling
    /// `expired` for each, then picks the next eligible PDU in strict
    /// class order. Allocation-free.
    ///
    /// When nothing is eligible, returns the earliest slot at which a
    /// carousel becomes due, if any.
    pub(super) fn pick(
        &mut self,
        now: MonoTime,
        slot: u64,
        rate: PacketRate,
        mut expired: impl FnMut(PduId),
    ) -> Result<Pick, Option<u64>> {
        for class in TrafficClass::ALL {
            let q = &mut self.once[class.index()];
            let mut i = 0;
            while i < q.len() {
                if q[i].pdu.deadline.is_some_and(|d| d < now) {
                    expired(q.swap_remove(i).id);
                } else {
                    i += 1;
                }
            }
            let released = |p: &Pdu| p.release_at.is_none_or(|r| r <= now);
            // Order key: higher priority first, then earlier arrival.
            let key = |priority: u8, seq: u64| (priority, core::cmp::Reverse(seq));

            let best_once = q
                .iter()
                .enumerate()
                .filter(|(_, e)| released(&e.pdu))
                .max_by_key(|(_, e)| key(e.pdu.priority, e.seq))
                .map(|(i, e)| (i, key(e.pdu.priority, e.seq)));
            let best_car = self
                .carousels
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.pdu.class == class && c.next_due_slot <= slot && released(&c.pdu)
                })
                .max_by_key(|(_, c)| key(c.pdu.priority, c.seq))
                .map(|(i, c)| (i, key(c.pdu.priority, c.seq)));

            let car = match (best_once, best_car) {
                (None, None) => continue,
                (Some((i, _)), None) => return Ok(Pick::Once(q.swap_remove(i))),
                (Some((i, ko)), Some((_, kc))) if ko > kc => {
                    return Ok(Pick::Once(q.swap_remove(i)));
                }
                (_, Some((i, _))) => i,
            };
            let seq = self.next_seq();
            let c = &mut self.carousels[car];
            c.seq = seq;
            c.next_due_slot = slot.saturating_add(rate.slots_for(c.period).max(1));
            return Ok(Pick::Carousel {
                pdu_id: c.pdu_id,
                pdu: c.pdu.clone(),
            });
        }
        // Nothing eligible. Within one pull `now` is fixed and only the
        // slot moves, so the only thing that can become eligible before
        // the next pull is a released carousel reaching its due slot.
        Err(self
            .carousels
            .iter()
            .filter(|c| c.pdu.release_at.is_none_or(|r| r <= now))
            .map(|c| c.next_due_slot)
            .min())
    }
}
