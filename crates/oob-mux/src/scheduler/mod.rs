//! Layer 1 of 3: the scheduler (ADR-002 decision 3, r6 F6-12).
//!
//! Producers (the oob-dac command layer, #22) hold a cloneable
//! [`SchedulerHandle`] and [`submit`](SchedulerHandle::submit) PDUs; the
//! pacer owns the consuming [`Scheduler`] and takes one PDU at a time, only
//! when a pull needs one. Nothing is packetized ahead of a pull.
//!
//! Selection is by class budget (#16, r6 F6-3): per-class floors and caps
//! as token buckets in packet slots, C2 preempting C3–C5 within its cap, C1
//! honouring release times, C5 off outside lab mode. See
//! [`BudgetConfig`], [`validate`] and the selection order in the private
//! `queue` module; the book page `protocol/oob-mux-pipeline.md` has the
//! class table.
//!
//! The scheduler persists nothing: oob-dac rebuilds the queues and the
//! carousel registry from its store on daemon start.

mod bucket;
mod budget;
mod class;
mod queue;

pub use budget::{
    BudgetConfig, BudgetViolation, ClassBudget, ClassMap, StreamParams, ValidatedBudget, validate,
};
pub use class::{LAB_GATED, PREEMPTOR, requires_cap};

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::clock::MonoTime;
use crate::config::ConfigError;
use crate::pacer::stats::PacerStats;
use crate::pdu::{
    CarouselId, MAX_SECTION_LEN, Outcome, OutcomeEvent, Pdu, PduId, Submission, TrafficClass,
};
use crate::rate::PacketRate;
use queue::{CarouselEntry, OnceEntry, Pick, Queues, Reaped};

/// Bounded queue sizes and the class budget. Runtime configuration
/// (ADR-003 Layer 1): the values come from oob-dac's store, so there is no
/// default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Maximum one-shot PDUs waiting per class; at least 1.
    pub queue_depth: usize,
    /// Maximum registered carousels across all classes; 0 disables
    /// carousels.
    pub max_carousels: usize,
    /// Capacity of the outcome channel; at least 1. When it is full,
    /// outcomes are counted as lost rather than blocking the pull path.
    pub outcome_capacity: usize,
    /// Per-class floors and caps, and the lab-mode switch that is the one
    /// input that disables a class (C5). Replaced at run time by
    /// [`SchedulerHandle::apply_budget`].
    pub budget: BudgetConfig,
}

impl SchedulerConfig {
    /// Checks the settings without building a scheduler, so oob-dac's save
    /// path can refuse a bad row (ARCH-2). [`Scheduler::new`] runs the same
    /// check.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a zero `queue_depth` or `outcome_capacity`
    /// (QA-7), or the first violation of the budget's own rules. The
    /// budget's fit to the stream rate is checked when the pacer binds the
    /// rate; use [`validate`] to check both at once.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.queue_depth == 0 {
            return Err(ConfigError::ZeroQueueDepth);
        }
        if self.outcome_capacity == 0 {
            return Err(ConfigError::ZeroOutcomeCapacity);
        }
        budget::validate_inner(&self.budget, None).map_err(|v| ConfigError::Budget(v[0]))?;
        Ok(())
    }
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
    /// The class is disabled: C5 while lab mode is off.
    ClassDisabled(TrafficClass),
    /// A C0 carousel that would push the registered C0 demand above C0's
    /// floor (kbps, truncated). Starvation of C0 is unreachable through
    /// registration as it is through [`SchedulerHandle::apply_budget`].
    C0DemandExceedsFloor {
        /// C0's floor in force.
        floor_kbps: u32,
        /// The demand with this carousel registered.
        demand_kbps: u32,
    },
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
            Self::C0DemandExceedsFloor {
                floor_kbps,
                demand_kbps,
            } => write!(
                f,
                "C0 carousel demand {demand_kbps} kbps would exceed the C0 floor {floor_kbps} kbps"
            ),
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
    /// The stream rate in bits/s once the pacer has bound it; 0 before.
    stream_bps: AtomicU64,
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

/// Whether a carousel replacement carries the same content: section bytes,
/// PID, class and priority.
fn same_content(a: &Pdu, b: &Pdu) -> bool {
    a.pid == b.pid
        && a.class == b.class
        && a.priority == b.priority
        && a.payload.as_bytes() == b.payload.as_bytes()
}

