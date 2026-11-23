//! Layer 1 of 3: the scheduler (ADR-002 decision 3, r6 F6-12).
//!
//! Producers (the oob-dac command layer, #22) hold a cloneable
//! [`SchedulerHandle`] and [`submit`](SchedulerHandle::submit) PDUs; the
//! pacer owns the consuming [`Scheduler`] and takes one PDU at a time, only
//! when a pull needs one. Nothing is packetized ahead of a pull.
//!
//! Selection today is strict class order (C0 first), then priority within
//! the class, then arrival. Class budgets, floors and caps are #16's scope
//! and replace the strict order; they will be counted in packet slots.
//!
//! The scheduler persists nothing: oob-dac rebuilds the queues and the
//! carousel registry from its store on daemon start.

mod queue;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::clock::MonoTime;
use crate::pacer::stats::PacerStats;
use crate::pdu::{
    CarouselId, MAX_SECTION_LEN, Outcome, OutcomeEvent, Pdu, PduId, Submission, TrafficClass,
};
use crate::rate::PacketRate;
use queue::{CarouselEntry, OnceEntry, Pick, Queues};

/// Bounded queue sizes and class switches. Runtime configuration (ADR-003
/// Layer 1): the values come from oob-dac's store, so there is no default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Maximum one-shot PDUs waiting per class.
    pub queue_depth: usize,
    /// Maximum registered carousels across all classes.
    pub max_carousels: usize,
    /// Capacity of the outcome channel. When it is full, outcomes are
    /// counted as lost rather than blocking the pull path.
    pub outcome_capacity: usize,
    /// Classes that refuse submissions.
    pub disabled_classes: Vec<TrafficClass>,
}

/// Why a submission or withdrawal was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubmitError {
    /// The class's one-shot queue is at its configured depth.
    QueueFull {
        /// The full class.
        class: TrafficClass,
        /// Its configured depth.
        depth: usize,
    },
    /// The class is disabled by configuration.
    ClassDisabled(TrafficClass),
    /// The section exceeds [`MAX_SECTION_LEN`].
    PayloadTooLarge {
        /// The offered length.
        len: usize,
    },
    /// The section's length field disagrees with its length, or a
    /// time-bearing section is too short to carry a CRC32.
    MalformedSection {
        /// The offered length.
        len: usize,
    },
    /// A carousel PDU carried a deadline; carousels repeat, so a deadline
    /// has no meaning for them.
    DeadlineOnCyclic,
    /// A carousel period of zero.
    ZeroPeriod,
    /// A new carousel would exceed the configured registry size.
    CarouselLimit {
        /// The configured maximum.
        max: usize,
    },
    /// No carousel with this id is registered.
    UnknownCarousel(CarouselId),
    /// The scheduler (and with it the pacer) is gone.
    Closed,
}

impl core::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::QueueFull { class, depth } => write!(f, "{class:?} queue full ({depth})"),
            Self::ClassDisabled(c) => write!(f, "{c:?} is disabled"),
            Self::PayloadTooLarge { len } => {
                write!(f, "section of {len} bytes exceeds {MAX_SECTION_LEN}")
            }
            Self::MalformedSection { len } => write!(f, "malformed section ({len} bytes)"),
            Self::DeadlineOnCyclic => f.write_str("a carousel PDU cannot carry a deadline"),
            Self::ZeroPeriod => f.write_str("carousel period is zero"),
            Self::CarouselLimit { max } => write!(f, "carousel registry full ({max})"),
            Self::UnknownCarousel(id) => write!(f, "no carousel {}", id.0),
            Self::Closed => f.write_str("scheduler closed"),
        }
    }
}

impl std::error::Error for SubmitError {}

/// The receiving end of the outcome channel, for one-shot PDUs. The
/// command layer drives its status lifecycle from it.
pub type OutcomeReceiver = Receiver<OutcomeEvent>;

struct Shared {
    queues: Mutex<Queues>,
    closed: AtomicBool,
    next_id: AtomicU64,
    config: SchedulerConfig,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Producer side. Cheap to clone; every clone feeds the same queues.
#[derive(Clone)]
pub struct SchedulerHandle {
    shared: Arc<Shared>,
}

impl core::fmt::Debug for SchedulerHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SchedulerHandle").finish_non_exhaustive()
    }
}

fn validate(pdu: &Pdu) -> Result<(), SubmitError> {
    let b = pdu.payload.as_bytes();
    let len = b.len();
    if len > MAX_SECTION_LEN {
        return Err(SubmitError::PayloadTooLarge { len });
    }
    // section_length: the 12 low bits of bytes 1..3, counting the bytes
    // after it (generic MPEG-2 section structure, `hypothesized` here).
    let declared = match b {
        [_, hi, lo, ..] => usize::from(u16::from_be_bytes([*hi & 0x0F, *lo])),
        _ => return Err(SubmitError::MalformedSection { len }),
    };
    if declared + 3 != len || (pdu.stamp.is_some() && len < 3 + 4) {
        return Err(SubmitError::MalformedSection { len });
    }
    Ok(())
}

