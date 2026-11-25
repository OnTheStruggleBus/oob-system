//! PAT builder (table_id 0x00, PID 0x0000). Layout `hypothesized`.

use super::section::{long_section, pid_field};
use crate::pid_plan::PidPlan;

/// Builds the PAT section and its section_length.
pub(crate) fn build(plan: &PidPlan, version: u8) -> (Vec<u8>, usize) {
    let mut body = Vec::with_capacity(4 * plan.programs.len());
    for p in &plan.programs {
        body.extend_from_slice(&p.program_number.to_be_bytes());
        body.extend_from_slice(&pid_field(p.pmt_pid.value()));
    }
    long_section(0x00, plan.transport_stream_id, version, &body)
}
