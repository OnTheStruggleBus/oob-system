//! DCII-flavored PSI assemblers: PAT, PMT and CAT with GI/Motorola CA
//! descriptors, driven by a runtime [`PidPlan`]
//! (issue #17, r4 F4-6, r5 F5-6).
//!
//! This is one builder library serving the OOB stream now and the in-band
//! path later (Marshal, 2026-11-23: "Library for both; in-band later").
//! The output is plain section bytes wrapped as C0 system
//! [`Submission::Cyclic`] carousels, so the W2 skeleton stream is PAT, PMT
//! and CAT plus the pacer's nulls. Issue #18 verifies the same bytes with
//! tsduck as an independent decoder; the box check (d 15 / d 16) is a later
//! capture-backed experiment.
//!
//! # Trust labels (AGENTS.md §2)
//!
//! Every row of [`statuses`] is `documented` or `hypothesized`; the test
//! `no_row_is_verified` guards it. No ISO/IEC 13818-1 copy is in hand, so
//! even the generic MPEG-2 structures are `hypothesized`. **Nothing here
//! claims a box accepts these tables.**
//!
//! # Versioning and emission
//!
//! [`TableVersions`] is owned by the caller (oob-dac keeps it in the store
//! so it survives restarts). A table's 5-bit version_number advances
//! (mod 32) only when its bytes change; unchanged tables are re-submitted
//! with identical bytes, which the scheduler treats as idempotent.
//! Continuity counters belong to the packetizer.
//!
//! NIT, the DCII MGT/VCT (PID 8187) and Huffman descriptor text are out of
//! scope (F5-6 research backlog).

pub mod cat;
pub mod dcii;
pub mod descriptor;
pub mod pat;
pub mod pmt;
pub(crate) mod section;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::pdu::{CarouselId, Payload, Pdu, Submission, TrafficClass};
use crate::pid::Pid;
use crate::pid_plan::{
    CAT_PID, MAX_PSI_SECTION_LENGTH, PAT_PID, PidPlan, PidPlanViolation, ValidatedPidPlan,
};
use crate::status::Status;

/// Which table a section is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TableKind {
    /// Program association table.
    Pat,
    /// Program map table of one program.
    Pmt {
        /// The program it describes.
        program_number: u16,
    },
    /// Conditional access table.
    Cat,
}

impl TableKind {
    /// The carousel this table's submissions use. PAT and CAT have fixed
    /// ids; a PMT's id is its program number in the low half.
    pub const fn carousel_id(self) -> CarouselId {
        match self {
            Self::Pat => CarouselId(0x5053_0000),
            Self::Cat => CarouselId(0x5053_0001),
            Self::Pmt { program_number } => CarouselId(0x5053_1000_u32 + program_number as u32),
        }
    }

    /// The trust label of this table's layout.
    pub const fn status(self) -> Status {
        Status::Hypothesized
    }
}

/// One line of the DCII SI conformance table (the mdBook page renders the
/// same rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusRow {
    /// What the row covers.
    pub item: &'static str,
    /// Its label.
    pub status: Status,
    /// The basis for the label.
    pub source: &'static str,
}

const STATUS_ROWS: &[StatusRow] = &[
    StatusRow {
        item: "Long-form section syntax and CRC32",
        status: Status::Hypothesized,
        source: "ISO/IEC 13818-1 structure; no in-hand copy; tsduck cross-check is #18",
    },
    StatusRow {
        item: "PAT layout",
        status: Status::Hypothesized,
        source: "ISO/IEC 13818-1; as above",
    },
    StatusRow {
        item: "PMT layout (no DCII extras emitted)",
        status: Status::Hypothesized,
        source: "ISO/IEC 13818-1; as above; DCII PMT conventions (F4-6) not emitted",
    },
    StatusRow {
        item: "CAT layout",
        status: Status::Hypothesized,
        source: "ISO/IEC 13818-1; as above",
    },
    StatusRow {
        item: "CA_descriptor (tag 0x09) layout",
        status: Status::Hypothesized,
        source: "ISO/IEC 13818-1; as above",
    },
    StatusRow {
        item: "GI/Motorola CA_system_id (0x0700)",
        status: Status::Hypothesized,
        source: "DVB CA_System_ID registry as commonly reported; no DAC/SEM manual or d 16 value in hand",
    },
    StatusRow {
        item: "CA descriptor private bytes (PMT)",
        status: Status::Hypothesized,
        source: "F4-6; empty by default",
    },
    StatusRow {
        item: "EMM provider ID as CAT descriptor private bytes",
        status: Status::Hypothesized,
        source: "Oob construction; the 0x0000/0x0001 value is the AGENTS.md §8 guardrail, the placement is not documented",
    },
    StatusRow {
        item: "PID 8187 (0x1FFB) reserved for the DCII MGT",
        status: Status::Hypothesized,
        source: "r5 F5-6, single source; enforced anyway",
    },
];

