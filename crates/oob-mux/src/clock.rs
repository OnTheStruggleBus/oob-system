//! Time sources.
//!
//! Two clocks, two jobs (ADR-002, r6 F6-16):
//!
//! - **monotonic** ([`MonoTime`]): C1 release times and PDU deadlines,
//!   which the command layer sets in wall-clock terms, and the pacer's
//!   own per-pull timing;
//! - **UTC** ([`UtcTime`]): stamping time-bearing PDUs at pull, read from
//!   the chrony-disciplined host.
//!
//! Neither clock paces the stream: stream time is counted in packet slots
//! served to pulls, so the flowgraph's clock governs the rate.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A point on a monotonic clock, as the time elapsed since that clock's
/// origin. Values from different clocks are not comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MonoTime(Duration);

impl MonoTime {
    /// A monotonic instant `since` the clock's origin.
    pub const fn from_origin(since: Duration) -> Self {
        Self(since)
    }

    /// Time since the clock's origin.
    pub const fn since_origin(self) -> Duration {
        self.0
    }

    /// `self - earlier`, or zero if `earlier` is later.
    pub fn saturating_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }

    /// `self + d`, saturating.
    pub fn saturating_add(self, d: Duration) -> Self {
        Self(self.0.saturating_add(d))
    }
}

/// A UTC instant, as the time elapsed since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UtcTime(Duration);

impl UtcTime {
    /// A UTC instant `since` 1970-01-01T00:00:00Z.
    pub const fn from_unix(since: Duration) -> Self {
        Self(since)
    }

    /// Time since the Unix epoch.
    pub const fn since_unix_epoch(self) -> Duration {
        self.0
    }
}

/// The time source the pipeline reads. Implementations must be cheap and
/// non-blocking: the pacer calls both methods on the pull path.
pub trait Clock: Send + Sync {
    /// Monotonic time: scheduling, release times, deadlines, pull timing.
    fn mono(&self) -> MonoTime;
    /// UTC: stamping only.
    fn utc(&self) -> UtcTime;
}

/// The host clock: [`Instant`] for monotonic time, [`SystemTime`] for UTC.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A system clock whose monotonic origin is now.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn mono(&self) -> MonoTime {
        MonoTime(self.origin.elapsed())
    }

    fn utc(&self) -> UtcTime {
        // A host clock set before 1970 reads as the epoch rather than failing.
        UtcTime(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default(),
        )
    }
}

/// A hand-driven clock for tests, including downstream contract tests
/// (the seam in #15). Time moves only when [`ManualClock::advance`] or
/// [`ManualClock::set_utc`] is called; share it through an `Arc` to drive
/// it while the pacer holds it.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: Mutex<(MonoTime, UtcTime)>,
}

impl ManualClock {
    /// A manual clock at the given instants.
    pub fn new(mono: MonoTime, utc: UtcTime) -> Self {
        Self {
            now: Mutex::new((mono, utc)),
        }
    }

    /// Moves both clocks forward by `d`.
    pub fn advance(&self, d: Duration) {
        let mut g = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        g.0 = g.0.saturating_add(d);
        g.1 = UtcTime(g.1.0.saturating_add(d));
    }

    /// Sets the UTC reading without touching monotonic time.
    pub fn set_utc(&self, utc: UtcTime) {
        self.now.lock().unwrap_or_else(PoisonError::into_inner).1 = utc;
    }
}

impl Clock for ManualClock {
    fn mono(&self) -> MonoTime {
        self.now.lock().unwrap_or_else(PoisonError::into_inner).0
    }

    fn utc(&self) -> UtcTime {
        self.now.lock().unwrap_or_else(PoisonError::into_inner).1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_told() {
        let c = ManualClock::new(
            MonoTime::default(),
            UtcTime::from_unix(Duration::from_secs(100)),
        );
        assert_eq!(c.mono(), MonoTime::default());
        c.advance(Duration::from_millis(5));
        assert_eq!(c.mono().since_origin(), Duration::from_millis(5));
        assert_eq!(c.utc().since_unix_epoch(), Duration::from_millis(100_005));
        c.set_utc(UtcTime::from_unix(Duration::from_secs(7)));
        assert_eq!(c.utc().since_unix_epoch(), Duration::from_secs(7));
        assert_eq!(c.mono().since_origin(), Duration::from_millis(5));
    }

    #[test]
    fn system_clock_is_monotonic() {
        let c = SystemClock::new();
        let a = c.mono();
        let b = c.mono();
        assert!(b >= a);
    }
}
