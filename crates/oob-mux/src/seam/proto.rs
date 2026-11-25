//! Seam protocol v1: pure encode/decode of the request and response
//! header frames. No I/O; the endpoint, the reference puller and the tests
//! share it (ADR-002 decision 2, r6 F6-2, issue #15 design note §2).
//!
//! This is Oob's own protocol between the flowgraph and the daemon, not a
//! box wire format, so it carries no `hypothesized`/`verified` status. The
//! one exception is the packet size it carries, 188: the MPEG-2 transport
//! framing of the OOB stream is the r4 lead and stays `hypothesized` until
//! a capture verifies it, which is why the size is on the wire and a
//! surprise is a version bump rather than a silent break.
//!
//! All integers are little-endian.

use core::fmt;

use crate::packet::PACKET_LEN;

/// The request frame length in bytes.
pub const REQUEST_LEN: usize = 24;
/// The response header length in bytes; `n × packet_size` body bytes follow.
pub const RESPONSE_HEADER_LEN: usize = 16;
/// The protocol version this module speaks.
pub const VERSION: u16 = 1;
/// Request magic.
pub const REQUEST_MAGIC: [u8; 4] = *b"OOBP";
/// Response magic.
pub const RESPONSE_MAGIC: [u8; 4] = *b"OOBR";
/// Response status: the body holds exactly `n` packets.
pub const STATUS_OK: u16 = 0;
/// Response status: the request was refused; no body, and the endpoint
/// closes the connection.
pub const STATUS_BAD_REQUEST: u16 = 1;
/// The v1 packet size (`hypothesized`, see the module docs).
pub const PACKET_SIZE_V1: u16 = PACKET_LEN as u16;

/// Why a frame was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProtoError {
    /// The magic bytes are wrong.
    BadMagic([u8; 4]),
    /// The version is not [`VERSION`].
    BadVersion(u16),
    /// The reserved flags field is not zero.
    BadFlags(u16),
    /// A request asked for zero packets.
    ZeroPackets,
    /// A request asked for more than `max_pull` packets.
    TooManyPackets {
        /// Packets asked for.
        n: u16,
        /// The endpoint's `max_pull`.
        max: u16,
    },
    /// The request's packet size is not the endpoint's.
    BadPacketSize {
        /// The size the request carried.
        got: u16,
        /// The size the endpoint serves.
        want: u16,
    },
    /// A response status other than [`STATUS_OK`] or [`STATUS_BAD_REQUEST`].
    BadStatus(u16),
    /// A response claimed more pacer nulls than packets.
    NullsExceedPackets {
        /// `pacer_nulls` as carried.
        nulls: u16,
        /// `n` as carried.
        n: u16,
    },
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic(m) => write!(f, "bad magic {m:02X?}"),
            Self::BadVersion(v) => write!(f, "unsupported version {v}"),
            Self::BadFlags(v) => write!(f, "reserved flags are 0x{v:04X}, not zero"),
            Self::ZeroPackets => write!(f, "request for zero packets"),
            Self::TooManyPackets { n, max } => {
                write!(f, "request for {n} packets exceeds max_pull {max}")
            }
            Self::BadPacketSize { got, want } => {
                write!(f, "packet size {got}, endpoint serves {want}")
            }
            Self::BadStatus(s) => write!(f, "unknown status {s}"),
            Self::NullsExceedPackets { nulls, n } => {
                write!(f, "{nulls} pacer nulls in {n} packets")
            }
        }
    }
}

impl std::error::Error for ProtoError {}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// One pull request, flowgraph → endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// Increments per request, wraps.
    pub seq: u32,
    /// Packets requested.
    pub n: u16,
    /// Bytes per packet; 188 in v1.
    pub packet_size: u16,
    /// The flowgraph's cumulative count of requests it null-filled itself.
    pub puller_underruns: u32,
    /// The flowgraph's cumulative count of packets it null-filled itself.
    pub puller_nulls: u32,
}

impl Request {
    /// The 24-byte wire form.
    pub fn encode(&self) -> [u8; REQUEST_LEN] {
        let mut b = [0u8; REQUEST_LEN];
        b[0..4].copy_from_slice(&REQUEST_MAGIC);
        b[4..6].copy_from_slice(&VERSION.to_le_bytes());
        // b[6..8]: flags, reserved, zero
        b[8..12].copy_from_slice(&self.seq.to_le_bytes());
        b[12..14].copy_from_slice(&self.n.to_le_bytes());
        b[14..16].copy_from_slice(&self.packet_size.to_le_bytes());
        b[16..20].copy_from_slice(&self.puller_underruns.to_le_bytes());
        b[20..24].copy_from_slice(&self.puller_nulls.to_le_bytes());
        b
    }

