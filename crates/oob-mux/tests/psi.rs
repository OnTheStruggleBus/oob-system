//! Box-free conformance tests for the DCII-flavored PSI assemblers (#17).
//!
//! Evidence class: structural. Golden bytes catch regressions; the
//! independent decoder here (and tsduck in #18) checks structure. Nothing
//! here claims a box accepts these tables (AGENTS.md §2): every layout is
//! `hypothesized`.
//!
//! Set `OOB_BLESS=1` to rewrite the golden files after a deliberate change.

mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use oob_mux::pid_plan::{
    CaRef, EmmProviderId, EmmStream, EsPlan, PidPlan, PidPlanViolation, ProgramPlan, validate,
};
use oob_mux::psi::dcii::{CaSystem, GI_CA_SYSTEM_ID};
use oob_mux::psi::{TableKind, TableVersions, build_all, build_table, statuses};
use oob_mux::{Pid, Status, Submission};

fn pid(v: u16) -> Pid {
    Pid::new(v).unwrap()
}

fn es(stream_type: u8, p: u16) -> EsPlan {
    EsPlan {
        stream_type,
        pid: pid(p),
        ca: vec![],
    }
}

fn ca(ecm: u16) -> CaRef {
    CaRef {
        system: CaSystem::GiDigiCipher,
        ca_pid: pid(ecm),
        private: vec![],
    }
}

fn program(n: u16, pmt: u16, base: u16) -> ProgramPlan {
    ProgramPlan {
        program_number: n,
        pmt_pid: pid(pmt),
        pcr_pid: pid(base),
        ca: vec![],
        streams: vec![es(0x02, base), es(0x81, base + 1)],
        virtual_channel: None,
    }
}

fn emm(p: u16, prov: u16) -> EmmStream {
    EmmStream {
        system: CaSystem::GiDigiCipher,
        pid: pid(p),
        provider_id: EmmProviderId::new(prov).unwrap(),
    }
}

fn plan(programs: Vec<ProgramPlan>, emm_streams: Vec<EmmStream>) -> PidPlan {
    PidPlan {
        transport_stream_id: 0x0007,
        programs,
        emm_streams,
        psi_repetition: Duration::from_millis(100),
    }
}

// ---- an independent section decoder (does not share builder code) ----

#[derive(Debug)]
struct Decoded {
    table_id: u8,
    ext: u16,
    version: u8,
    current_next: bool,
    section_number: u8,
    last_section_number: u8,
    body: Vec<u8>,
}

