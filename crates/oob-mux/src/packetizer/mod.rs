//! Layer 2 of 3: the packetizer (ADR-002 decision 3, r6 F6-12).
//!
//! Pull-driven: it holds at most one PDU in flight and turns it into
//! 188-byte transport packets one at a time, only as the pacer asks. Each
//! section starts a new packet (payload_unit_start_indicator set,
//! pointer_field 0) and its last packet is stuffed with 0xFF. Continuity
//! counters are kept per PID here.
//!
//! A time-bearing PDU is stamped when it starts, that is, when the pacer
//! is about to release its first packet (r6 F6-16): the stamper writes the
//! time field into the packetizer's working copy, then the CRC32 is
//! recomputed. The stamper is foreign code and runs under
//! `catch_unwind`; if it panics the PDU is dropped and nothing of it goes
//! out.
//!
//! Section-in-packet framing is generic MPEG-2 structure; for the OOB
//! stream it is the r4 lead and stays `hypothesized`. SCTE 55-1 coding,
//! interleaving and randomization are the flowgraph's, not this crate's
//! (r6 F6-2).

#![deny(clippy::unwrap_used, clippy::expect_used)]

mod cc;
pub(crate) mod section;

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::clock::UtcTime;
use crate::packet::{PAYLOAD_LEN, TsPacket};
use crate::pdu::{DropReason, MAX_SECTION_LEN, Pdu};
use crate::pid::Pid;
use cc::ContinuityCounters;
pub use section::packets_for;
use section::{FIRST_PACKET_BYTES, rewrite_crc};

/// Turns one PDU at a time into transport packets, on demand.
pub struct Packetizer {
    cc: ContinuityCounters,
    /// Working copy of the in-flight section; capacity reserved once.
    buf: Vec<u8>,
    /// Bytes of `buf` already packetized.
    off: usize,
    /// PID of the in-flight section, `None` when idle.
    pid: Option<Pid>,
}

impl core::fmt::Debug for Packetizer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Packetizer")
            .field("pid", &self.pid)
            .field("remaining", &self.buf.len().saturating_sub(self.off))
            .finish_non_exhaustive()
    }
}

impl Default for Packetizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Packetizer {
    /// An idle packetizer with its buffers allocated.
    pub fn new() -> Self {
        Self {
            cc: ContinuityCounters::new(),
            buf: Vec::with_capacity(MAX_SECTION_LEN),
            off: 0,
            pid: None,
        }
    }

    /// Whether a PDU is partly packetized.
    pub fn in_flight(&self) -> bool {
        self.pid.is_some()
    }