impl SchedulerHandle {
    /// Queues a submission. Never blocks beyond the queue lock.
    pub fn submit(&self, submission: Submission) -> Result<PduId, SubmitError> {
        let s = &self.shared;
        if s.closed.load(Ordering::Acquire) {
            return Err(SubmitError::Closed);
        }
        let pdu = match &submission {
            Submission::Once(p) | Submission::Cyclic { pdu: p, .. } => p,
        };
        if s.config.disabled_classes.contains(&pdu.class) {
            return Err(SubmitError::ClassDisabled(pdu.class));
        }
        validate(pdu)?;
        let id = PduId(s.next_id.fetch_add(1, Ordering::Relaxed));
        let mut q = s.lock();
        match submission {
            Submission::Once(pdu) => {
                let depth = s.config.queue_depth;
                let class = pdu.class;
                if q.once[class.index()].len() >= depth {
                    return Err(SubmitError::QueueFull { class, depth });
                }
                let seq = q.next_seq();
                q.once[class.index()].push(OnceEntry { id, seq, pdu });
            }
            Submission::Cyclic {
                id: carousel,
                pdu,
                period,
            } => {
                if pdu.deadline.is_some() {
                    return Err(SubmitError::DeadlineOnCyclic);
                }
                if period == Duration::ZERO {
                    return Err(SubmitError::ZeroPeriod);
                }
                let seq = q.next_seq();
                let entry = CarouselEntry {
                    id: carousel,
                    pdu_id: id,
                    seq,
                    pdu,
                    period,
                    next_due_slot: 0,
                };
                if let Some(slot) = q.carousels.iter_mut().find(|c| c.id == carousel) {
                    *slot = entry;
                } else if q.carousels.len() >= s.config.max_carousels {
                    return Err(SubmitError::CarouselLimit {
                        max: s.config.max_carousels,
                    });
                } else {
                    q.carousels.push(entry);
                }
            }
        }
        Ok(id)
    }

    /// Removes a carousel. A PDU of it already in flight finishes.
    pub fn withdraw(&self, id: CarouselId) -> Result<(), SubmitError> {
        let s = &self.shared;
        if s.closed.load(Ordering::Acquire) {
            return Err(SubmitError::Closed);
        }
        let mut q = s.lock();
        let before = q.carousels.len();
        q.carousels.retain(|c| c.id != id);
        if q.carousels.len() == before {
            return Err(SubmitError::UnknownCarousel(id));
        }
        Ok(())
    }
}

/// A PDU handed from the scheduler to the packetizer.
pub(crate) struct Scheduled {
    pub(crate) id: PduId,
    /// One-shot PDUs report an outcome; carousel emissions do not.
    pub(crate) reports: bool,
    pub(crate) pdu: Pdu,
}

/// Consumer side, owned by the pacer.
pub struct Scheduler {
    shared: Arc<Shared>,
    outcomes: SyncSender<OutcomeEvent>,
    stats: Arc<PacerStats>,
}

impl core::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Scheduler").finish_non_exhaustive()
    }
}

impl Scheduler {
    /// A scheduler, its producer handle, and the outcome channel.
    pub fn new(config: SchedulerConfig) -> (Self, SchedulerHandle, OutcomeReceiver) {
        let (tx, rx) = sync_channel(config.outcome_capacity);
        let shared = Arc::new(Shared {
            queues: Mutex::new(Queues::new(config.queue_depth, config.max_carousels)),
            closed: AtomicBool::new(false),
            next_id: AtomicU64::new(0),
            config,
        });
        let scheduler = Self {
            shared: Arc::clone(&shared),
            outcomes: tx,
            stats: Arc::new(PacerStats::default()),
        };
        (scheduler, SchedulerHandle { shared }, rx)
    }

    pub(crate) fn set_stats(&mut self, stats: Arc<PacerStats>) {
        self.stats = stats;
    }

    /// Reports an outcome without blocking; a full channel counts as lost.
    pub(crate) fn report(&self, id: PduId, outcome: Outcome) {
        match self.outcomes.try_send(OutcomeEvent { id, outcome }) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => self.stats.add_outcome_lost(),
        }
    }

    /// The next eligible PDU at monotonic `now` and stream `slot`.
    /// Expired one-shot PDUs are removed and reported on the way. When
    /// nothing is eligible, `Err` carries the earliest slot at which a
    /// carousel becomes due, so the pacer need not re-take the lock before
    /// then.
    pub(crate) fn next(
        &self,
        now: MonoTime,
        slot: u64,
        rate: PacketRate,
    ) -> Result<Scheduled, Option<u64>> {
        let pick = {
            let mut q = self.shared.lock();
            q.pick(now, slot, rate, |id| {
                self.stats.add_expired();
                self.report(id, Outcome::Expired);
            })
        }?;
        Ok(match pick {
            Pick::Once(e) => Scheduled {
                id: e.id,
                reports: true,
                pdu: e.pdu,
            },
            Pick::Carousel { pdu_id, pdu } => Scheduled {
                id: pdu_id,
                reports: false,
                pdu,
            },
        })
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
    }
}
