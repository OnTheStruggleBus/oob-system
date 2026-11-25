//! Long-form section assembly (ISO/IEC 13818-1 section syntax,
//! **`hypothesized`**: no copy of the standard is in hand, design note on
//! #17). One section per table, section_number = last_section_number = 0,
//! current_next_indicator = 1.

use crate::packetizer::section::crc32_mpeg2;

/// Builds a complete long-form section: header, `body`, CRC32.
///
/// `table_id_extension` is transport_stream_id (PAT), program_number
/// (PMT) or 0xFFFF (CAT). Returns the section and its section_length
/// field value.
pub(crate) fn long_section(
    table_id: u8,
    table_id_extension: u16,
    version: u8,
    body: &[u8],
) -> (Vec<u8>, usize) {
    // After the length field: 5 header bytes + body + 4 CRC.
    let section_length = 5 + body.len() + 4;
    let mut s = Vec::with_capacity(3 + section_length);
    s.push(table_id);
    // section_syntax_indicator=1, private bit 0, reserved 11, length high.
    s.push(0xB0 | ((section_length >> 8) & 0x0F) as u8);
    s.push((section_length & 0xFF) as u8);
    s.extend_from_slice(&table_id_extension.to_be_bytes());
    // reserved 11, version 5 bits, current_next 1.
    s.push(0xC0 | ((version & 0x1F) << 1) | 1);
    s.push(0); // section_number
    s.push(0); // last_section_number
    s.extend_from_slice(body);
    let crc = crc32_mpeg2(&s);
    s.extend_from_slice(&crc.to_be_bytes());
    (s, section_length)
}

/// One descriptor loop entry: tag, length, payload.
pub(crate) fn descriptor(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut d = Vec::with_capacity(2 + payload.len());
    d.push(tag);
    d.push(payload.len().min(255) as u8);
    d.extend_from_slice(payload);
    d
}

/// A 13-bit PID with the three reserved bits set.
pub(crate) fn pid_field(pid: u16) -> [u8; 2] {
    (0xE000 | (pid & 0x1FFF)).to_be_bytes()
}

/// A 12-bit length with the four reserved bits set.
pub(crate) fn len12_field(len: usize) -> [u8; 2] {
    (0xF000 | (len as u16 & 0x0FFF)).to_be_bytes()
}
