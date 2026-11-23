//! The only place in the crate a null packet can be made (ADR-002
//! decision 2, r6 F6-2).
//!
//! [`NullKey`]'s field is private to this module, so no other module can
//! build one, and [`TsPacket::null`](crate::packet::TsPacket) requires one.
//! The null packet itself is visible to the pacer only.

use crate::packet::TsPacket;

/// Proof of being inside the pacer's null module. Constructible only here.
pub(crate) struct NullKey(());

/// The stuffing packet: PID 0x1FFF, payload 0xFF. Generic MPEG-2
/// structure; for the OOB stream it is `hypothesized` (r4 lead).
pub(super) const NULL_PACKET: TsPacket = TsPacket::null(NullKey(()));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::SYNC_BYTE;

    #[test]
    fn null_packet_shape() {
        let b = NULL_PACKET.as_bytes();
        assert_eq!(b[0], SYNC_BYTE);
        assert!(NULL_PACKET.is_null());
        assert_eq!(NULL_PACKET.pid_value(), 0x1FFF);
        assert!(!NULL_PACKET.pusi());
        assert!(b[4..].iter().all(|&x| x == 0xFF));
    }
}