    /// Parses the wire form. Checks structure only (magic, version,
    /// reserved flags, `n ≥ 1`); [`Request::check`] holds a request to the
    /// endpoint's limits.
    ///
    /// # Errors
    ///
    /// [`ProtoError`] for a bad magic, version or flags field, or `n = 0`.
    pub fn decode(b: &[u8; REQUEST_LEN]) -> Result<Self, ProtoError> {
        let magic = [b[0], b[1], b[2], b[3]];
        if magic != REQUEST_MAGIC {
            return Err(ProtoError::BadMagic(magic));
        }
        let version = u16_at(b, 4);
        if version != VERSION {
            return Err(ProtoError::BadVersion(version));
        }
        let flags = u16_at(b, 6);
        if flags != 0 {
            return Err(ProtoError::BadFlags(flags));
        }
        let n = u16_at(b, 12);
        if n == 0 {
            return Err(ProtoError::ZeroPackets);
        }
        Ok(Self {
            seq: u32_at(b, 8),
            n,
            packet_size: u16_at(b, 14),
            puller_underruns: u32_at(b, 16),
            puller_nulls: u32_at(b, 20),
        })
    }

    /// Holds the request to the endpoint's limits.
    ///
    /// # Errors
    ///
    /// [`ProtoError::TooManyPackets`] for `n > max_pull`;
    /// [`ProtoError::BadPacketSize`] for a packet size other than
    /// `packet_size`.
    pub fn check(&self, max_pull: u16, packet_size: u16) -> Result<(), ProtoError> {
        if self.n > max_pull {
            return Err(ProtoError::TooManyPackets {
                n: self.n,
                max: max_pull,
            });
        }
        if self.packet_size != packet_size {
            return Err(ProtoError::BadPacketSize {
                got: self.packet_size,
                want: packet_size,
            });
        }
        Ok(())
    }
}

/// The response header, endpoint → flowgraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseHeader {
    /// [`STATUS_OK`] or [`STATUS_BAD_REQUEST`].
    pub status: u16,
    /// Echo of the request's seq (0 when the request could not be parsed).
    pub seq: u32,
    /// Packets in the body: the requested `n` when OK, 0 otherwise.
    pub n: u16,
    /// How many of the `n` packets are pacer null fill.
    pub pacer_nulls: u16,
}

impl ResponseHeader {
    /// The 16-byte wire form.
    pub fn encode(&self) -> [u8; RESPONSE_HEADER_LEN] {
        let mut b = [0u8; RESPONSE_HEADER_LEN];
        b[0..4].copy_from_slice(&RESPONSE_MAGIC);
        b[4..6].copy_from_slice(&VERSION.to_le_bytes());
        b[6..8].copy_from_slice(&self.status.to_le_bytes());
        b[8..12].copy_from_slice(&self.seq.to_le_bytes());
        b[12..14].copy_from_slice(&self.n.to_le_bytes());
        b[14..16].copy_from_slice(&self.pacer_nulls.to_le_bytes());
        b
    }

    /// Parses the wire form.
    ///
    /// # Errors
    ///
    /// [`ProtoError`] for a bad magic, version or status, or more pacer
    /// nulls than packets.
    pub fn decode(b: &[u8; RESPONSE_HEADER_LEN]) -> Result<Self, ProtoError> {
        let magic = [b[0], b[1], b[2], b[3]];
        if magic != RESPONSE_MAGIC {
            return Err(ProtoError::BadMagic(magic));
        }
        let version = u16_at(b, 4);
        if version != VERSION {
            return Err(ProtoError::BadVersion(version));
        }
        let status = u16_at(b, 6);
        if status != STATUS_OK && status != STATUS_BAD_REQUEST {
            return Err(ProtoError::BadStatus(status));
        }
        let (n, nulls) = (u16_at(b, 12), u16_at(b, 14));
        if nulls > n {
            return Err(ProtoError::NullsExceedPackets { nulls, n });
        }
        Ok(Self {
            status,
            seq: u32_at(b, 8),
            n,
            pacer_nulls: nulls,
        })
    }

