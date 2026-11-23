//! Layer 3 of 3: the pacer, the mux side of the pull seam (ADR-002
//! decision 2, r6 F6-2).
//!
//! The flowgraph owns the clock. It asks for `n` packets; the
//! [`Pacer`] answers with exactly `n`, taking them from the packetizer
//! (which takes PDUs from the scheduler only as needed) and filling any
//! shortfall with null packets. Each packet answered, served or null,
//! advances the stream by one *slot*; slots, not the host clock, are the
//! stream's time base, so rate accounting follows the flowgraph's clock.
//! The host monotonic clock is used only for release times, deadlines and
//! timing the pull itself.
//!
//! # Null packets exist only here
//!
//! The null packet is a private constant of this module's private `null`
//! submodule, and [`Pid`](crate::Pid) cannot hold 0x1FFF, so nothing else
//! in the crate, and nothing outside it, can produce one. Outside the
//! crate the only way to get a null packet is to pull one:
//!
//! ```
//! use std::sync::Arc;
//! use std::time::Duration;
//! use oob_mux::{Pacer, PacerConfig, PacketRate, PullSource, Scheduler, SchedulerConfig, SystemClock};
//!
//! let (scheduler, _handle, _outcomes) = Scheduler::new(SchedulerConfig {
//!     queue_depth: 4, max_carousels: 4, outcome_capacity: 4, disabled_classes: vec![],
//! })?;
//! let config = PacerConfig {
//!     packet_rate: PacketRate::from_bits_per_second(1504 * 1000).unwrap_or_else(|| unreachable!()),
//!     max_pull: 8,
//!     pull_budget: Duration::from_millis(1),
//! };
//! let mut pacer = Pacer::new(config, scheduler, Arc::new(SystemClock::new()))?;
//! let mut out = Vec::with_capacity(8);
//! let report = pacer.pull(3, &mut out)?;
//! assert_eq!((out.len(), report.nulls), (3, 3));
//! assert!(out.iter().all(|p| p.is_null()));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The null constructor is private to the crate (E0624):
//!
//! ```compile_fail,E0624
//! let _ = oob_mux::TsPacket::null;
//! ```
//!
//! the null module and its constant are private (E0603):
//!
//! ```compile_fail,E0603
//! let _ = oob_mux::pacer::null::NULL_PACKET;
//! ```
//!
//! and a packet cannot be built from raw bytes (E0423):
//!
//! ```compile_fail,E0423
//! let _ = oob_mux::TsPacket([0x47; 188]);
//! ```
//!
//! # Bounded pull
//!
//! A pull does no I/O, never awaits, and allocates nothing when `out` has
//! capacity for `n` (reserve `max_pull` once). It takes the queue lock only
//! for each dequeue. It times itself on the monotonic clock: a pull that
//! takes longer than `pull_budget` is reported `late` and counted in
//! `late_pulls`. The worst case is `n <= max_pull` packet constructions
//! plus at most one dequeue per PDU started; #16's criterion bench sets
//! the documented number. A request for more than `max_pull` packets is
//! refused with [`PullError::Oversized`], `out` untouched, and counted in
//! `oversized_pulls` (QA-1): a garbled request from the seam cannot make
//! the mux allocate without bound.

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub(crate) mod null;
pub mod stats;

use std::sync::Arc;
use std::time::Duration;

use crate::clock::{Clock, MonoTime};
use crate::config::ConfigError;
use crate::packet::TsPacket;
use crate::packetizer::Packetizer;
use crate::pdu::{Outcome, TrafficClass};
use crate::rate::PacketRate;
use crate::scheduler::Scheduler;
use null::NULL_PACKET;
use stats::PacerStats;

/// The seam contract: whatever sits behind the flowgraph's pull.
pub trait PullSource: Send {
    /// Appends exactly `n` packets to `out`, null-filled on shortfall.
    /// Never blocks on I/O and never awaits.
    ///
    /// # Errors
    ///
    /// [`PullError::Oversized`] when `n` exceeds the source's `max_pull`;
    /// `out` is then left untouched and the stream does not advance.
    fn pull(&mut self, n: usize, out: &mut Vec<TsPacket>) -> Result<PullReport, PullError>;
}

/// Why a pull was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PullError {
    /// The request asked for more packets than `max_pull`.
    Oversized {
        /// Packets asked for.
        n: usize,
        /// The configured `max_pull`.
        max: usize,
    },
}

impl core::fmt::Display for PullError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Oversized { n, max } => write!(f, "pull of {n} packets exceeds max_pull {max}"),
        }
    }
}

impl std::error::Error for PullError {}

/// Pacer settings. Runtime configuration (ADR-003 Layer 1): the values
/// come from oob-dac's store, so there is no default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacerConfig {
    /// The rate the flowgraph pulls at; a parameter until O7 fixes it,
    /// and still a parameter afterwards (r6 F6-2).
    pub packet_rate: PacketRate,
    /// The largest `n` a pull accepts; at least 1.
    pub max_pull: usize,
    /// The documented worst-case duration of one pull; non-zero.
    pub pull_budget: Duration,
}

