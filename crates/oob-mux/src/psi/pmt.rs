//! PMT builder (table_id 0x02, on the program's PMT PID). Layout
//! `hypothesized`; DCII PMT conventions beyond the CA descriptors (F4-6)
//! are not emitted.

use super::descriptor::ca_loop;
use super::section::{len12_field, long_section, pid_field};
use crate::pid_plan::ProgramPlan;

/// Builds one PMT section and its section_length.
pub(crate) fn build(p: &ProgramPlan, version: u8) -> (Vec<u8>, usize) {
    let prog_desc = ca_loop(&p.ca);
    let mut body = Vec::new();
    body.extend_from_slice(&pid_field(p.pcr_pid.value()));
    body.extend_from_slice(&len12_field(prog_desc.len()));
    body.extend_from_slice(&prog_desc);
    for s in &p.streams {
        let es_desc = ca_loop(&s.ca);
        body.push(s.stream_type);
        body.extend_from_slice(&pid_field(s.pid.value()));
        body.extend_from_slice(&len12_field(es_desc.len()));
        body.extend_from_slice(&es_desc);
    }
    long_section(0x02, p.program_number, version, &body)
}
