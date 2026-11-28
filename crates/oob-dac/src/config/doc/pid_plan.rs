//! The `pid_plan` document: the OOB stream's PID plan in the raw shape of
//! the mux's plan (#17). PIDs are plain integers here; the validator (#26)
//! converts the document and so refuses an out-of-range PID, a reserved
//! PID or an EMM provider ID other than 0x0000/0x0001 by rule name.

use serde::{Deserialize, Serialize};

/// The PID plan singleton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PidPlan {
    /// transport_stream_id of the PAT.
    pub transport_stream_id: u16,
    /// Programs listed in the PAT.
    #[serde(default)]
    pub programs: Vec<Program>,
    /// EMM streams listed in the CAT.
    #[serde(default)]
    pub emm_streams: Vec<EmmStream>,
    /// Carousel period for PAT, PMT and CAT, in milliseconds.
    pub psi_repetition_ms: u32,
}

/// One program (service).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    /// program_number (never 0).
    pub program_number: u16,
    /// The PMT's PID.
    pub pmt_pid: u16,
    /// The PCR PID.
    pub pcr_pid: u16,
    /// Program-level CA references.
    #[serde(default)]
    pub ca: Vec<CaRef>,
    /// Elementary streams.
    #[serde(default)]
    pub streams: Vec<ElementaryStream>,
    /// DCII virtual channel number (r5 F5-6).
    #[serde(default)]
    pub virtual_channel: Option<u16>,
}

/// One elementary stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementaryStream {
    /// MPEG-2 stream_type.
    pub stream_type: u8,
    /// The stream's PID.
    pub pid: u16,
    /// ES-level CA references.
    #[serde(default)]
    pub ca: Vec<CaRef>,
}

/// A CA reference (an ECM PID of a CA system).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaRef {
    /// The CA system's name (`gi-digicipher`).
    pub system: String,
    /// The ECM PID.
    pub ca_pid: u16,
    /// CA descriptor private bytes (`hypothesized`; usually empty).
    #[serde(default)]
    pub private: Vec<u8>,
}

/// One EMM stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmmStream {
    /// The CA system's name (`gi-digicipher`).
    pub system: String,
    /// The EMM PID.
    pub pid: u16,
    /// The EMM provider ID; only 0x0000 and 0x0001 pass the validator
    /// (AGENTS.md §8).
    pub provider_id: u16,
}