impl PacerConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.max_pull == 0 {
            return Err(ConfigError::ZeroMaxPull);
        }
        if self.pull_budget.is_zero() {
            return Err(ConfigError::ZeroPullBudget);
        }
        Ok(())
    }
}

/// What one pull did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PullReport {
    /// Non-null packets.
    pub served: usize,
    /// Null packets.
    pub nulls: usize,
    /// The pull exceeded `pull_budget`.
    pub late: bool,
}

/// Scheduler + packetizer + counters behind the pull seam.
pub struct Pacer {
    scheduler: Scheduler,
    packetizer: Packetizer,
    stats: Arc<PacerStats>,
    clock: Arc<dyn Clock>,
    config: PacerConfig,
    slot: u64,
    /// Stream time at `base_slot`; moves on each rate change (QA-2).
    base_time: Duration,
    base_slot: u64,
    class: TrafficClass,
}

impl core::fmt::Debug for Pacer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Pacer")
            .field("config", &self.config)
            .field("slot", &self.slot)
            .field("packetizer", &self.packetizer)
            .finish_non_exhaustive()
    }
}

impl Pacer {
    /// A pacer at slot 0 over `scheduler`.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a zero `max_pull` or `pull_budget`.
    pub fn new(
        config: PacerConfig,
        mut scheduler: Scheduler,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, ConfigError> {
        config.validate()?;
        let stats = Arc::new(PacerStats::default());
        scheduler.set_stats(Arc::clone(&stats));
        Ok(Self {
            scheduler,
            packetizer: Packetizer::new(),
            stats,
            clock,
            config,
            slot: 0,
            base_time: Duration::ZERO,
            base_slot: 0,
            class: TrafficClass::C0System,
        })
    }

    /// The live counters, for `/metrics` (#29).
    pub fn stats(&self) -> Arc<PacerStats> {
        Arc::clone(&self.stats)
    }

    /// The settings in force.
    pub fn config(&self) -> &PacerConfig {
        &self.config
    }

    /// Replaces the settings. Takes effect at the next pull, which is the
    /// only boundary there is: a pull holds `&mut self`. Stream time
    /// accrued so far is kept; later slots accrue at the new rate.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a zero `max_pull` or `pull_budget`; the old
    /// settings then stay in force.
    pub fn apply_config(&mut self, config: PacerConfig) -> Result<(), ConfigError> {
        config.validate()?;
        self.base_time = self.stream_time();
        self.base_slot = self.slot;
        self.config = config;
        Ok(())
    }

    /// Slots answered so far: the stream's time base.
    pub fn slot(&self) -> u64 {
        self.slot
    }

    /// Stream time covered so far. Monotonic across rate changes.
    pub fn stream_time(&self) -> Duration {
        let since = self.slot.saturating_sub(self.base_slot);
        self.base_time
            .saturating_add(self.config.packet_rate.stream_time(since))
    }

    /// The next non-null packet, starting a new PDU if needed. The
    /// scheduler is not asked again before slot `idle_until`: within one
    /// pull, only a carousel reaching its due slot can change its answer.
    fn next_payload_packet(&mut self, now: MonoTime, idle_until: &mut u64) -> Option<TsPacket> {
        loop {
            if let Some(p) = self.packetizer.next_packet() {
                return Some(p);
            }
            if self.slot < *idle_until {
                return None;
            }
            let s = match self.scheduler.next(now, self.slot, self.config.packet_rate) {
                Ok(s) => s,
                Err(retry) => {
                    *idle_until = retry.unwrap_or(u64::MAX).max(self.slot + 1);
                    return None;
                }
            };
            // Stamp at pull (r6 F6-16): UTC is read as the first packet of
            // this PDU is about to be released, never at enqueue.
            let utc = self.clock.utc();
            match self.packetizer.start(&s.pdu, utc) {
                Ok(()) => {
                    self.class = s.pdu.class;
                    if s.reports {
                        let outcome = Outcome::Emitted {
                            first_packet_slot: self.slot,
                            utc,
                        };
                        self.scheduler.report(s.id, outcome);
                    }
                }
                Err(reason) => {
                    self.stats.add_stamper_panic();
                    if s.reports {
                        self.scheduler.report(s.id, Outcome::Dropped(reason));
                    }
                }
            }
        }
    }
}

impl PullSource for Pacer {
    fn pull(&mut self, n: usize, out: &mut Vec<TsPacket>) -> Result<PullReport, PullError> {
        if n > self.config.max_pull {
            self.stats.add_oversized();
            return Err(PullError::Oversized {
                n,
                max: self.config.max_pull,
            });
        }
        let start = self.clock.mono();
        out.reserve(n);
        let mut report = PullReport::default();
        let mut idle_until = 0;
        for _ in 0..n {
            match self.next_payload_packet(start, &mut idle_until) {
                Some(p) => {
                    out.push(p);
                    report.served += 1;
                    self.stats.add_served(self.class, 1);
                }
                None => {
                    out.push(NULL_PACKET);
                    report.nulls += 1;
                }
            }
            self.slot += 1;
        }
        self.stats
            .add_pull(report.served as u64, report.nulls as u64);
        if self.clock.mono().saturating_since(start) > self.config.pull_budget {
            report.late = true;
            self.stats.add_late();
        }
        Ok(report)
    }
}
