//! Box-free stream verification (#18, r6 F6-15).
//!
//! Evidence class: structural. The golden vectors are regression locks and
//! the properties are invariants of the packetizer; neither says a box
//! accepts these bytes. The OOB framing is the r4 lead, `hypothesized`
//! (AGENTS.md §2): every vector header says so, and only a capture in
//! `lab/captures/` can promote it. The tsduck pass over `gen_stream` output
//! (CI job `tsduck analyze`) is the independent oracle for section syntax,
//! CRC and continuity; it cannot vouch for the DCII-flavored bits.
//!
//! Deterministic by construction: a [`oob_mux::ManualClock`] and fixed
//! submissions, no wall clock. Set `OOB_BLESS=1` to rewrite a vector after
//! a deliberate change.

mod common;

use std::collections::HashMap;

use common::golden;
use common::{one_packet, pid_of, pull, rig, roomy};
use oob_mux::packetizer::packets_for;
use oob_mux::pdu::MAX_SECTION_LEN;
use oob_mux::{Outcome, Payload, Pdu, Pid, Submission, TrafficClass, TsPacket};
use proptest::prelude::*;

const NULL_HEADER: [u8; 4] = [0x47, 0x1F, 0xFF, 0x10];

/// An independent bitwise CRC-32/MPEG-2 (poly 04C11DB7, init FFFFFFFF, no
/// reflection, no final XOR). It shares nothing with the crate's table
/// implementation.
fn crc32_bitwise(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// A private section (table_id 0x80, long form, section 0 of 0) of `len`
/// bytes: header, a counting body, valid CRC32.
fn section(len: usize, tag: u8) -> Vec<u8> {
    assert!((12..=MAX_SECTION_LEN).contains(&len));
    let mut s = vec![0u8; len];
    s[0] = 0x80;
    let sl = u16::try_from(len - 3).unwrap();
    s[1] = 0xB0 | (sl >> 8) as u8;
    s[2] = sl as u8;
    s[3] = tag; // table_id_extension, high byte
    s[5] = 0xC1; // version 0, current_next 1; section_number and last stay 0
    for (i, b) in s.iter_mut().enumerate().take(len - 4).skip(8) {
        *b = (i % 251) as u8;
    }
    let crc = crc32_bitwise(&s[..len - 4]).to_be_bytes();
    s[len - 4..].copy_from_slice(&crc);
    s
}

fn pdu(class: TrafficClass, pid: u16, bytes: Vec<u8>) -> Pdu {
    Pdu {
        class,
        priority: 0,
        pid: Pid::new(pid).unwrap(),
        release_at: None,
        deadline: None,
        payload: Payload::Section(bytes.into()),
        stamp: None,
    }
}

/// The packets one section becomes on `pid`, through the public pipeline.
/// Fresh pipeline, so continuity counters start at 0.
fn packets_of(pid: u16, bytes: &[u8]) -> Vec<TsPacket> {
    let mut r = rig(roomy(true));
    r.handle
        .submit(Submission::Once(pdu(
            TrafficClass::C0System,
            pid,
            bytes.to_vec(),
        )))
        .unwrap();
    let n = packets_for(bytes.len());
    let mut out = Vec::new();
    let mut left = n;
    while left > 0 {
        let k = left.min(64);
        out.extend(pull(&mut r.pacer, k));
        left -= k;
    }
    out
}

fn flat(pkts: &[TsPacket]) -> Vec<u8> {
    pkts.iter()
        .flat_map(|p| p.as_bytes().iter().copied())
        .collect()
}

// ---- golden vectors: the null packet and one packet per PID class ----

#[test]
fn golden_null_packet() {
    let mut r = rig(roomy(true));
    let nulls = pull(&mut r.pacer, 2);
    assert!(nulls.iter().all(TsPacket::is_null));
    assert_eq!(nulls[0], nulls[1], "the null packet carries no counter");
    golden::check("ts_null", nulls[0].as_bytes());
    // Independent of the vector file: ISO/IEC 13818-1 §2.4.3.2 fixes the
    // header bytes (sync, PID 0x1FFF, payload only) and the 0xFF fill.
    let b = nulls[0].as_bytes();
    assert_eq!(b[..4], NULL_HEADER);
    assert!(b[4..].iter().all(|&x| x == 0xFF));
}

fn one_of(name: &str, pid: u16, section: &[u8]) {
    let pkts = packets_of(pid, section);
    assert_eq!(pkts.len(), 1, "{name} is a one-packet section");
    golden::check(name, pkts[0].as_bytes());
}

#[test]
fn golden_pid_class_packets() {
    // PSI sections come from the #17 vectors, so the two files cannot drift.
    one_of("ts_pat_pid0", 0x0000, &golden::load("pat_1"));
    one_of("ts_cat_pid1", 0x0001, &golden::load("cat_1"));
    one_of("ts_pmt_pid", 0x0100, &golden::load("pmt_no_ca"));
    // An EMM-class PID (a CAT-referenced private PID) and a plain carousel PID.
    one_of("ts_emm_pid", 0x1D00, &section(40, 0xE1));
    one_of(
        "ts_private_pid",
        pid_of(TrafficClass::C3Guide),
        &section(60, 0xC3),
    );
}

#[test]
fn golden_pat_packet_matches_the_standard_by_hand() {
    // ISO/IEC 13818-1 §2.4.3.2/§2.4.4: sync 0x47; TEI 0, PUSI 1, priority 0,
    // PID 0 -> 0x40 0x00; scrambling 00, adaptation_field_control 01,
    // continuity_counter 0 -> 0x10; pointer_field 0; then the section.
    let pkts = packets_of(0, &golden::load("pat_1"));
    let b = pkts[0].as_bytes();
    assert_eq!(b[..5], [0x47, 0x40, 0x00, 0x10, 0x00]);
    assert_eq!(b[5..21], golden::load("pat_1")[..]);
    assert!(b[21..].iter().all(|&x| x == 0xFF));
}

// ---- golden vectors: section boundaries ----

#[test]
fn golden_section_boundaries() {
    let single = packets_of(0x1D01, &section(12, 0x01));
    assert_eq!(single.len(), 1);
    golden::check("ts_section_single", &flat(&single));

    // 183 bytes: pointer_field + 183 fills the 184-byte payload exactly, so
    // there is no stuffing and the section ends on the packet boundary.
    let exact_one = packets_of(0x1D02, &section(183, 0x02));
    assert_eq!(exact_one.len(), 1);
    assert_eq!(
        exact_one[0].payload_bytes()[183],
        *section(183, 0x02).last().unwrap()
    );
    golden::check("ts_section_exact_one", &flat(&exact_one));

    // 183 + 184: a second packet filled exactly, no stuffing either.
    let exact_two = packets_of(0x1D03, &section(183 + 184, 0x03));
    assert_eq!(exact_two.len(), 2);
    assert_eq!(
        exact_two[1].as_bytes().last(),
        section(183 + 184, 0x03).last()
    );
    golden::check("ts_section_exact_two", &flat(&exact_two));

    // 400 bytes: 183 + 184 + 33, the last packet stuffed with 0xFF.
    let multi = packets_of(0x1D04, &section(400, 0x04));
    assert_eq!(multi.len(), 3);
    assert!(multi[0].pusi() && !multi[1].pusi() && !multi[2].pusi());
    golden::check("ts_section_multi", &flat(&multi));
}

// ---- helpers for the stream properties ----

/// Per PID, the continuity counters must run 0, 1, 2, ... mod 16 in stream
/// order; the null PID is exempt.
fn assert_continuity(stream: &[TsPacket]) -> Result<(), TestCaseError> {
    let mut next: HashMap<u16, u8> = HashMap::new();
    for p in stream {
        if p.is_null() {
            prop_assert_eq!(&p.as_bytes()[..4], &NULL_HEADER);
            continue;
        }
        let want = next.entry(p.pid_value()).or_insert(0);
        prop_assert_eq!(p.continuity_counter(), *want, "pid {:#x}", p.pid_value());
        *want = (*want + 1) & 0x0F;
    }
    Ok(())
}

/// Per PID, the sections in order, parsed from the packet stream (each
/// section starts a packet with pointer_field 0; the rest is 0xFF).
fn reassemble(stream: &[TsPacket]) -> Result<HashMap<u16, Vec<Vec<u8>>>, TestCaseError> {
    let mut out: HashMap<u16, Vec<Vec<u8>>> = HashMap::new();
    let mut cur: HashMap<u16, (Vec<u8>, usize)> = HashMap::new();
    for p in stream.iter().filter(|p| !p.is_null()) {
        let pid = p.pid_value();
        let pl = p.payload_bytes();
        let body = if p.pusi() {
            prop_assert_eq!(pl[0], 0, "pointer_field on a PUSI packet");
            prop_assert!(
                !cur.contains_key(&pid),
                "a section never straddles into the next"
            );
            let sl = (usize::from(pl[2] & 0x0F) << 8) | usize::from(pl[3]);
            cur.insert(pid, (Vec::new(), sl + 3));
            &pl[1..]
        } else {
            prop_assert!(cur.contains_key(&pid), "continuation without a start");
            pl
        };
        let (buf, total) = cur.get_mut(&pid).unwrap();
        let take = body.len().min(*total - buf.len());
        buf.extend_from_slice(&body[..take]);
        prop_assert!(body[take..].iter().all(|&b| b == 0xFF), "stuffing is 0xFF");
        if buf.len() == *total {
            let (done, _) = cur.remove(&pid).unwrap();
            out.entry(pid).or_default().push(done);
        }
    }
    prop_assert!(cur.is_empty(), "no section left half-emitted");
    Ok(out)
}

fn class_of(i: usize) -> TrafficClass {
    TrafficClass::ALL[i % TrafficClass::ALL.len()]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Any interleaving of PDUs over six class PIDs, pulled in random chunk
    /// sizes: counters run +1 mod 16 per PID, independent PIDs never
    /// disturb each other, nulls are exempt, every PUSI packet has
    /// pointer_field 0, and each PID's sections come back byte-identical
    /// and in order.
    #[test]
    fn stream_invariants_hold_across_any_interleaving(
        pdus in prop::collection::vec((0usize..6, 12usize..=MAX_SECTION_LEN), 1..16),
        chunks in prop::collection::vec(1usize..=64, 1..8),
    ) {
        let mut r = rig(roomy(true));
        let mut sent: HashMap<u16, Vec<Vec<u8>>> = HashMap::new();
        for (i, &(c, len)) in pdus.iter().enumerate() {
            let class = class_of(c);
            let bytes = section(len, i as u8);
            sent.entry(pid_of(class)).or_default().push(bytes.clone());
            r.handle
                .submit(Submission::Once(pdu(class, pid_of(class), bytes)))
                .unwrap();
        }
        let mut stream = Vec::new();
        let mut emitted = 0;
        let mut k = 0;
        while emitted < pdus.len() {
            let n = chunks[k % chunks.len()];
            k += 1;
            let got = pull(&mut r.pacer, n);
            prop_assert_eq!(got.len(), n, "pull(n) returns exactly n");
            stream.extend(got);
            while let Ok(ev) = r.outcomes.try_recv() {
                let is_emitted = matches!(ev.outcome, Outcome::Emitted { .. });
                prop_assert!(is_emitted);
                emitted += 1;
            }
            prop_assert!(k < 20_000, "the stream never drained");
        }
        // Flush the last in-flight PDU (at most one section's packets).
        stream.extend(pull(&mut r.pacer, 64));
        stream.extend(pull(&mut r.pacer, 64));
        assert_continuity(&stream)?;
        let got = reassemble(&stream)?;
        prop_assert_eq!(got, sent);
    }

    /// `pull(n)` returns exactly `n` packets for every `n <= max_pull`,
    /// idle (all null) or busy.
    #[test]
    fn pull_returns_exactly_n(n in 0usize..=64, busy in any::<bool>()) {
        let mut r = rig(roomy(true));
        if busy {
            common::saturate(&r.handle, TrafficClass::C0System);
        }
        let got = pull(&mut r.pacer, n);
        prop_assert_eq!(got.len(), n);
        if !busy {
            prop_assert!(got.iter().all(TsPacket::is_null));
        }
    }

    /// A section of any legal length spans exactly `packets_for(len)`
    /// packets, and every PUSI packet carries pointer_field 0.
    #[test]
    fn section_spans_packets_for_len(len in 12usize..=MAX_SECTION_LEN) {
        let s = section(len, 7);
        let pkts = packets_of(0x1D05, &s);
        prop_assert_eq!(pkts.len(), packets_for(len));
        prop_assert!(pkts[0].pusi() && pkts[1..].iter().all(|p| !p.pusi()));
        prop_assert_eq!(pkts[0].payload_bytes()[0], 0);
        let got = reassemble(&pkts)?;
        prop_assert_eq!(&got[&0x1D05][0], &s);
    }
}

#[test]
fn a_one_packet_pdu_helper_still_builds() {
    // Guards the shared fixture the properties above lean on.
    let p = one_packet(TrafficClass::C0System, 0, 9);
    assert_eq!(p.payload.as_bytes().len(), 40);
}