    /// Starts `pdu`, stamping it with `at` if it is time-bearing. Any PDU
    /// still in flight is abandoned (the pacer never does this).
    ///
    /// Returns [`DropReason::StamperPanicked`] if the stamper panicked; the
    /// packetizer is then idle and nothing of the PDU is emitted.
    pub fn start(&mut self, pdu: &Pdu, at: UtcTime) -> Result<(), DropReason> {
        self.pid = None;
        self.off = 0;
        self.buf.clear();
        // Validated at submit to fit MAX_SECTION_LEN, so this never grows
        // the buffer past its reserved capacity.
        self.buf.extend_from_slice(pdu.payload.as_bytes());
        if let Some(stamper) = &pdu.stamp {
            let buf = &mut self.buf;
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| stamper.stamp(buf, at))) {
                // The payload is foreign too: its Drop could panic again,
                // outside any guard. Leak it instead (QA-3).
                std::mem::forget(payload);
                self.buf.clear();
                return Err(DropReason::StamperPanicked);
            }
            rewrite_crc(&mut self.buf);
        }
        if !self.buf.is_empty() {
            self.pid = Some(pdu.pid);
        }
        Ok(())
    }

    /// The next packet of the in-flight PDU, or `None` when idle.
    pub fn next_packet(&mut self) -> Option<TsPacket> {
        let pid = self.pid?;
        let first = self.off == 0;
        let mut payload = [0xFFu8; PAYLOAD_LEN];
        let (dst, room) = if first {
            payload[0] = 0x00; // pointer_field: the section starts right here
            (&mut payload[1..], FIRST_PACKET_BYTES)
        } else {
            (&mut payload[..], PAYLOAD_LEN)
        };
        let end = self.buf.len().min(self.off + room);
        let chunk = self.buf.get(self.off..end)?;
        dst[..chunk.len()].copy_from_slice(chunk);
        self.off = end;
        if self.off >= self.buf.len() {
            self.pid = None;
        }
        let cc = self.cc.next(pid);
        Some(TsPacket::payload(pid, first, cc, &payload))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::Arc;

    use super::section::{crc32_mpeg2, packets_for};
    use super::*;
    use crate::pdu::{Payload, TimeStamper, TrafficClass};

    fn section(len: usize) -> Vec<u8> {
        let mut s: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let sl = (len - 3) as u16;
        s[1] = 0xB0 | (sl >> 8) as u8;
        s[2] = sl as u8;
        s
    }

    fn pdu(bytes: Vec<u8>) -> Pdu {
        Pdu {
            class: TrafficClass::C0System,
            priority: 0,
            pid: Pid::new(0x20).unwrap(),
            release_at: None,
            deadline: None,
            payload: Payload::Section(bytes.into()),
            stamp: None,
        }
    }

    fn drain(p: &mut Packetizer) -> Vec<TsPacket> {
        std::iter::from_fn(|| p.next_packet()).collect()
    }

    fn reassemble(pkts: &[TsPacket], len: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, p) in pkts.iter().enumerate() {
            let pl = p.payload_bytes();
            out.extend_from_slice(if i == 0 { &pl[1..] } else { pl });
        }
        assert!(out[len..].iter().all(|&b| b == 0xFF), "stuffing is 0xFF");
        out.truncate(len);
        out
    }

    #[test]
    fn sections_spanning_one_two_and_n_packets() {
        for len in [12, 183, 184, 367, 368, 1000, MAX_SECTION_LEN] {
            let bytes = section(len);
            let mut p = Packetizer::new();
            p.start(&pdu(bytes.clone()), UtcTime::default()).unwrap();
            let pkts = drain(&mut p);
            assert_eq!(pkts.len(), packets_for(len), "len {len}");
            assert!(pkts[0].pusi());
            assert_eq!(pkts[0].payload_bytes()[0], 0, "pointer_field");
            assert!(pkts[1..].iter().all(|k| !k.pusi()));
            assert!(pkts.iter().all(|k| k.pid_value() == 0x20 && !k.is_null()));
            assert_eq!(reassemble(&pkts, len), bytes, "len {len}");
            assert!(!p.in_flight());
        }
    }

    #[test]
    fn continuity_counter_runs_across_sections() {
        let mut p = Packetizer::new();
        let mut ccs = Vec::new();
        for _ in 0..20 {
            p.start(&pdu(section(12)), UtcTime::default()).unwrap();
            ccs.extend(drain(&mut p).iter().map(TsPacket::continuity_counter));
        }
        let want: Vec<u8> = (0..20).map(|i| (i % 16) as u8).collect();
        assert_eq!(ccs, want);
    }

    struct WriteSecs;
    impl TimeStamper for WriteSecs {
        fn stamp(&self, s: &mut [u8], at: UtcTime) {
            s[3..7].copy_from_slice(&(at.since_unix_epoch().as_secs() as u32).to_be_bytes());
        }
    }

    #[test]
    fn stamping_rewrites_the_crc() {
        let mut bytes = section(20);
        rewrite_crc(&mut bytes);
        let mut d = pdu(bytes);
        d.stamp = Some(Arc::new(WriteSecs));
        let mut p = Packetizer::new();
        let at = UtcTime::from_unix(std::time::Duration::from_secs(0x0102_0304));
        p.start(&d, at).unwrap();
        let out = reassemble(&drain(&mut p), 20);
        assert_eq!(&out[3..7], &[1, 2, 3, 4]);
        assert_eq!(crc32_mpeg2(&out), 0, "CRC32 valid after stamping");
    }

    struct Boom;
    impl TimeStamper for Boom {
        fn stamp(&self, _: &mut [u8], _: UtcTime) {
            panic!("stamper bug");
        }
    }

    #[test]
    fn a_panicking_stamper_drops_the_pdu() {
        let mut d = pdu(section(20));
        d.stamp = Some(Arc::new(Boom));
        let mut p = Packetizer::new();
        assert_eq!(
            p.start(&d, UtcTime::default()),
            Err(DropReason::StamperPanicked)
        );
        assert!(!p.in_flight());
        assert!(p.next_packet().is_none());
    }

    proptest::proptest! {
        /// F6-15: a section of any legal length is `packets_for(len)`
        /// packets; reassembly returns the original bytes; every
        /// payload-unit-start packet has pointer_field 0 and is the first
        /// of the section only; the last packet's tail is 0xFF.
        #[test]
        fn spans_packets_for_len_and_reassembles(len in 12usize..=MAX_SECTION_LEN) {
            let bytes = section(len);
            let mut p = Packetizer::new();
            p.start(&pdu(bytes.clone()), UtcTime::default()).unwrap();
            let pkts = drain(&mut p);
            proptest::prop_assert_eq!(pkts.len(), packets_for(len));
            proptest::prop_assert!(pkts[0].pusi());
            proptest::prop_assert_eq!(pkts[0].payload_bytes()[0], 0);
            proptest::prop_assert!(pkts[1..].iter().all(|k| !k.pusi()));
            proptest::prop_assert_eq!(reassemble(&pkts, len), bytes);
            proptest::prop_assert!(!p.in_flight());
        }

        /// Counters run +1 mod 16 per PID across any sequence of sections
        /// on interleaved PIDs; a section never disturbs another PID's count.
        #[test]
        fn counters_follow_packets_per_pid(
            seq in proptest::collection::vec((0usize..3, 12usize..800), 1..30)
        ) {
            const PIDS: [u16; 3] = [0x20, 0x21, 0x22];
            let mut p = Packetizer::new();
            let mut packets = [0usize; 3];
            for (i, len) in seq {
                let mut d = pdu(section(len));
                d.pid = Pid::new(PIDS[i]).unwrap();
                p.start(&d, UtcTime::default()).unwrap();
                for k in drain(&mut p) {
                    proptest::prop_assert_eq!(
                        usize::from(k.continuity_counter()),
                        packets[i] % 16
                    );
                    packets[i] += 1;
                }
            }
        }
    }
}
