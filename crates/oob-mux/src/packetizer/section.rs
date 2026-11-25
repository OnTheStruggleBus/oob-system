//! Section helpers: the MPEG-2 CRC32 and section segmentation arithmetic.
//!
//! The CRC is the generic MPEG-2 section CRC (polynomial 0x04C11DB7,
//! initial value 0xFFFFFFFF, no reflection, no final XOR). Whether the OOB
//! stream's sections use it is the r4 lead, `hypothesized` until a capture
//! verifies it.

use crate::packet::PAYLOAD_LEN;

const POLY: u32 = 0x04C1_1DB7;

const fn table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u32) << 24;
        let mut k = 0;
        while k < 8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ POLY
            } else {
                c << 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static TABLE: [u32; 256] = table();

/// The MPEG-2 CRC32 of `data`.
pub(crate) fn crc32_mpeg2(data: &[u8]) -> u32 {
    data.iter().fold(0xFFFF_FFFF, |crc, &b| {
        (crc << 8) ^ TABLE[usize::from((crc >> 24) as u8 ^ b)]
    })
}

/// Rewrites the trailing CRC32 of a section after its body changed.
/// Sections shorter than a CRC are left alone (validation rejects
/// time-bearing ones up front).
pub(crate) fn rewrite_crc(section: &mut [u8]) {
    if let Some(body_len) = section.len().checked_sub(4) {
        let crc = crc32_mpeg2(&section[..body_len]).to_be_bytes();
        section[body_len..].copy_from_slice(&crc);
    }
}

/// Section bytes the first packet of a section carries: the payload after
/// the one-byte pointer_field.
pub(crate) const FIRST_PACKET_BYTES: usize = PAYLOAD_LEN - 1;

/// Packet slots a section of `len` bytes occupies: it starts a packet
/// with pointer_field = 0 and its last packet is stuffed with 0xFF. Slot
/// budgets (#16) count PDUs with this.
///
/// ```
/// assert_eq!(oob_mux::packetizer::packets_for(183), 1);
/// assert_eq!(oob_mux::packetizer::packets_for(184), 2);
/// ```
pub const fn packets_for(len: usize) -> usize {
    if len <= FIRST_PACKET_BYTES {
        1
    } else {
        1 + (len - FIRST_PACKET_BYTES).div_ceil(PAYLOAD_LEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        // The CRC-32/MPEG-2 check value for ASCII "123456789".
        assert_eq!(crc32_mpeg2(b"123456789"), 0x0376_E6E7);
        assert_eq!(crc32_mpeg2(&[]), 0xFFFF_FFFF);
    }

    #[test]
    fn a_section_with_its_crc_checks_to_zero() {
        let mut s = *b"\x00\xB0\x09abcde\x00\x00\x00\x00";
        rewrite_crc(&mut s);
        assert_eq!(crc32_mpeg2(&s), 0);
    }

    #[test]
    fn packet_counts() {
        assert_eq!(packets_for(1), 1);
        assert_eq!(packets_for(183), 1);
        assert_eq!(packets_for(184), 2);
        assert_eq!(packets_for(183 + 184), 2);
        assert_eq!(packets_for(183 + 184 + 1), 3);
        assert_eq!(packets_for(4096), 23);
    }

    /// A random section body with its CRC32 appended.
    fn with_crc(body: &[u8]) -> Vec<u8> {
        let mut s = body.to_vec();
        s.extend_from_slice(&[0; 4]);
        rewrite_crc(&mut s);
        s
    }

    proptest::proptest! {
        /// F6-15: any section with its CRC32 appended checks to zero ...
        #[test]
        fn crc_of_section_with_crc_is_zero(
            body in proptest::collection::vec(proptest::num::u8::ANY, 0..4096)
        ) {
            proptest::prop_assert_eq!(crc32_mpeg2(&with_crc(&body)), 0);
        }

        /// ... and any single bit flip, in the body or the CRC, breaks it.
        #[test]
        fn any_single_bit_flip_is_detected(
            body in proptest::collection::vec(proptest::num::u8::ANY, 1..512),
            bit in 0usize..usize::MAX,
        ) {
            let mut s = with_crc(&body);
            let bit = bit % (s.len() * 8);
            s[bit / 8] ^= 1 << (bit % 8);
            proptest::prop_assert_ne!(crc32_mpeg2(&s), 0);
        }
    }
}
