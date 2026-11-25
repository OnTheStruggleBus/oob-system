//! CAT builder (table_id 0x01, PID 0x0001). Layout `hypothesized`.

use super::descriptor::emm_descriptor;
use super::section::long_section;
use crate::pid_plan::PidPlan;

/// Builds the CAT section and its section_length. table_id_extension is
/// 0xFFFF (reserved, all ones).
pub(crate) fn build(plan: &PidPlan, version: u8) -> (Vec<u8>, usize) {
    let body: Vec<u8> = plan.emm_streams.iter().flat_map(emm_descriptor).collect();
    long_section(0x01, 0xFFFF, version, &body)
}
