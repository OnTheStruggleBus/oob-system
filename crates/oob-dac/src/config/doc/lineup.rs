//! The `lineup` document: the RF lineup and what rides on it, mirroring the
//! seed `ops/lineup.yaml` (#30). Consistency (lineup ↔ VCM ↔ playout ↔ SEM,
//! one STD/HRC choice, the 860 MHz tuner ceiling) is checked by the
//! validator (#26), not by these types.

use serde::{Deserialize, Serialize};

/// The lineup singleton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lineup {
    /// The channel plan, `STD` or `HRC` (decided once, r4 F4-8).
    pub plan: String,
    /// The SEM's configuration.
    pub sem: Sem,
    /// The RF channels carrying digital multiplexes.
    #[serde(default)]
    pub rf_channels: Vec<RfChannel>,
    /// The virtual channel map.
    #[serde(default)]
    pub vcm: Vec<VcmEntry>,
    /// Which playout source feeds which virtual channel.
    #[serde(default)]
    pub playout: Vec<PlayoutBinding>,
}

/// The SEM's view of the plant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sem {
    /// The channel plan configured on the SEM (must equal [`Lineup::plan`]).
    pub plan: String,
    /// The SEM's QAM outputs.
    #[serde(default)]
    pub outputs: Vec<SemOutput>,
}

/// One SEM QAM output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemOutput {
    /// The output's index on the SEM.
    pub output: u32,
    /// The RF channel number it transmits.
    pub rf_channel: u32,
}

/// One RF channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfChannel {
    /// The channel number in the plan.
    pub channel: u32,
    /// Centre frequency in kHz (integer; the tuner ceiling is 860 000).
    pub freq_khz: u32,
    /// `qam64` or `qam256` (J.83 Annex B).
    pub modulation: String,
    /// The multiplex's transport_stream_id.
    pub transport_stream_id: u16,
}

/// One virtual channel map entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VcmEntry {
    /// The virtual channel number the box shows.
    pub virtual_channel: u16,
    /// The RF channel carrying it.
    pub rf_channel: u32,
    /// Its program_number in that multiplex.
    pub program_number: u16,
    /// The short name.
    pub name: String,
}

/// One playout binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayoutBinding {
    /// The virtual channel fed.
    pub virtual_channel: u16,
    /// The playout source's name (a self-authored loop, AGENTS.md §5).
    pub source: String,
}
