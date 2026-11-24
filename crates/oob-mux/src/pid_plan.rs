//! The PID plan: runtime operator configuration (ADR-003 §2 Layer 1).
//!
//! A [`PidPlan`] is an *input value*. It says which PMT, PCR, elementary
//! and EMM PIDs a stream uses, which program numbers exist and which CA
//! system each service references. oob-dac keeps it in its store and
//! validates it on save with [`validate`]; the assemblers only accept the
//! resulting [`ValidatedPidPlan`], so an invalid plan cannot reach the
//! wire. No PID is hard-coded here except the fixed MPEG-2 ones (PAT 0x0000,
//! CAT 0x0001) and the reserved set, which are code (Layer 3).
//!
//! What a plan may contain is bounded by the DCII guardrails: the EMM
//! provider ID is 0x0000 or 0x0001 and nothing else (AGENTS.md §8, a wrong
//! value forces boxes to re-hunt), and PID 0x1FFB (8187, the DCII master
//! guide table PID, r5 F5-6) is reserved. The 8187 reservation is
//! `hypothesized` (single-source); it is enforced anyway because a false
//! positive costs nothing.

use std::collections::HashMap;
use std::time::Duration;

use crate::pid::Pid;
use crate::psi::dcii::CaSystem;
use crate::psi::{TableKind, build_one_for_validation};

/// PID of the PAT (fixed by MPEG-2 Systems).
pub const PAT_PID: Pid = match Pid::new(0x0000) {
    Ok(p) => p,
    Err(_) => unreachable!(),
};
/// PID of the CAT (fixed by MPEG-2 Systems).
pub const CAT_PID: Pid = match Pid::new(0x0001) {
    Ok(p) => p,
    Err(_) => unreachable!(),
};
/// PID 8187, reserved for the DCII master guide table (r5 F5-6,
/// `hypothesized`). Not built here; just never allocatable.
pub const MGT_PID_VALUE: u16 = 0x1FFB;
/// The longest section_length PSI sections are allowed (1021, the MPEG-2
/// limit for PAT/PMT/CAT).
pub const MAX_PSI_SECTION_LENGTH: usize = 1021;

/// The EMM provider ID on the OOB stream: 0x0000 or 0x0001 only
/// (AGENTS.md §8). Any other value is unconstructible.
///
/// ```
/// use oob_mux::pid_plan::EmmProviderId;
/// assert!(EmmProviderId::new(0x0001).is_ok());
/// assert!(EmmProviderId::new(0x0002).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EmmProviderId(u16);

impl EmmProviderId {
    /// Accepts 0x0000 and 0x0001; rejects everything else.
    pub const fn new(value: u16) -> Result<Self, PidPlanViolation> {
        if value <= 1 {
            Ok(Self(value))
        } else {
            Err(PidPlanViolation::EmmProviderId(value))
        }
    }

    /// The raw provider ID.
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// A reference from a program or stream to a CA system's ECM/EMM PID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaRef {
    /// Which CA system (its ID is code, with a status).
    pub system: CaSystem,
    /// The PID carrying that system's ECMs for this service.
    pub ca_pid: Pid,
    /// CA descriptor private bytes (`hypothesized`; empty by default).
    pub private: Vec<u8>,
}

/// One elementary stream of a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EsPlan {
    /// MPEG-2 stream_type.
    pub stream_type: u8,
    /// The stream's PID.
    pub pid: Pid,
    /// ES-level CA descriptors.
    pub ca: Vec<CaRef>,
}

/// One program (service).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramPlan {
    /// program_number; never 0 (0 means the network PID in the PAT).
    pub program_number: u16,
    /// The PMT's PID.
    pub pmt_pid: Pid,
    /// The PCR PID: an ES of this program or a PID of its own.
    pub pcr_pid: Pid,
    /// Program-level CA descriptors.
    pub ca: Vec<CaRef>,
    /// Elementary streams.
    pub streams: Vec<EsPlan>,
    /// DCII virtual channel number (F5-6): carried, not yet emitted.
    pub virtual_channel: Option<u16>,
}