    /// Body bytes that follow this header for `packet_size`-byte packets.
    pub fn body_len(&self, packet_size: u16) -> usize {
        usize::from(self.n) * usize::from(packet_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny xorshift so the round-trip property needs no dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn request_round_trips_for_random_fields() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..10_000 {
            let r = rng.next();
            let q = Request {
                seq: r as u32,
                n: ((r >> 32) as u16).max(1),
                packet_size: (r >> 48) as u16,
                puller_underruns: rng.next() as u32,
                puller_nulls: rng.next() as u32,
            };
            assert_eq!(Request::decode(&q.encode()), Ok(q));
        }
    }

    #[test]
    fn response_round_trips_for_random_fields() {
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        for _ in 0..10_000 {
            let r = rng.next();
            let n = (r >> 16) as u16;
            let h = ResponseHeader {
                status: if r & 1 == 0 {
                    STATUS_OK
                } else {
                    STATUS_BAD_REQUEST
                },
                seq: r as u32,
                n,
                pacer_nulls: if n == 0 { 0 } else { ((r >> 32) as u16) % n },
            };
            assert_eq!(ResponseHeader::decode(&h.encode()), Ok(h));
        }
    }

    #[test]
    fn request_layout_is_the_documented_one() {
        let b = Request {
            seq: 0x0403_0201,
            n: 0x0605,
            packet_size: 188,
            puller_underruns: 7,
            puller_nulls: 9,
        }
        .encode();
        assert_eq!(&b[0..4], b"OOBP");
        assert_eq!(&b[4..6], &[1, 0]);
        assert_eq!(&b[6..8], &[0, 0]);
        assert_eq!(&b[8..12], &[1, 2, 3, 4]);
        assert_eq!(&b[12..14], &[5, 6]);
        assert_eq!(&b[14..16], &[188, 0]);
        assert_eq!(&b[16..20], &[7, 0, 0, 0]);
        assert_eq!(&b[20..24], &[9, 0, 0, 0]);
    }

    #[test]
    fn response_layout_is_the_documented_one() {
        let b = ResponseHeader {
            status: 0,
            seq: 0x0403_0201,
            n: 0x0605,
            pacer_nulls: 0x0201,
        }
        .encode();
        assert_eq!(&b[0..4], b"OOBR");
        assert_eq!(&b[4..6], &[1, 0]);
        assert_eq!(&b[6..8], &[0, 0]);
        assert_eq!(&b[8..12], &[1, 2, 3, 4]);
        assert_eq!(&b[12..14], &[5, 6]);
        assert_eq!(&b[14..16], &[1, 2]);
    }

    fn valid() -> [u8; REQUEST_LEN] {
        Request {
            seq: 1,
            n: 4,
            packet_size: 188,
            puller_underruns: 0,
            puller_nulls: 0,
        }
        .encode()
    }

    #[test]
    fn request_rejects_bad_magic_version_flags_and_zero_n() {
        let mut b = valid();
        b[0] = b'X';
        assert!(matches!(Request::decode(&b), Err(ProtoError::BadMagic(_))));
        let mut b = valid();
        b[4] = 2;
        assert_eq!(Request::decode(&b), Err(ProtoError::BadVersion(2)));
        let mut b = valid();
        b[6] = 1;
        assert_eq!(Request::decode(&b), Err(ProtoError::BadFlags(1)));
        let mut b = valid();
        b[12] = 0;
        assert_eq!(Request::decode(&b), Err(ProtoError::ZeroPackets));
    }

    #[test]
    fn check_rejects_oversize_and_wrong_packet_size() {
        let q = Request::decode(&valid()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(q.check(4, 188), Ok(()));
        assert_eq!(
            q.check(3, 188),
            Err(ProtoError::TooManyPackets { n: 4, max: 3 })
        );
        assert_eq!(
            q.check(4, 204),
            Err(ProtoError::BadPacketSize {
                got: 188,
                want: 204
            })
        );
    }

    #[test]
    fn response_rejects_bad_magic_version_status_and_excess_nulls() {
        let ok = ResponseHeader {
            status: 0,
            seq: 1,
            n: 2,
            pacer_nulls: 1,
        }
        .encode();
        let mut b = ok;
        b[3] = b'X';
        assert!(matches!(
            ResponseHeader::decode(&b),
            Err(ProtoError::BadMagic(_))
        ));
        let mut b = ok;
        b[4] = 9;
        assert_eq!(ResponseHeader::decode(&b), Err(ProtoError::BadVersion(9)));
        let mut b = ok;
        b[6] = 7;
        assert_eq!(ResponseHeader::decode(&b), Err(ProtoError::BadStatus(7)));
        let mut b = ok;
        b[14] = 3;
        assert_eq!(
            ResponseHeader::decode(&b),
            Err(ProtoError::NullsExceedPackets { nulls: 3, n: 2 })
        );
    }
}
