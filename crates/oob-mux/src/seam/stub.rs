//! A stub pull source for box-free validation (issue #15 design note §3).
//!
//! [`StubSource`] is a real [`Pacer`] behind a real scheduler, fed with
//! synthetic cyclic sections, so the null packets it serves are the
//! pacer's own (null packets exist only in the pacer, r6 F6-2) and the
//! continuity counters are the packetizer's. The null ratio is set by how
//! many one-packet carousels share a 100-slot period: `k` carousels give a
//! payload fraction of `k / 100`. Nothing here is a box wire format; the
//! sections are filler on private PIDs 0x0100 and up.
//!
//! This lets the flowgraph side (#36) and the loopback harness run before
//! the real producers exist ("stub allowed", F6-2).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::clock::SystemClock;
use crate::config::ConfigError;
use crate::pacer::stats::PacerStats;
use crate::pacer::{Pacer, PacerConfig, PullError, PullReport, PullSource};
use crate::packet::TsPacket;
use crate::pdu::{CarouselId, Payload, Pdu, Submission, TrafficClass};
use crate::pid::Pid;
use crate::rate::PacketRate;
use crate::scheduler::{OutcomeReceiver, Scheduler, SchedulerConfig, SchedulerHandle};

/// Carousel period, in packet slots; the null-ratio resolution is 1/100.
const GRID_SLOTS: u64 = 100;
/// Section length of the filler sections: one packet each.
const SECTION_LEN: usize = 40;
const FIRST_PID: u16 = 0x0100;

/// Why a stub could not be built.
#[derive(Debug)]
#[non_exhaustive]
pub enum StubError {
    /// `null_ratio` is not in `0.0..=1.0`.
    BadRatio(f64),
    /// The pacer or scheduler settings were refused.
    Config(ConfigError),
    /// Building the filler carousels failed.
    Setup(String),
}

impl fmt::Display for StubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRatio(r) => write!(f, "null ratio {r} is not in 0.0..=1.0"),
            Self::Config(e) => write!(f, "stub configuration refused: {e}"),
            Self::Setup(e) => write!(f, "stub setup failed: {e}"),
        }
    }
}

impl std::error::Error for StubError {}

impl From<ConfigError> for StubError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

/// Settings for a [`StubSource`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StubConfig {
    /// The pacer's packet rate (a parameter until O7).
    pub packet_rate: PacketRate,
    /// The pacer's `max_pull`; the seam takes its own from here.
    pub max_pull: usize,
    /// Target fraction of null packets, `0.0..=1.0`, in steps of 0.01.
    pub null_ratio: f64,
}

/// A [`Pacer`] with synthetic cyclic traffic at a configured null ratio.
pub struct StubSource {
    pacer: Pacer,
    // Held so the scheduler stays open for the life of the stub.
    _handle: SchedulerHandle,
    _outcomes: OutcomeReceiver,
}

impl fmt::Debug for StubSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StubSource")
            .field("pacer", &self.pacer)
            .finish()
    }
}

impl StubSource {
    /// A stub at `config`.
    ///
    /// # Errors
    ///
    /// [`StubError`] for a ratio outside `0.0..=1.0` or refused settings.
    pub fn new(config: StubConfig) -> Result<Self, StubError> {
        let ratio = config.null_ratio;
        if !(0.0..=1.0).contains(&ratio) {
            return Err(StubError::BadRatio(ratio));
        }
        // Rounded to the grid: k of GRID_SLOTS slots carry payload.
        let payload_slots = ((1.0 - ratio) * GRID_SLOTS as f64).round() as usize;
        let (scheduler, handle, outcomes) = Scheduler::new(SchedulerConfig {
            queue_depth: 4,
            max_carousels: payload_slots.max(1),
            outcome_capacity: 4,
            disabled_classes: vec![],
        })?;
        let pacer = Pacer::new(
            PacerConfig {
                packet_rate: config.packet_rate,
                max_pull: config.max_pull,
                pull_budget: Duration::from_millis(50),
            },
            scheduler,
            Arc::new(SystemClock::new()),
        )?;
        let period = config.packet_rate.stream_time(GRID_SLOTS);
        for i in 0..payload_slots {
            let tag = u8::try_from(i % 256).unwrap_or(0);
            let pid = Pid::new(FIRST_PID + u16::try_from(i).unwrap_or(0))
                .map_err(|e| StubError::Setup(e.to_string()))?;
            let submit = handle.submit(Submission::Cyclic {
                id: CarouselId(u32::try_from(i).unwrap_or(0)),
                pdu: Pdu {
                    class: TrafficClass::C0System,
                    priority: 0,
                    pid,
                    release_at: None,
                    deadline: None,
                    payload: Payload::Section(filler_section(tag)),
                    stamp: None,
                },
                period,
            });
            submit.map_err(|e| StubError::Setup(e.to_string()))?;
        }
        Ok(Self {
            pacer,
            _handle: handle,
            _outcomes: outcomes,
        })
    }

    /// A stub that serves only null packets.
    ///
    /// # Errors
    ///
    /// [`StubError`] for refused settings.
    pub fn nulls_only(packet_rate: PacketRate, max_pull: usize) -> Result<Self, StubError> {
        Self::new(StubConfig {
            packet_rate,
            max_pull,
            null_ratio: 1.0,
        })
    }

    /// The pacer's counters.
    pub fn pacer_stats(&self) -> Arc<PacerStats> {
        self.pacer.stats()
    }
}

impl PullSource for StubSource {
    fn pull(&mut self, n: usize, out: &mut Vec<TsPacket>) -> Result<PullReport, PullError> {
        self.pacer.pull(n, out)
    }
}

/// A valid one-packet section: a long-form private header with a correct
/// section_length, filler body, and `tag` in byte 3.
fn filler_section(tag: u8) -> Arc<[u8]> {
    let mut s = vec![0x5Au8; SECTION_LEN];
    let sl = u16::try_from(SECTION_LEN - 3).unwrap_or(0);
    s[0] = 0xC0;
    s[1] = 0xB0 | (sl >> 8) as u8;
    s[2] = sl as u8;
    s[3] = tag;
    s.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate() -> PacketRate {
        PacketRate::from_bits_per_second(2_048_000).unwrap_or_else(|| unreachable!())
    }

    fn ratio_of(null_ratio: f64) -> f64 {
        let mut s = StubSource::new(StubConfig {
            packet_rate: rate(),
            max_pull: 16,
            null_ratio,
        })
        .unwrap_or_else(|e| panic!("{e}"));
        let mut out = Vec::new();
        let (mut nulls, mut total) = (0usize, 0usize);
        for _ in 0..1000 {
            out.clear();
            let r = s.pull(10, &mut out).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(out.len(), 10);
            nulls += r.nulls;
            total += 10;
        }
        nulls as f64 / total as f64
    }

    #[test]
    fn null_ratio_follows_the_configured_fill() {
        assert!((ratio_of(0.4) - 0.4).abs() < 0.02);
        assert!((ratio_of(0.75) - 0.75).abs() < 0.02);
    }

    #[test]
    fn extremes_are_all_payload_and_all_null() {
        assert!(ratio_of(1.0) > 0.999);
        assert!(ratio_of(0.0) < 0.05);
    }

    #[test]
    fn bad_ratio_is_refused() {
        let c = StubConfig {
            packet_rate: rate(),
            max_pull: 4,
            null_ratio: 1.5,
        };
        assert!(matches!(StubSource::new(c), Err(StubError::BadRatio(_))));
    }
}