fn validate_pdu(pdu: &Pdu) -> Result<(), SubmitError> {
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
        validate_pdu(pdu)?;
        let mut q = s.lock();
        if !q.buckets.enabled(pdu.class) {
            return Err(SubmitError::ClassDisabled(pdu.class));
        }
        let id = PduId(s.next_id.fetch_add(1, Ordering::Relaxed));
        match submission {
            Submission::Once(pdu) => {
                let depth = s.config.queue_depth;
                let class = pdu.class;
                if q.once[class.index()].len() >= depth {
                    return Err(SubmitError::QueueFull { class, depth });
                }
                let seq = q.next_seq();
                q.note_deadline(pdu.deadline);
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
                let demand = q.c0_demand_kbps(Some(carousel), Some((&pdu, period)));
                let floor = q.buckets.config().classes[TrafficClass::C0System].floor_kbps;
                if pdu.class == TrafficClass::C0System && demand > floor {
                    return Err(SubmitError::C0DemandExceedsFloor {
                        floor_kbps: floor,
                        demand_kbps: demand,
                    });
                }
                let seq = q.next_seq();
                let entry = CarouselEntry {
                    id: carousel,
                    pdu_id: id,
                    seq,
                    pdu,
                    period,
                    next_due_slot: 0,
                    urgent: false,
                    last_early: None,
                };
                if let Some(slot) = q.carousels.iter_mut().find(|c| c.id == carousel) {
                    // Same content and period keep the schedule, so
                    // idempotent re-submits never raise the rate. Any
                    // change, the period included, may jump the schedule
                    // (QA-5, QA-9, see `Submission::Cyclic`), but only once
                    // per period: that keeps C0 demand exact, because a
                    // producer that keeps changing a carousel cannot emit
                    // faster than its period (#14 amendment 4).
                    let unchanged =
                        slot.period == entry.period && same_content(&slot.pdu, &entry.pdu);
                    let (old_due, old_seq, old_early) =
                        (slot.next_due_slot, slot.seq, slot.last_early);
                    *slot = entry;
                    slot.next_due_slot = old_due;
                    slot.last_early = old_early;
                    if unchanged {
                        slot.seq = old_seq;
                    } else {
                        slot.urgent = true;
                    }
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

    /// Validates `cfg` against the stream rate (once the pacer has bound
    /// it) and the live C0 carousel demand, then swaps it in at the next
    /// pull boundary: the swap takes the queue lock the pull path takes
    /// for each dequeue. Bucket fills are clamped to the new depths, so
    /// no class gets a burst from the change. Lab mode off drops queued C5
    /// work, reporting each one-shot as `Dropped(LabModeOff)`.
    ///
    /// # Errors
    ///
    /// Every [`BudgetViolation`], [`C0FloorBelowDemand`] included; the old
    /// budget then stays in force.
    ///
    /// [`C0FloorBelowDemand`]: BudgetViolation::C0FloorBelowDemand
    pub fn apply_budget(&self, cfg: BudgetConfig) -> Result<(), Vec<BudgetViolation>> {
        let s = &self.shared;
        let stream = u64_to_rate(s.stream_bps.load(Ordering::Acquire));
        let mut violations = match budget::validate_inner(
            &cfg,
            stream.map(|packet_rate| StreamParams { packet_rate }),
        ) {
            Ok(_) => Vec::new(),
            Err(v) => v,
        };
        let mut q = s.lock();
        let demand = q.c0_demand_kbps(None, None);
        let floor = cfg.classes[TrafficClass::C0System].floor_kbps;
        if demand > floor {
            violations.push(BudgetViolation::C0FloorBelowDemand {
                floor_kbps: floor,
                demand_kbps: demand,
            });
        }
        if !violations.is_empty() {
            return Err(violations);
        }
        q.apply_budget(cfg);
        Ok(())
    }

    /// The budget in force.
    pub fn budget(&self) -> BudgetConfig {
        self.shared.lock().buckets.config().clone()
    }

    /// One-shot PDUs waiting per class, indexed by [`TrafficClass::index`]
    /// (a gauge for #29).
    pub fn queue_depths(&self) -> [usize; 6] {
        let q = self.shared.lock();
        core::array::from_fn(|i| q.once[i].len())
    }
}

fn u64_to_rate(bps: u64) -> Option<PacketRate> {
    PacketRate::from_bits_per_second(bps)
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
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a zero `queue_depth` or `outcome_capacity`
    /// (QA-7).
    pub fn new(
        config: SchedulerConfig,
    ) -> Result<(Self, SchedulerHandle, OutcomeReceiver), ConfigError> {
        config.validate()?;
        let (tx, rx) = sync_channel(config.outcome_capacity);
        let shared = Arc::new(Shared {
            queues: Mutex::new(Queues::new(
                config.queue_depth,
                config.max_carousels,
                config.budget.clone(),
            )),
            closed: AtomicBool::new(false),
            next_id: AtomicU64::new(0),
            stream_bps: AtomicU64::new(0),
            config,
        });
        let scheduler = Self {
            shared: Arc::clone(&shared),
            outcomes: tx,
            stats: Arc::new(PacerStats::default()),
        };
        Ok((scheduler, SchedulerHandle { shared }, rx))
    }

    pub(crate) fn set_stats(&mut self, stats: Arc<PacerStats>) {
        self.stats = stats;
    }

    /// Checks the budget in force against a stream rate, so the pacer can
    /// refuse a rate the budget does not fit (`BudgetExceedsStream`).
    pub(crate) fn check_rate(&self, rate: PacketRate) -> Result<(), ConfigError> {
        let q = self.shared.lock();
        budget::validate(q.buckets.config(), StreamParams { packet_rate: rate })
            .map(drop)
            .map_err(|v| ConfigError::Budget(v[0]))
    }

    /// Records the bound stream rate for [`SchedulerHandle::apply_budget`].
    pub(crate) fn bind_rate(&self, rate: PacketRate) {
        self.shared
            .stream_bps
            .store(rate.bits_per_second(), Ordering::Release);
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
            q.pick(now, slot, rate, |r| match r {
                Reaped::Expired(id, class) => {
                    self.stats.add_expired(class);
                    self.report(id, Outcome::Expired);
                }
                Reaped::Dropped(id, class, reason) => {
                    self.stats.add_dropped(class);
                    self.report(id, Outcome::Dropped(reason));
                }
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
