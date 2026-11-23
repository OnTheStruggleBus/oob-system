//! Pacer counters: plain atomics, read by #29's `/metrics`.
//!
//! The underrun vocabulary is fixed here so #15, #29 and #36 agree:
//!
//! - `nulls_emitted`: null fill on shortfall. Normal operation, not an
//!   error.
//! - `late_pulls`: a pull took longer than its configured budget.
//! - `puller_underruns`: the flowgraph timed out waiting for a pull and
//!   inserted nulls itself. The flowgraph counts these and reports them
//!   over the seam (#15), which records them with
//!   [`PacerStats::record_puller_underruns`]. This is the G0.5 evidence
//!   counter.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use crate::pdu::TrafficClass;

const CLASSES: usize = TrafficClass::ALL.len();

/// Live counters. Shared as `Arc<PacerStats>`; every field only grows.
#[derive(Debug, Default)]
pub struct PacerStats {
    slots: AtomicU64,
    packets_served: AtomicU64,
    nulls_emitted: AtomicU64,
    late_pulls: AtomicU64,
    oversized_pulls: AtomicU64,
    puller_underruns: AtomicU64,
    expired: AtomicU64,
    stamper_panics: AtomicU64,
    outcomes_lost: AtomicU64,
    served_by_class: [AtomicU64; CLASSES],
}

/// A point-in-time copy of [`PacerStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PacerStatsSnapshot {
    /// Packet slots answered: `packets_served + nulls_emitted`.
    pub slots: u64,
    /// Non-null packets released.
    pub packets_served: u64,
    /// Null packets filled on shortfall.
    pub nulls_emitted: u64,
    /// Pulls that exceeded the configured `pull_budget`.
    pub late_pulls: u64,
    /// Pulls asking for more than `max_pull` packets (served in full,
    /// outside the documented time bound).
    pub oversized_pulls: u64,
    /// Puller-side underruns reported by the flowgraph over the seam.
    pub puller_underruns: u64,
    /// One-shot PDUs dropped because their deadline passed.
    pub expired: u64,
    /// PDUs dropped because their stamper panicked.
    pub stamper_panics: u64,
    /// Outcomes not delivered because the outcome channel was full.
    pub outcomes_lost: u64,
    /// Non-null packets per traffic class, indexed by
    /// [`TrafficClass::index`].
    pub served_by_class: [u64; CLASSES],
}

impl PacerStats {
    /// Copies every counter.
    pub fn snapshot(&self) -> PacerStatsSnapshot {
        PacerStatsSnapshot {
            slots: self.slots.load(Relaxed),
            packets_served: self.packets_served.load(Relaxed),
            nulls_emitted: self.nulls_emitted.load(Relaxed),
            late_pulls: self.late_pulls.load(Relaxed),
            oversized_pulls: self.oversized_pulls.load(Relaxed),
            puller_underruns: self.puller_underruns.load(Relaxed),
            expired: self.expired.load(Relaxed),
            stamper_panics: self.stamper_panics.load(Relaxed),
            outcomes_lost: self.outcomes_lost.load(Relaxed),
            served_by_class: core::array::from_fn(|i| self.served_by_class[i].load(Relaxed)),
        }
    }

    /// Records `n` underruns the flowgraph reported (called by the seam
    /// endpoint, #15).
    pub fn record_puller_underruns(&self, n: u64) {
        self.puller_underruns.fetch_add(n, Relaxed);
    }

    pub(crate) fn add_pull(&self, served: u64, nulls: u64) {
        self.slots.fetch_add(served + nulls, Relaxed);
        self.packets_served.fetch_add(served, Relaxed);
        self.nulls_emitted.fetch_add(nulls, Relaxed);
    }

    pub(crate) fn add_served(&self, class: TrafficClass, n: u64) {
        self.served_by_class[class.index()].fetch_add(n, Relaxed);
    }

    pub(crate) fn add_late(&self) {
        self.late_pulls.fetch_add(1, Relaxed);
    }

    pub(crate) fn add_oversized(&self) {
        self.oversized_pulls.fetch_add(1, Relaxed);
    }

    pub(crate) fn add_expired(&self) {
        self.expired.fetch_add(1, Relaxed);
    }

    pub(crate) fn add_stamper_panic(&self) {
        self.stamper_panics.fetch_add(1, Relaxed);
    }

    pub(crate) fn add_outcome_lost(&self) {
        self.outcomes_lost.fetch_add(1, Relaxed);
    }
}
