//! The stream's packet rate: a runtime parameter, never a constant.
//!
//! O7 (the PHY record) has not fixed the OOB stream's bit rate or its
//! framing, so the rate the flowgraph pulls at is passed in (ADR-003
//! Layer 1, r6 F6-2). Rate accounting in this crate is done in packet
//! slots; this type only converts between slots and stream time.

use std::time::Duration;

use crate::packet::PACKET_LEN;

const BITS_PER_PACKET: u128 = PACKET_LEN as u128 * 8;
const NANOS_PER_SEC: u128 = 1_000_000_000;

/// The aggregate transport rate the flowgraph pulls at, in bits per
/// second of 188-byte packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PacketRate {
    bits_per_second: u64,
}

impl PacketRate {
    /// A rate of `bits_per_second`; `None` for zero.
    ///
    /// ```
    /// use oob_mux::PacketRate;
    /// use std::time::Duration;
    ///
    /// let r = PacketRate::from_bits_per_second(1504 * 100).unwrap_or_else(|| unreachable!());
    /// assert_eq!(r.slots_for(Duration::from_secs(1)), 100);
    /// assert_eq!(r.stream_time(50), Duration::from_millis(500));
    /// ```
    pub const fn from_bits_per_second(bits_per_second: u64) -> Option<Self> {
        if bits_per_second == 0 {
            None
        } else {
            Some(Self { bits_per_second })
        }
    }

    /// The configured rate in bits per second.
    pub const fn bits_per_second(self) -> u64 {
        self.bits_per_second
    }

    /// Packet slots in `d` of stream time, rounded up.
    pub fn slots_for(self, d: Duration) -> u64 {
        let num = d
            .as_nanos()
            .saturating_mul(u128::from(self.bits_per_second));
        let den = BITS_PER_PACKET * NANOS_PER_SEC;
        u64::try_from(num.div_ceil(den)).unwrap_or(u64::MAX)
    }

    /// Stream time covered by `slots` packet slots, rounded down to the
    /// nanosecond.
    pub fn stream_time(self, slots: u64) -> Duration {
        let nanos =
            u128::from(slots) * BITS_PER_PACKET * NANOS_PER_SEC / u128::from(self.bits_per_second);
        let secs = u64::try_from(nanos / NANOS_PER_SEC).unwrap_or(u64::MAX);
        // The remainder is below 10^9, so it always fits a u32.
        let sub = u32::try_from(nanos % NANOS_PER_SEC).unwrap_or(0);
        Duration::new(secs, sub)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_rate_is_rejected() {
        assert!(PacketRate::from_bits_per_second(0).is_none());
    }

    #[test]
    fn slots_round_up() {
        let r = PacketRate::from_bits_per_second(1504).unwrap_or_else(|| unreachable!());
        assert_eq!(r.slots_for(Duration::ZERO), 0);
        assert_eq!(r.slots_for(Duration::from_millis(1)), 1);
        assert_eq!(r.slots_for(Duration::from_secs(3)), 3);
    }
}
