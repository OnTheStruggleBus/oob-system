//! Deterministic whole-stream generator for the box-free verification of
//! the OOB transport stream (#18, r6 F6-15).
//!
//! ```text
//! cargo run -p oob-mux --example gen_stream -- \
//!     --out /tmp/oob-gen.ts --packets 20000 --rate-bps 2048000
//! ```
//!
//! Options: `--out PATH` (default `oob-gen.ts`), `--packets N` (default
//! 20000, the fixed length), `--rate-bps B` (the stream rate, default
//! 2048000; `docs/facts.yaml` `oob_stream.stream_rate_mb_s` is its one
//! home and the CI script `ops/ci/tsduck-analyze.py` passes it in).
//!
//! The stream is a pure function of its arguments: a
//! [`oob_mux::ManualClock`] (no wall clock), fixed submissions and a fixed
//! xorshift seed. It carries the class mix of #16 through the real
//! scheduler, packetizer and pacer:
//!
//! - C0: the PAT, PMT and CAT carousels built by `oob_mux::psi`;
//! - C1: addressed one-shots on the EMM PID listed in the CAT;
//! - C2: emergency-alert one-shots, PID 0x101;
//! - C3: a guide carousel (multi-packet sections), PID 0x102;
//! - C4: a code-download carousel (a section at the size limit), PID 0x103;
//! - C5: lab one-shots (lab mode on), PID 0x104.
//!
//! Every PID is declared (PAT, PMT or CAT) so the analyzer's "unreferenced
//! PID" check has teeth. The C1-C5 payloads are private sections of
//! counting filler with valid CRC32s: **structure, not DCII content**.
//! Exit status: 0 on success. What the stream proves is stated in the
//! book (`protocol/oob-mux-pipeline.md`): section syntax, CRC and
//! continuity only, never box acceptance (AGENTS.md §2).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use oob_mux::pid_plan::{
    EmmProviderId, EmmStream, EsPlan, PidPlan, ProgramPlan, ValidatedPidPlan, validate,
};
use oob_mux::psi::dcii::CaSystem;
use oob_mux::psi::{TableVersions, build_all};
use oob_mux::scheduler::{BudgetConfig, ClassBudget, ClassMap};
use oob_mux::{
    CarouselId, Clock, ManualClock, MonoTime, Pacer, PacerConfig, PacketRate, Payload, Pdu, Pid,
    PullSource, Scheduler, SchedulerConfig, SchedulerHandle, Submission, SubmitError, TrafficClass,
    UtcTime,
};

/// A fixed-seed xorshift64; the generator's only source of "randomness".
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() % (hi - lo + 1) as u64) as usize
    }
}