/// An EMM stream, listed in the CAT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmmStream {
    /// Which CA system the EMMs belong to.
    pub system: CaSystem,
    /// The EMM PID.
    pub pid: Pid,
    /// Provider ID, 0x0000/0x0001.
    pub provider_id: EmmProviderId,
}

/// The operator's PID plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidPlan {
    /// transport_stream_id of the PAT.
    pub transport_stream_id: u16,
    /// Programs listed in the PAT, one PMT each.
    pub programs: Vec<ProgramPlan>,
    /// EMM streams listed in the CAT.
    pub emm_streams: Vec<EmmStream>,
    /// Carousel period for PAT, PMT and CAT.
    pub psi_repetition: Duration,
}

impl PidPlan {
    /// A seed default for new installs and tests: one program with a
    /// video and an audio stream and one EMM stream. Values are example
    /// data, not a claim about any real plant (ADR-003).
    pub fn seed_default() -> Self {
        let p = |v| Pid::new(v).unwrap_or(PAT_PID);
        Self {
            transport_stream_id: 1,
            programs: vec![ProgramPlan {
                program_number: 1,
                pmt_pid: p(0x0100),
                pcr_pid: p(0x0101),
                ca: Vec::new(),
                streams: vec![
                    EsPlan {
                        stream_type: 0x02,
                        pid: p(0x0101),
                        ca: Vec::new(),
                    },
                    EsPlan {
                        stream_type: 0x81,
                        pid: p(0x0102),
                        ca: Vec::new(),
                    },
                ],
                virtual_channel: None,
            }],
            emm_streams: vec![EmmStream {
                system: CaSystem::GiDigiCipher,
                pid: p(0x0200),
                provider_id: EmmProviderId(0),
            }],
            psi_repetition: Duration::from_millis(100),
        }
    }
}

/// A plan that passed [`validate`]. Only [`crate::psi::build_all`] and
/// friends accept this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPidPlan(PidPlan);

impl ValidatedPidPlan {
    /// The validated plan.
    pub fn plan(&self) -> &PidPlan {
        &self.0
    }
}

/// Why a plan is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PidPlanViolation {
    /// Two roles want the same PID.
    DuplicatePid {
        /// The contested PID.
        pid: Pid,
        /// The roles that claim it.
        uses: Vec<&'static str>,
    },
    /// A fixed or reserved PID: 0x0000–0x000F or 0x1FFB.
    ReservedPid {
        /// The PID.
        pid: Pid,
        /// Why it is reserved.
        why: &'static str,
    },
    /// Two programs share a program_number.
    DuplicateProgramNumber(u16),
    /// program_number 0.
    ProgramNumberZero,
    /// An EMM provider ID other than 0x0000/0x0001.
    EmmProviderId(u16),
    /// A built section would exceed section_length 1021.
    SectionTooLong {
        /// Which table.
        table: TableKind,
        /// The section_length it would need.
        len: usize,
    },
    /// CA descriptor private data over 251 bytes (descriptor_length is
    /// one byte and 4 are the system ID and PID).
    CaPrivateTooLong {
        /// The offending length.
        len: usize,
    },
    /// A zero carousel period.
    ZeroRepetition,
}

impl core::fmt::Display for PidPlanViolation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicatePid { pid, uses } => {
                write!(f, "PID {pid} claimed by {}", uses.join(" and "))
            }
            Self::ReservedPid { pid, why } => write!(f, "PID {pid} is reserved: {why}"),
            Self::DuplicateProgramNumber(n) => write!(f, "program_number {n} used twice"),
            Self::ProgramNumberZero => f.write_str("program_number 0 is the network PID entry"),
            Self::EmmProviderId(v) => {
                write!(
                    f,
                    "EMM provider ID 0x{v:04X}: only 0x0000/0x0001 are allowed"
                )
            }
            Self::SectionTooLong { table, len } => {
                write!(
                    f,
                    "{table:?} needs section_length {len} (> {MAX_PSI_SECTION_LENGTH})"
                )
            }
            Self::CaPrivateTooLong { len } => {
                write!(f, "CA descriptor private data {len} > 251 bytes")
            }
            Self::ZeroRepetition => f.write_str("psi_repetition must be non-zero"),
        }
    }
}

