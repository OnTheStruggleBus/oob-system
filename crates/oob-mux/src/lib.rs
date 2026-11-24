//! OOB transport: scheduler → packetizer → pacer, the PID plan and the
//! PSI/SI assemblers.
//!
//! PSI/SI is DCII SI, not DVB SI (r5 F5-6, accepted 2026-11-22). The OOB
//! stream is QPSK at 1.024 Msym/s, a 2.048 Mb/s aggregate (ADR-001), with
//! a ~1000 Kbps insertion budget for DAC traffic shared by the traffic
//! classes C0–C5 (r6 F6-3). The GNU Radio flowgraph owns RF timing; this
//! crate owns bytes, and the pacer null-stuffs at pull.
//!
//! # The three layers (ADR-002 decision 3, r6 F6-12)
//!
//! ```text
//!  producers (oob-dac command layer, #22)
//!        │ SchedulerHandle::submit(Submission)
//!        ▼
//!  [scheduler]  per-class queues + carousel registry
//!        │ one PDU, only when a pull needs one
//!        ▼
//!  [packetizer] PDU → 188-byte packets; CC per PID; stamp at pull
//!        │ one packet at a time
//!        ▼
//!  [pacer]      PullSource::pull(n ≤ max_pull) → exactly n packets, nulls on shortfall
//!        ▲
//!        │ the pull seam ([`seam`], #15): the flowgraph asks, the flowgraph's clock rules
//! ```
//!
//! - [`scheduler`]: [`Pdu`]s with class, priority, release time and
//!   deadline; one-shot or cyclic ([`Submission`]); outcomes on a bounded
//!   channel. Serves the insertion budget by class (#16, r6 F6-3):
//!   per-class floors and caps as token buckets in packet slots
//!   ([`scheduler::BudgetConfig`]), checked by the pure, exported
//!   [`scheduler::validate`] that oob-dac's save path calls. C0 system
//!   and C1 addressed are guaranteed, C2 (EAS) preempts the C3–C5
//!   floors within its cap, C1 honours release times and infers no
//!   spacing, C5 (lab) is off outside lab mode.
//! - [`packetizer`]: one PDU in flight, segmented on demand. Time-bearing
//!   PDUs are stamped when their first packet is released (r6 F6-16).
//! - [`pacer`]: the seam's mux side. Null packets exist only here.
//!   Stream time is counted in packet slots, so the rate is whatever the
//!   flowgraph pulls; [`PacketRate`] is a runtime parameter (O7 open).
//!
//! The pull path is synchronous and bounded: no I/O, no `await`, no
//! allocation beyond the caller's buffer, and the queue lock only for
//! each dequeue. Counters ([`PacerStats`]), per-class served packets and
//! bytes, expired and dropped included, are plain atomics; exposing them
//! as `/metrics` is #29. `cargo bench -p oob-mux --bench scheduler`
//! measures the pull under saturation.
//!
//! # What is a hypothesis
//!
//! The 188-byte MPEG-2 transport framing of the OOB stream is the r4 lead,
//! corroborated only by the d 02/d 14 diagnostics. Everything here that
//! depends on it (packet header, null PID, section-in-packet framing,
//! CRC32) is **`hypothesized`** until a capture in `lab/captures/`
//! verifies it. SCTE 55-1 coding, interleaving and randomization are the
//! flowgraph's job, not this crate's (r6 F6-2).
//!
//! The seam endpoint ([`seam`], #15) serves the pull over a Unix socket:
//! see `protocol/seam.md` in the book. The PSI assemblers (#17) arrive as
//! a sibling module. Nothing here is hub-specific (AGENTS.md §9).

pub mod clock;
pub mod config;
pub mod pacer;
pub mod packet;
pub mod packetizer;
pub mod pdu;
pub mod pid;
pub mod rate;
pub mod scheduler;
pub mod seam;

pub use clock::{Clock, ManualClock, MonoTime, SystemClock, UtcTime};
pub use config::ConfigError;
pub use pacer::stats::{PacerStats, PacerStatsSnapshot};
pub use pacer::{Pacer, PacerConfig, PullError, PullReport, PullSource};
pub use packet::TsPacket;
pub use packetizer::Packetizer;
pub use pdu::{
    CarouselId, DropReason, Outcome, OutcomeEvent, Payload, Pdu, PduId, Submission, TimeStamper,
    TrafficClass,
};
pub use pid::{Pid, PidError};
pub use rate::PacketRate;
pub use scheduler::{OutcomeReceiver, Scheduler, SchedulerConfig, SchedulerHandle, SubmitError};
pub use seam::{SeamConfig, SeamError, SeamStats, SeamStatsSnapshot, ShutdownFlag};