/// The conformance rows. Nothing here is `verified` without a capture.
pub fn statuses() -> &'static [StatusRow] {
    STATUS_ROWS
}

/// One table's persisted version state: what oob-dac (#25) stores and
/// restores through [`TableVersions::snapshot`] and
/// [`TableVersions::from_parts`]. `#[non_exhaustive]`, so build it with
/// [`TableVersionEntry::new`] and read it with the getters.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TableVersionEntry {
    /// Which table.
    pub kind: TableKind,
    /// Its 5-bit version_number (0..32).
    pub version: u8,
    /// The section bytes last emitted at that version.
    pub bytes: Arc<[u8]>,
    /// The table left the plan; its version is kept so a re-add continues
    /// the count.
    pub retired: bool,
}

impl TableVersionEntry {
    /// Builds an entry; `version` is taken mod 32.
    pub fn new(kind: TableKind, version: u8, bytes: Arc<[u8]>, retired: bool) -> Self {
        Self {
            kind,
            version: version % 32,
            bytes,
            retired,
        }
    }

    /// Which table.
    pub const fn kind(&self) -> TableKind {
        self.kind
    }

    /// The version_number.
    pub const fn version(&self) -> u8 {
        self.version
    }

    /// The last-emitted section bytes.
    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }

    /// Whether the table is out of the plan.
    pub const fn retired(&self) -> bool {
        self.retired
    }
}

/// The per-table version counters and last-emitted bytes. Owned by the
/// caller so it can persist across restarts.
#[derive(Debug, Default, Clone)]
pub struct TableVersions {
    last: HashMap<TableKind, (u8, Arc<[u8]>)>,
    /// Kinds whose table left the plan. Their version and bytes are kept so
    /// a re-added table continues the mod-32 count instead of restarting
    /// at 0 (a box that cached the old version would keep stale metadata).
    retired: HashSet<TableKind>,
}

impl TableVersions {
    /// An empty set: every table starts at version 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything needed to restore this set after a restart: each table
    /// with its version and last-emitted section bytes, in table order.
    /// oob-dac (#25) stores this and feeds it to [`TableVersions::from_parts`];
    /// until #25 lands, persistence is the caller's job. Without a restore a
    /// restart would re-emit a changed table at version 0, which a box that
    /// cached that version could ignore.
    pub fn snapshot(&self) -> Vec<TableVersionEntry> {
        let mut v: Vec<_> = self
            .last
            .iter()
            .map(|(k, (ver, b))| {
                TableVersionEntry::new(*k, *ver, Arc::clone(b), self.retired.contains(k))
            })
            .collect();
        v.sort_by_key(|e| e.kind);
        v
    }

    /// Restores a set from a [`TableVersions::snapshot`].
    pub fn from_parts(parts: impl IntoIterator<Item = TableVersionEntry>) -> Self {
        let mut out = Self::default();
        for e in parts {
            out.last.insert(e.kind, (e.version, e.bytes));
            if e.retired {
                out.retired.insert(e.kind);
            }
        }
        out
    }

    /// The current version of a table, if it has been built.
    pub fn version(&self, kind: TableKind) -> Option<u8> {
        self.last.get(&kind).map(|(v, _)| *v)
    }
}