impl std::error::Error for PidPlanViolation {}

fn reserved(pid: Pid) -> Option<&'static str> {
    match pid.value() {
        0x0000..=0x000F => Some("MPEG-2 Systems fixed/reserved range 0x0000-0x000F"),
        MGT_PID_VALUE => Some("DCII master guide table PID 8187 (r5 F5-6, hypothesized)"),
        _ => None,
    }
}

/// Checks a plan and, if it is sound, returns the type the assemblers
/// accept. All violations are reported, not just the first.
///
/// ```
/// use oob_mux::pid_plan::{PidPlan, validate};
/// assert!(validate(&PidPlan::seed_default()).is_ok());
/// ```
pub fn validate(plan: &PidPlan) -> Result<ValidatedPidPlan, Vec<PidPlanViolation>> {
    let mut v = Vec::new();
    // Unique-use PIDs: PMT, ES, PCR-only and EMM. ECM PIDs may repeat
    // across services but must not collide with any of these.
    let mut uses: HashMap<Pid, Vec<&'static str>> = HashMap::new();
    let mut ecm: Vec<Pid> = Vec::new();
    let mut numbers: Vec<u16> = Vec::new();

    if plan.psi_repetition.is_zero() {
        v.push(PidPlanViolation::ZeroRepetition);
    }
    let mut check_ca = |refs: &[CaRef], v: &mut Vec<PidPlanViolation>| {
        for c in refs {
            if c.private.len() > 251 {
                v.push(PidPlanViolation::CaPrivateTooLong {
                    len: c.private.len(),
                });
            }
            ecm.push(c.ca_pid);
        }
    };
    for p in &plan.programs {
        if p.program_number == 0 {
            v.push(PidPlanViolation::ProgramNumberZero);
        } else if numbers.contains(&p.program_number) {
            v.push(PidPlanViolation::DuplicateProgramNumber(p.program_number));
        }
        numbers.push(p.program_number);
        uses.entry(p.pmt_pid).or_default().push("PMT");
        if !p.streams.iter().any(|s| s.pid == p.pcr_pid) {
            uses.entry(p.pcr_pid).or_default().push("PCR");
        }
        check_ca(&p.ca, &mut v);
        for s in &p.streams {
            uses.entry(s.pid).or_default().push("ES");
            check_ca(&s.ca, &mut v);
        }
    }
    for e in &plan.emm_streams {
        uses.entry(e.pid).or_default().push("EMM");
    }
    let mut pids: Vec<Pid> = uses.keys().copied().collect();
    pids.extend(ecm.iter().copied());
    pids.sort();
    pids.dedup();
    for pid in pids {
        if let Some(why) = reserved(pid) {
            v.push(PidPlanViolation::ReservedPid { pid, why });
        }
    }
    let mut dups: Vec<_> = uses.iter().filter(|(_, u)| u.len() > 1).collect();
    dups.sort_by_key(|(p, _)| **p);
    for (pid, u) in dups {
        v.push(PidPlanViolation::DuplicatePid {
            pid: *pid,
            uses: u.clone(),
        });
    }
    ecm.sort();
    ecm.dedup();
    for pid in ecm {
        if let Some(u) = uses.get(&pid) {
            let mut all = vec!["ECM"];
            all.extend(u.iter().copied());
            v.push(PidPlanViolation::DuplicatePid { pid, uses: all });
        }
    }
    if v.is_empty() {
        // Size checks: build each table at version 0.
        for kind in crate::psi::kinds(plan) {
            if let Err(e) = build_one_for_validation(plan, kind) {
                v.push(e);
            }
        }
    }
    if v.is_empty() {
        Ok(ValidatedPidPlan(plan.clone()))
    } else {
        Err(v)
    }
}