/// Bitwise CRC-32/MPEG-2, independent of the crate's table version.
fn crc32(data: &[u8]) -> u32 {
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
/// bytes, 12 or more.
fn section(len: usize, tag: u8, rng: &mut Rng) -> Vec<u8> {
    let mut s = vec![0u8; len];
    s[0] = 0x80;
    let sl = (len - 3) as u16;
    s[1] = 0xB0 | (sl >> 8) as u8;
    s[2] = sl as u8;
    s[3] = tag; // table_id_extension, high byte
    s[5] = 0xC1; // version 0, current_next 1
    // s[4], s[6], s[7]: extension low byte, section_number 0, last_section_number 0
    for b in &mut s[8..len - 4] {
        *b = rng.next() as u8;
    }
    let crc = crc32(&s[..len - 4]).to_be_bytes();
    s[len - 4..].copy_from_slice(&crc);
    s
}

fn pid(v: u16) -> Pid {
    Pid::new(v).unwrap_or_else(|e| panic!("{e}"))
}

fn pdu(class: TrafficClass, p: u16, bytes: Vec<u8>) -> Pdu {
    Pdu {
        class,
        priority: 0,
        pid: pid(p),
        release_at: None,
        deadline: None,
        payload: Payload::Section(bytes.into()),
        stamp: None,
    }
}

const EMM_PID: u16 = 0x0200;
const C2_PID: u16 = 0x0101;
const C3_PID: u16 = 0x0102;
const C4_PID: u16 = 0x0103;
const C5_PID: u16 = 0x0104;

fn plan() -> ValidatedPidPlan {
    let es = |p| EsPlan {
        // stream_type 0x05: ISO/IEC 13818-1 private sections.
        stream_type: 0x05,
        pid: pid(p),
        ca: vec![],
    };
    let plan = PidPlan {
        transport_stream_id: 0x0007,
        programs: vec![ProgramPlan {
            program_number: 1,
            pmt_pid: pid(0x0100),
            pcr_pid: pid(C2_PID),
            ca: vec![],
            streams: vec![es(C2_PID), es(C3_PID), es(C4_PID), es(C5_PID)],
            virtual_channel: None,
        }],
        emm_streams: vec![EmmStream {
            system: CaSystem::GiDigiCipher,
            pid: pid(EMM_PID),
            provider_id: EmmProviderId::new(0).unwrap_or_else(|e| panic!("{e}")),
        }],
        psi_repetition: Duration::from_millis(100),
    };
    validate(&plan).unwrap_or_else(|v| panic!("{v:?}"))
}

/// A budget with every floor satisfiable and C2 and C5 capped; fixture
/// values for this generator, not a statement about any plant.
fn budget() -> BudgetConfig {
    let rows: [(u32, Option<u32>); 6] = [
        (200, Some(250)),
        (100, Some(150)),
        (0, Some(300)),
        (250, Some(300)),
        (0, Some(200)),
        (0, Some(100)),
    ];
    BudgetConfig {
        insertion_kbps: 1000,
        classes: ClassMap::from_fn(|c| ClassBudget {
            floor_kbps: rows[c.index()].0,
            cap_kbps: rows[c.index()].1,
            burst_packets: 8,
        }),
        lab_mode: true,
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn cyclic(h: &SchedulerHandle, id: u32, class: TrafficClass, p: u16, bytes: Vec<u8>, ms: u64) {
    h.submit(Submission::Cyclic {
        id: CarouselId(id),
        pdu: pdu(class, p, bytes),
        period: Duration::from_millis(ms),
    })
    .unwrap_or_else(|e| panic!("{e}"));
}

fn run(args: &[String]) -> Result<(), String> {
    let out = arg(args, "--out").unwrap_or_else(|| "oob-gen.ts".into());
    let packets: usize = arg(args, "--packets")
        .map_or(Ok(20_000), |v| v.parse())
        .map_err(|e| format!("--packets: {e}"))?;
    let rate_bps: u64 = arg(args, "--rate-bps")
        .map_or(Ok(2_048_000), |v| v.parse())
        .map_err(|e| format!("--rate-bps: {e}"))?;
    let rate =
        PacketRate::from_bits_per_second(rate_bps).ok_or("--rate-bps: not a usable stream rate")?;

    let clock = Arc::new(ManualClock::new(
        MonoTime::default(),
        UtcTime::from_unix(Duration::from_secs(1_000_000)),
    ));
    let (scheduler, handle, outcomes) = Scheduler::new(SchedulerConfig {
        queue_depth: 24,
        max_carousels: 16,
        outcome_capacity: 4096,
        budget: budget(),
    })
    .map_err(|e| format!("{e}"))?;
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    let mut pacer = Pacer::new(
        PacerConfig {
            packet_rate: rate,
            max_pull: 64,
            pull_budget: Duration::from_secs(1),
        },
        scheduler,
        dyn_clock,
    )
    .map_err(|e| format!("{e}"))?;

    let mut rng = Rng(0x006F_6F62_5F31_3821); // fixed seed
    for s in build_all(&plan(), &mut TableVersions::new()).submissions {
        handle.submit(s).map_err(|e| format!("{e}"))?;
    }
    cyclic(
        &handle,
        1,
        TrafficClass::C3Guide,
        C3_PID,
        section(1500, 0x31, &mut rng),
        40,
    );
    cyclic(
        &handle,
        2,
        TrafficClass::C4Cdl,
        C4_PID,
        section(4096, 0x41, &mut rng),
        100,
    );

    let mut file = BufWriter::new(File::create(&out).map_err(|e| format!("{out}: {e}"))?);
    let mut buf = Vec::with_capacity(64);
    let mut written = 0usize;
    let mut nulls = 0usize;
    let mut tag = 0u8;
    while written < packets {
        // A fixed submission schedule: a one-shot every 64 packets.
        if written.is_multiple_of(64) {
            tag = tag.wrapping_add(1);
            let (class, p, lo, hi) = match (written / 64) % 3 {
                0 => (TrafficClass::C1Addressed, EMM_PID, 20, 200),
                1 => (TrafficClass::C2Eas, C2_PID, 40, 700),
                _ => (TrafficClass::C5Lab, C5_PID, 12, 400),
            };
            let len = rng.range(lo, hi);
            match handle.submit(Submission::Once(pdu(class, p, section(len, tag, &mut rng)))) {
                Ok(_) | Err(SubmitError::QueueFull { .. }) => {}
                Err(e) => return Err(format!("{e}")),
            }
        }
        let n = 64.min(packets - written);
        buf.clear();
        pacer.pull(n, &mut buf).map_err(|e| format!("{e:?}"))?;
        for p in &buf {
            nulls += usize::from(p.is_null());
            file.write_all(p.as_bytes()).map_err(|e| e.to_string())?;
        }
        written += n;
        clock.advance(rate.stream_time(n as u64));
        while outcomes.try_recv().is_ok() {}
    }
    file.flush().map_err(|e| e.to_string())?;
    eprintln!("gen_stream: {written} packets ({nulls} null) -> {out} at {rate_bps} b/s");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gen_stream: {e}");
            ExitCode::FAILURE
        }
    }
}
