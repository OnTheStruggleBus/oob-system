//! The 188-byte transport packet, the unit of the pull seam (r6 F6-2).
//!
//! [`TsPacket`] has no public constructor. The packetizer builds payload
//! packets from a [`Pid`], which can never be the null PID, and the null
//! packet can only be built with a key that exists solely inside the
//! pacer's private `null` module. The header layout is generic MPEG-2
//! transport structure; for the OOB stream it is the r4 lead and stays
//! `hypothesized` until a capture verifies it.

use crate::pacer::null::NullKey;
use crate::pid::{NULL_PID_VALUE, Pid};

/// Bytes in one transport packet.
pub const PACKET_LEN: usize = 188;
/// Bytes of payload after the 4-byte header (no adaptation field).
pub const PAYLOAD_LEN: usize = PACKET_LEN - 4;
/// The transport sync byte.
pub const SYNC_BYTE: u8 = 0x47;

/// One 188-byte transport packet.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TsPacket([u8; PACKET_LEN]);

impl TsPacket {
    /// Builds a payload-only packet (adaptation_field_control = 01).
    pub(crate) fn payload(pid: Pid, pusi: bool, cc: u8, payload: &[u8; PAYLOAD_LEN]) -> Self {
        let mut b = [0u8; PACKET_LEN];
        let [hi, lo] = pid.value().to_be_bytes();
        b[0] = SYNC_BYTE;
        b[1] = (u8::from(pusi) << 6) | (hi & 0x1F);
        b[2] = lo;
        b[3] = 0x10 | (cc & 0x0F);
        b[4..].copy_from_slice(payload);
        Self(b)
    }

    /// Builds the null packet. Only the pacer's `null` module can make a
    /// [`NullKey`], so only the pacer can call this.
    pub(crate) const fn null(_key: NullKey) -> Self {
        let [hi, lo] = NULL_PID_VALUE.to_be_bytes();
        let mut b = [0xFFu8; PACKET_LEN];
        b[0] = SYNC_BYTE;
        b[1] = hi;
        b[2] = lo;
        b[3] = 0x10;
        Self(b)
    }

    /// The raw packet bytes.
    pub fn as_bytes(&self) -> &[u8; PACKET_LEN] {
        &self.0
    }

    /// The 13-bit PID field (0x1FFF for a null packet).
    pub fn pid_value(&self) -> u16 {
        (u16::from(self.0[1] & 0x1F) << 8) | u16::from(self.0[2])
    }

    /// Whether this is a null (stuffing) packet.
    pub fn is_null(&self) -> bool {
        self.pid_value() == NULL_PID_VALUE
    }

    /// payload_unit_start_indicator: a section starts in this packet.
    pub fn pusi(&self) -> bool {
        self.0[1] & 0x40 != 0
    }

    /// The 4-bit continuity counter.
    pub fn continuity_counter(&self) -> u8 {
        self.0[3] & 0x0F
    }

    /// The 184 payload bytes after the header.
    pub fn payload_bytes(&self) -> &[u8] {
        &self.0[4..]
    }
}

impl core::fmt::Debug for TsPacket {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TsPacket")
            .field("pid", &format_args!("0x{:04X}", self.pid_value()))
            .field("pusi", &self.pusi())
            .field("cc", &self.continuity_counter())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_fields_round_trip() {
        let pid = Pid::new(0x1ABC).unwrap_or_else(|e| panic!("{e}"));
        let p = TsPacket::payload(pid, true, 0x1F, &[0xAA; PAYLOAD_LEN]);
        assert_eq!(p.as_bytes()[0], SYNC_BYTE);
        assert_eq!(p.pid_value(), 0x1ABC);
        assert!(p.pusi());
        assert_eq!(p.continuity_counter(), 0x0F);
        assert!(!p.is_null());
        assert!(p.payload_bytes().iter().all(|&b| b == 0xAA));
    }
}