fn crc32_mpeg2(data: &[u8]) -> u32 {
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

fn decode(s: &[u8]) -> Decoded {
    assert!(s.len() >= 12, "short section");
    assert_eq!(s[1] & 0x80, 0x80, "section_syntax_indicator");
    assert_eq!(s[1] & 0x40, 0, "private_indicator is 0 for PSI");
    assert_eq!(s[1] & 0x30, 0x30, "reserved bits");
    let section_length = (usize::from(s[1] & 0x0F) << 8) | usize::from(s[2]);
    assert_eq!(s.len(), 3 + section_length, "section_length matches bytes");
    assert!(section_length <= 1021);
    assert_eq!(s[5] & 0xC0, 0xC0, "reserved bits in version byte");
    // CRC over the whole section, including the CRC itself, is zero.
    assert_eq!(crc32_mpeg2(s), 0, "CRC32 residue");
    Decoded {
        table_id: s[0],
        ext: u16::from_be_bytes([s[3], s[4]]),
        version: (s[5] >> 1) & 0x1F,
        current_next: s[5] & 1 == 1,
        section_number: s[6],
        last_section_number: s[7],
        body: s[8..s.len() - 4].to_vec(),
    }
}

/// (tag, payload) pairs of a descriptor loop.
fn descriptors(mut b: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut v = vec![];
    while !b.is_empty() {
        let l = usize::from(b[1]);
        v.push((b[0], b[2..2 + l].to_vec()));
        b = &b[2 + l..];
    }
    v
}

fn parse_ca(d: &(u8, Vec<u8>)) -> (u16, u16, Vec<u8>) {
    assert_eq!(d.0, 0x09, "CA_descriptor tag");
    let p = &d.1;
    assert_eq!(p[2] & 0xE0, 0xE0, "reserved bits before CA_PID");
    (
        u16::from_be_bytes([p[0], p[1]]),
        u16::from_be_bytes([p[2] & 0x1F, p[3]]),
        p[4..].to_vec(),
    )
}

// ---- golden vectors ----

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn hex(b: &[u8]) -> String {
    let mut s = String::new();
    for (i, c) in b.chunks(16).enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(
            &c.iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    s.push('\n');
    s
}

fn golden(name: &str, bytes: &[u8]) {
    let path = golden_dir().join(format!("{name}.hex"));
    if std::env::var_os("OOB_BLESS").is_some() {
        std::fs::write(&path, hex(bytes)).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing golden {}: {e}", path.display()));
    let want: Vec<u8> = want
        .split_whitespace()
        .map(|h| u8::from_str_radix(h, 16).unwrap())
        .collect();
    assert_eq!(
        bytes,
        want.as_slice(),
        "golden {name} differs; hex now:\n{}",
        hex(bytes)
    );
}

fn build(p: &PidPlan, kind: TableKind) -> Vec<u8> {
    let v = validate(p).unwrap();
    build_table(&v, kind, &mut TableVersions::new()).to_vec()
}

fn pat_plan(n: u16) -> PidPlan {
    plan(
        (0..n)
            .map(|i| program(i + 1, 0x100 + i * 0x10, 0x101 + i * 0x10))
            .collect(),
        vec![],
    )
}

#[test]
fn golden_pat_one_program() {
    let b = build(&pat_plan(1), TableKind::Pat);
    let d = decode(&b);
    assert_eq!(
        (d.table_id, d.ext, d.version, d.current_next),
        (0x00, 7, 0, true)
    );
    assert_eq!((d.section_number, d.last_section_number), (0, 0));
    assert_eq!(d.body, [0x00, 0x01, 0xE1, 0x00]);
    golden("pat_1", &b);
}

#[test]
fn golden_pat_three_programs() {
    let b = build(&pat_plan(3), TableKind::Pat);
    let d = decode(&b);
    assert_eq!(d.body.len(), 12);
    let entries: Vec<(u16, u16)> = d
        .body
        .chunks(4)
        .map(|c| {
            (
                u16::from_be_bytes([c[0], c[1]]),
                u16::from_be_bytes([c[2] & 0x1F, c[3]]),
            )
        })
        .collect();
    assert_eq!(entries, [(1, 0x100), (2, 0x110), (3, 0x120)]);
    golden("pat_3", &b);
}

type Descs = Vec<(u8, Vec<u8>)>;

fn pmt_body_streams(body: &[u8]) -> (u16, Vec<(u8, u16, Descs)>, Descs) {
    let pcr = u16::from_be_bytes([body[0] & 0x1F, body[1]]);
    assert_eq!(body[0] & 0xE0, 0xE0);
    assert_eq!(body[2] & 0xF0, 0xF0);
    let pil = (usize::from(body[2] & 0x0F) << 8) | usize::from(body[3]);
    let prog = descriptors(&body[4..4 + pil]);
    let mut rest = &body[4 + pil..];
    let mut streams = vec![];
    while !rest.is_empty() {
        let il = (usize::from(rest[3] & 0x0F) << 8) | usize::from(rest[4]);
        assert_eq!(rest[3] & 0xF0, 0xF0);
        streams.push((
            rest[0],
            u16::from_be_bytes([rest[1] & 0x1F, rest[2]]),
            descriptors(&rest[5..5 + il]),
        ));
        rest = &rest[5 + il..];
    }
    (pcr, streams, prog)
}

#[test]
fn golden_pmt_no_ca() {
    let p = plan(vec![program(1, 0x100, 0x101)], vec![]);
    let b = build(&p, TableKind::Pmt { program_number: 1 });
    let d = decode(&b);
    assert_eq!((d.table_id, d.ext), (0x02, 1));
    let (pcr, streams, prog) = pmt_body_streams(&d.body);
    assert_eq!(pcr, 0x101);
    assert!(prog.is_empty());
    assert_eq!(
        streams.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>(),
        [(0x02, 0x101), (0x81, 0x102)]
    );
    assert!(streams.iter().all(|s| s.2.is_empty()));
    golden("pmt_no_ca", &b);
}

#[test]
fn golden_pmt_program_level_ca() {
    let mut pr = program(1, 0x100, 0x101);
    pr.ca = vec![ca(0x0300)];
    let b = build(
        &plan(vec![pr], vec![]),
        TableKind::Pmt { program_number: 1 },
    );
    let (_, _, prog) = pmt_body_streams(&decode(&b).body);
    assert_eq!(prog.len(), 1);
    assert_eq!(parse_ca(&prog[0]), (GI_CA_SYSTEM_ID, 0x0300, vec![]));
    golden("pmt_program_ca", &b);
}

#[test]
fn golden_pmt_es_level_ca() {
    let mut pr = program(1, 0x100, 0x101);
    pr.streams[0].ca = vec![CaRef {
        private: vec![0xAA, 0xBB],
        ..ca(0x0301)
    }];
    let b = build(
        &plan(vec![pr], vec![]),
        TableKind::Pmt { program_number: 1 },
    );
    let (_, streams, prog) = pmt_body_streams(&decode(&b).body);
    assert!(prog.is_empty());
    assert_eq!(
        parse_ca(&streams[0].2[0]),
        (GI_CA_SYSTEM_ID, 0x0301, vec![0xAA, 0xBB])
    );
    assert!(streams[1].2.is_empty());
    golden("pmt_es_ca", &b);
}

#[test]
fn golden_cat_one_emm() {
    let b = build(&plan(vec![], vec![emm(0x200, 0)]), TableKind::Cat);
    let d = decode(&b);
    assert_eq!((d.table_id, d.ext), (0x01, 0xFFFF));
    let ds = descriptors(&d.body);
    assert_eq!(parse_ca(&ds[0]), (GI_CA_SYSTEM_ID, 0x200, vec![0x00, 0x00]));
    golden("cat_1", &b);
}

#[test]
fn golden_cat_two_emm() {
    let b = build(
        &plan(vec![], vec![emm(0x200, 0), emm(0x201, 1)]),
        TableKind::Cat,
    );
    let ds = descriptors(&decode(&b).body);
    assert_eq!(ds.len(), 2);
    assert_eq!(parse_ca(&ds[1]), (GI_CA_SYSTEM_ID, 0x201, vec![0x00, 0x01]));
    golden("cat_2", &b);
}

// ---- validation ----

fn has(
    r: Result<impl Sized, Vec<PidPlanViolation>>,
    f: impl Fn(&PidPlanViolation) -> bool,
) -> bool {
    r.err().is_some_and(|v| v.iter().any(f))
}

#[test]
fn seed_default_validates() {
    assert!(validate(&PidPlan::seed_default()).is_ok());
}

#[test]
fn violation_duplicate_pid() {
    let p = plan(
        vec![program(1, 0x100, 0x101), program(2, 0x100, 0x111)],
        vec![],
    );
    assert!(has(validate(&p), |v| matches!(
        v,
        PidPlanViolation::DuplicatePid { .. }
    )));
}

#[test]
fn violation_ecm_collides_with_es() {
    let mut pr = program(1, 0x100, 0x101);
    pr.ca = vec![ca(0x102)];
    assert!(has(validate(&plan(vec![pr], vec![])), |v| matches!(
        v,
        PidPlanViolation::DuplicatePid { .. }
    )));
}

#[test]
fn ecm_pid_may_be_shared_across_services() {
    let mut a = program(1, 0x100, 0x101);
    let mut b = program(2, 0x110, 0x111);
    a.ca = vec![ca(0x300)];
    b.ca = vec![ca(0x300)];
    assert!(validate(&plan(vec![a, b], vec![])).is_ok());
}

fn reserved(p: &PidPlan) -> bool {
    has(validate(p), |v| {
        matches!(v, PidPlanViolation::ReservedPid { .. })
    })
}

#[test]
fn violation_reserved_pids() {
    for bad in [0x0000u16, 0x0001, 0x000F, 0x1FFB] {
        // PMT role.
        assert!(
            reserved(&plan(vec![program(1, bad, 0x101)], vec![])),
            "pmt {bad:#x}"
        );
        // ES role (PCR sits on the same ES).
        let mut pr = program(1, 0x100, 0x101);
        pr.streams[1].pid = pid(bad);
        assert!(reserved(&plan(vec![pr], vec![])), "es {bad:#x}");
        // PCR-only role.
        let mut pr = program(1, 0x100, 0x101);
        pr.pcr_pid = pid(bad);
        assert!(reserved(&plan(vec![pr], vec![])), "pcr {bad:#x}");
        // EMM role.
        assert!(reserved(&plan(vec![], vec![emm(bad, 0)])), "emm {bad:#x}");
        // ECM role.
        let mut pr = program(1, 0x100, 0x101);
        pr.ca = vec![ca(bad)];
        assert!(reserved(&plan(vec![pr], vec![])), "ecm {bad:#x}");
    }
    // 0x0010 is the first allocatable PID.
    assert!(validate(&plan(vec![program(1, 0x0010, 0x101)], vec![])).is_ok());
}

#[test]
fn all_violations_are_reported_together() {
    // A duplicate PID and an oversize PAT in one plan: both are reported.
    let mut p = plan(
        (0..260u16)
            .map(|i| program(i + 1, 0x100 + i, 0x1000 + i))
            .collect(),
        vec![],
    );
    p.programs[1].pmt_pid = p.programs[0].pmt_pid;
    let v = validate(&p).unwrap_err();
    assert!(
        v.iter()
            .any(|x| matches!(x, PidPlanViolation::DuplicatePid { .. }))
    );
    assert!(
        v.iter()
            .any(|x| matches!(x, PidPlanViolation::SectionTooLong { .. }))
    );
}

#[test]
fn version_snapshot_round_trips_and_keeps_numbering() {
    let mut v = TableVersions::new();
    let mut p = plan(vec![program(1, 0x100, 0x101)], vec![emm(0x200, 0)]);
    build_all(&validate(&p).unwrap(), &mut v);
    p.programs[0].streams = vec![es(0x02, 0x101)];
    let before = build_all(&validate(&p).unwrap(), &mut v);
    assert_eq!(v.version(TableKind::Pmt { program_number: 1 }), Some(1));

    let mut restored = TableVersions::from_parts(v.snapshot());
    assert_eq!(restored.snapshot(), v.snapshot());
    // Unchanged plan after a restart: identical bytes, versions kept.
    let after = build_all(&validate(&p).unwrap(), &mut restored);
    for (a, b) in before.submissions.iter().zip(&after.submissions) {
        let (Submission::Cyclic { pdu: x, .. }, Submission::Cyclic { pdu: y, .. }) = (a, b) else {
            panic!("not cyclic")
        };
        assert_eq!(x.payload.as_bytes(), y.payload.as_bytes());
    }
    // A change after a restart continues from the restored version, not 0.
    p.programs[0].streams = vec![es(0x02, 0x101), es(0x81, 0x102)];
    build_all(&validate(&p).unwrap(), &mut restored);
    assert_eq!(
        restored.version(TableKind::Pmt { program_number: 1 }),
        Some(2)
    );
}

#[test]
fn violation_program_numbers() {
    let p = plan(vec![program(0, 0x100, 0x101)], vec![]);
    assert!(has(validate(&p), |v| matches!(
        v,
        PidPlanViolation::ProgramNumberZero
    )));
    let p = plan(
        vec![program(1, 0x100, 0x101), program(1, 0x110, 0x111)],
        vec![],
    );
    assert!(has(validate(&p), |v| matches!(
        v,
        PidPlanViolation::DuplicateProgramNumber(1)
    )));
}

#[test]
fn emm_provider_id_is_guardrailed() {
    assert!(EmmProviderId::new(0).is_ok());
    assert!(EmmProviderId::new(1).is_ok());
    for bad in [2u16, 0x00FF, 0xFFFF] {
        assert_eq!(
            EmmProviderId::new(bad),
            Err(PidPlanViolation::EmmProviderId(bad))
        );
    }
}

#[test]
fn violation_section_too_long() {
    // 260 programs x 4 bytes overflows the 1021-byte PAT.
    let programs = (0..260u16)
        .map(|i| program(i + 1, 0x100 + i, 0x1000))
        .collect();
    let mut p = plan(programs, vec![]);
    for (i, pr) in p.programs.iter_mut().enumerate() {
        pr.streams = vec![es(0x02, 0x1000 + i as u16)];
        pr.pcr_pid = pid(0x1000 + i as u16);
    }
    assert!(has(validate(&p), |v| matches!(
        v,
        PidPlanViolation::SectionTooLong {
            table: TableKind::Pat,
            ..
        }
    )));
}

#[test]
fn violation_ca_private_and_period() {
    let mut pr = program(1, 0x100, 0x101);
    pr.ca = vec![CaRef {
        private: vec![0; 252],
        ..ca(0x300)
    }];
    assert!(has(validate(&plan(vec![pr], vec![])), |v| matches!(
        v,
        PidPlanViolation::CaPrivateTooLong { len: 252 }
    )));
    let mut p = pat_plan(1);
    p.psi_repetition = Duration::ZERO;
    assert!(has(validate(&p), |v| matches!(
        v,
        PidPlanViolation::ZeroRepetition
    )));
}

// ---- versioning ----

#[test]
fn version_bumps_only_the_changed_table_and_wraps() {
    let mut v = TableVersions::new();
    let mut p = plan(
        vec![program(1, 0x100, 0x101), program(2, 0x110, 0x111)],
        vec![emm(0x200, 0)],
    );
    let vp = validate(&p).unwrap();
    let first = build_all(&vp, &mut v);
    assert_eq!(first.submissions.len(), 4); // PAT, CAT, 2 PMTs
    let again = build_all(&vp, &mut v);
    for (a, b) in first.submissions.iter().zip(&again.submissions) {
        let (Submission::Cyclic { pdu: x, .. }, Submission::Cyclic { pdu: y, .. }) = (a, b) else {
            panic!("not cyclic")
        };
        assert_eq!(
            x.payload.as_bytes(),
            y.payload.as_bytes(),
            "unchanged plan, unchanged bytes"
        );
    }
    let pmt2 = TableKind::Pmt { program_number: 2 };
    for k in [
        TableKind::Pat,
        TableKind::Cat,
        TableKind::Pmt { program_number: 1 },
        pmt2,
    ] {
        assert_eq!(v.version(k), Some(0));
    }
    // Change only program 2's stream list, 33 times: 31 -> 0 wrap.
    for i in 1..=33u16 {
        p.programs[1].streams = vec![es(0x02, 0x111), es(0x81, 0x112 + (i % 2))];
        build_all(&validate(&p).unwrap(), &mut v);
    }
    assert_eq!(v.version(pmt2), Some(33 % 32));
    assert_eq!(v.version(TableKind::Pat), Some(0));
    assert_eq!(v.version(TableKind::Pmt { program_number: 1 }), Some(0));
}

#[test]
fn removed_program_is_withdrawn() {
    let mut v = TableVersions::new();
    let p = plan(
        vec![program(1, 0x100, 0x101), program(2, 0x110, 0x111)],
        vec![],
    );
    build_all(&validate(&p).unwrap(), &mut v);
    let p1 = plan(vec![program(1, 0x100, 0x101)], vec![]);
    let set = build_all(&validate(&p1).unwrap(), &mut v);
    assert_eq!(
        set.withdrawn,
        [TableKind::Pmt { program_number: 2 }.carousel_id()]
    );
}

// ---- statuses ----

#[test]
fn no_row_is_verified() {
    assert!(!statuses().is_empty());
    assert!(statuses().iter().all(|r| r.status != Status::Verified));
    assert_eq!(CaSystem::GiDigiCipher.status(), Status::Hypothesized);
}

// ---- through the scheduler, packetizer and pacer ----

#[test]
fn psi_through_the_pipeline_with_continuity_across_wrap() {
    let mut rig = common::rig(common::roomy(false));
    let p = plan(vec![program(1, 0x100, 0x101)], vec![emm(0x200, 0)]);
    let vp = validate(&p).unwrap();
    let set = build_all(&vp, &mut TableVersions::new());
    for s in set.submissions {
        rig.handle.submit(s).unwrap();
    }
    // 100 ms period at 2.048 Mb/s is ~1089 slots; run long enough for the
    // PAT to repeat more than 16 times.
    let mut by_pid: HashMap<u16, Vec<u8>> = HashMap::new();
    let mut sections: HashMap<u16, Vec<Vec<u8>>> = HashMap::new();
    for _ in 0..40 {
        rig.clock.advance(Duration::from_millis(100));
        let mut pulled = Vec::new();
        for _ in 0..18 {
            pulled.extend(common::pull(&mut rig.pacer, 64));
        }
        for pk in pulled {
            if pk.is_null() {
                continue;
            }
            by_pid
                .entry(pk.pid_value())
                .or_default()
                .push(pk.continuity_counter());
            assert!(pk.pusi(), "each PSI section fits one packet here");
            let pl = pk.payload_bytes();
            assert_eq!(pl[0], 0, "pointer_field");
            let len = 3 + (usize::from(pl[2] & 0x0F) << 8 | usize::from(pl[3]));
            sections
                .entry(pk.pid_value())
                .or_default()
                .push(pl[1..1 + len].to_vec());
        }
    }
    let pat_ccs = &by_pid[&0x0000];
    assert!(
        pat_ccs.len() >= 17,
        "PAT repeats past a CC wrap: {}",
        pat_ccs.len()
    );
    for (i, w) in pat_ccs.windows(2).enumerate() {
        assert_eq!(w[1], (w[0] + 1) % 16, "PAT CC discontinuity at {i}");
    }
    // Independent per PID: each PID's own counter runs 0,1,2...
    for (pid, ccs) in &by_pid {
        assert_eq!(ccs[0], 0, "PID {pid:#x} starts at CC 0");
        assert!(
            ccs.windows(2).all(|w| w[1] == (w[0] + 1) % 16),
            "PID {pid:#x}"
        );
    }
    // Every emitted section decodes (CRC residue zero) and equals the build.
    for pid in [0x0000u16, 0x0001, 0x0100] {
        let s = &sections[&pid];
        assert!(s.iter().all(|x| x == &s[0]));
        decode(&s[0]);
    }
}
