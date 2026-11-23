//! Protocol data units: what producers hand the scheduler.
//!
//! A [`Pdu`] is one complete section with its routing (PID), its traffic
//! class (r6 F6-3), a within-class priority, an optional release time
//! (C1 per-terminal spacing set by the command layer, r6 F6-11) and an
//! optional deadline. Time-bearing PDUs carry a [`TimeStamper`] and are
//! stamped when the pacer releases their first packet (r6 F6-16).

use std::sync::Arc;
use std::time::Duration;

use crate::clock::{MonoTime, UtcTime};
use crate::pid::Pid;

/// Traffic classes sharing the insertion budget (r6 F6-3, ADR-002).
///
/// The class *set* and its semantics are code (ADR-003 Layer 3). Budgets,
/// floors and caps per class are runtime settings and arrive with #16;
/// until then the scheduler serves classes in strict declaration order
/// (C0 first), a placeholder #16 replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TrafficClass {
    /// System: time, connect/config carousel.
    C0System,
    /// Addressed: lifecycle operations, EMMs, polls.
    C1Addressed,
    /// Emergency alert messages.
    C2Eas,
    /// Guide listings carousel.
    C3Guide,
    /// Code download carousel.
    C4Cdl,
    /// Discovery Lab injection.
    C5Lab,
}

impl TrafficClass {
    /// Every class, in the scheduler's current service order.
    pub const ALL: [Self; 6] = [
        Self::C0System,
        Self::C1Addressed,
        Self::C2Eas,
        Self::C3Guide,
        Self::C4Cdl,
        Self::C5Lab,
    ];

    /// Dense index (0..6) for per-class tables.
    pub const fn index(self) -> usize {
        match self {
            Self::C0System => 0,
            Self::C1Addressed => 1,
            Self::C2Eas => 2,
            Self::C3Guide => 3,
            Self::C4Cdl => 4,
            Self::C5Lab => 5,
        }
    }
}

/// The largest section the packetizer accepts, in bytes. A 12-bit
/// section_length allows more, but private sections are bounded at 4096
/// bytes; for the OOB stream this is generic MPEG-2 structure and stays
/// `hypothesized`.
pub const MAX_SECTION_LEN: usize = 4096;

/// What a PDU carries.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Payload {
    /// One complete section, header through CRC32. Shared, so a carousel
    /// re-emits it without copying on the producer side.
    Section(Arc<[u8]>),
}

impl Payload {
    /// The payload bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Section(b) => b,
        }
    }
}

/// Writes the time field of a time-bearing section at pull (r6 F6-16).
/// Implemented by `oob-msg`'s time-family encoders.
pub trait TimeStamper: Send + Sync {
    /// Writes the time field of `section` in place for `at`.
    ///
    /// Must not allocate or block. The packetizer recomputes the CRC32
    /// afterwards. A panic here is contained: the PDU is dropped
    /// ([`DropReason::StamperPanicked`]), the panic payload is leaked
    /// rather than dropped (its `Drop` could panic again), and the stream
    /// continues. Containment requires `panic = "unwind"`, the default;
    /// no Oob build profile may set `panic = "abort"`.
    fn stamp(&self, section: &mut [u8], at: UtcTime);
}

/// One unit of work for the scheduler.
#[derive(Clone)]
pub struct Pdu {
    /// Traffic class (r6 F6-3).
    pub class: TrafficClass,
    /// Within-class ordering; higher goes first, FIFO among equals.
    pub priority: u8,
    /// The PID the section is carried on; never the null PID.
    pub pid: Pid,
    /// Not eligible before this monotonic instant (C1 spacing, r6 F6-11).
    pub release_at: Option<MonoTime>,
    /// Dropped and reported [`Outcome::Expired`] if its first packet has
    /// not been released by this monotonic instant.
    pub deadline: Option<MonoTime>,
    /// The section itself.
    pub payload: Payload,
    /// Present on time-bearing PDUs: stamped at pull, not at enqueue.
    pub stamp: Option<Arc<dyn TimeStamper>>,
}

impl core::fmt::Debug for Pdu {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Pdu")
            .field("class", &self.class)
            .field("priority", &self.priority)
            .field("pid", &self.pid)
            .field("release_at", &self.release_at)
            .field("deadline", &self.deadline)
            .field("payload_len", &self.payload.as_bytes().len())
            .field("time_bearing", &self.stamp.is_some())
            .finish()
    }
}

/// Names a carousel so it can be replaced or withdrawn. Chosen by the
/// producer (for example one id per PSI table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CarouselId(pub u32);

/// Identifies one accepted submission; assigned by the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PduId(pub(crate) u64);

impl PduId {
    /// The raw id, unique per scheduler.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// How a PDU enters the scheduler.
#[derive(Debug, Clone)]
pub enum Submission {
    /// Emitted once, then reported on the outcome channel.
    Once(Pdu),
    /// Repeats every `period` of stream time until replaced (same `id`)
    /// or withdrawn (PSI, connect/config carousel). A PDU already in
    /// flight is never cut; a replacement takes effect at the next PDU
    /// boundary. When it is due depends on what changed:
    ///
    /// - **anything changed** (section bytes, PID, class, priority or
    ///   period; for example a new PSI version): due at once;
    /// - **nothing changed** (an idempotent re-submit, such as oob-dac
    ///   reconciling from its store): keeps the old schedule, so
    ///   re-submitting never raises the carousel's rate (QA-5, QA-9).
    ///
    /// Carousel emissions are counted in the pacer's statistics, not
    /// reported per emission.
    Cyclic {
        /// The carousel's name.
        id: CarouselId,
        /// The section to repeat. Must not carry a deadline.
        pdu: Pdu,
        /// Repetition period, measured in stream time (packet slots).
        period: Duration,
    },
}

/// What happened to a one-shot PDU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// Its first packet went out in stream slot `first_packet_slot`,
    /// stamped (if time-bearing) with `utc`.
    Emitted {
        /// The pacer slot of the first packet.
        first_packet_slot: u64,
        /// UTC read when the first packet was released.
        utc: UtcTime,
    },
    /// The deadline passed before the first packet; nothing went out.
    Expired,
    /// Dropped for another reason; nothing went out.
    Dropped(DropReason),
}

/// Why a PDU was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DropReason {
    /// The PDU's [`TimeStamper`] panicked.
    StamperPanicked,
}

/// One entry on the outcome channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutcomeEvent {
    /// The submission this is about.
    pub id: PduId,
    /// What happened.
    pub outcome: Outcome,
}