/// The tables a plan describes, PAT first.
pub fn kinds(plan: &PidPlan) -> Vec<TableKind> {
    let mut k = vec![TableKind::Pat, TableKind::Cat];
    k.extend(plan.programs.iter().map(|p| TableKind::Pmt {
        program_number: p.program_number,
    }));
    k
}

fn assemble(plan: &PidPlan, kind: TableKind, version: u8) -> (Vec<u8>, usize) {
    match kind {
        TableKind::Pat => pat::build(plan, version),
        TableKind::Cat => cat::build(plan, version),
        TableKind::Pmt { program_number } => match plan
            .programs
            .iter()
            .find(|p| p.program_number == program_number)
        {
            Some(p) => pmt::build(p, version),
            None => (Vec::new(), 0),
        },
    }
}

pub(crate) fn build_one_for_validation(
    plan: &PidPlan,
    kind: TableKind,
) -> Result<(), PidPlanViolation> {
    let (_, len) = assemble(plan, kind, 0);
    if len > MAX_PSI_SECTION_LENGTH {
        Err(PidPlanViolation::SectionTooLong { table: kind, len })
    } else {
        Ok(())
    }
}

fn pid_of(plan: &PidPlan, kind: TableKind) -> Pid {
    match kind {
        TableKind::Pat => PAT_PID,
        TableKind::Cat => CAT_PID,
        TableKind::Pmt { program_number } => plan
            .programs
            .iter()
            .find(|p| p.program_number == program_number)
            .map_or(PAT_PID, |p| p.pmt_pid),
    }
}

/// Builds one table, advancing its version only if the bytes changed.
/// Returns the section bytes.
pub(crate) fn build_table(
    plan: &ValidatedPidPlan,
    kind: TableKind,
    versions: &mut TableVersions,
) -> Arc<[u8]> {
    let plan = plan.plan();
    let (version, bytes): (u8, Arc<[u8]>) = match versions.last.get(&kind) {
        None => (0, assemble(plan, kind, 0).0.into()),
        Some((pv, prev)) => {
            let same = assemble(plan, kind, *pv).0;
            if prev.as_ref() == same.as_slice() {
                (*pv, Arc::clone(prev))
            } else {
                let nv = (*pv + 1) % 32;
                (nv, assemble(plan, kind, nv).0.into())
            }
        }
    };
    versions.last.insert(kind, (version, Arc::clone(&bytes)));
    bytes
}

/// What [`build_all`] returns.
#[derive(Debug)]
pub struct PsiSet {
    /// One cyclic C0 system submission per table.
    pub submissions: Vec<Submission>,
    /// Carousels of tables that were in the previous plan and are no
    /// longer (a removed program's PMT): withdraw these.
    pub withdrawn: Vec<CarouselId>,
}

/// Builds every table of the plan as a C0 system cyclic submission with
/// period `psi_repetition`. Safe to call on every plan apply: unchanged
/// tables repeat identical bytes, which the scheduler keeps on schedule.
pub fn build_all(plan: &ValidatedPidPlan, versions: &mut TableVersions) -> PsiSet {
    let p = plan.plan();
    let current = kinds(p);
    // Newly withdrawn tables keep their version and bytes (retired), so a
    // later re-add continues the count; they are withdrawn once.
    let mut gone: Vec<TableKind> = versions
        .last
        .keys()
        .filter(|k| !current.contains(k) && !versions.retired.contains(k))
        .copied()
        .collect();
    gone.sort();
    let withdrawn: Vec<CarouselId> = gone.iter().map(|k| k.carousel_id()).collect();
    versions.retired.extend(gone);
    for k in &current {
        versions.retired.remove(k);
    }
    let submissions = current
        .iter()
        .map(|&kind| {
            let bytes = build_table(plan, kind, versions);
            Submission::Cyclic {
                id: kind.carousel_id(),
                pdu: Pdu {
                    class: TrafficClass::C0System,
                    priority: 0,
                    pid: pid_of(p, kind),
                    release_at: None,
                    deadline: None,
                    payload: Payload::Section(bytes),
                    stamp: None,
                },
                period: p.psi_repetition,
            }
        })
        .collect();
    PsiSet {
        submissions,
        withdrawn,
    }
}
