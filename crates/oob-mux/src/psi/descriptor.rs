//! The CA_descriptor (tag 0x09) and descriptor loops.
//!
//! Layout (13818-1, **`hypothesized`**): `tag(8)=0x09, length(8),
//! CA_system_id(16), reserved(3)=111, CA_PID(13), private bytes`. The
//! CA_system_id value is `hypothesized` ([`super::dcii`]); the private
//! bytes are `hypothesized` and empty unless the plan sets them (PMT) or
//! the CAT carries the EMM provider ID (see the CAT builder).

use super::dcii::CaSystem;
use super::section::{descriptor, pid_field};
use crate::pid_plan::{CaRef, EmmStream};

/// MPEG-2 CA_descriptor tag.
pub const CA_DESCRIPTOR_TAG: u8 = 0x09;

/// One CA_descriptor. Crate-private: `validate` bounds `private` to 251
/// bytes, so only plan-driven callers reach it with a sound length.
pub(crate) fn ca_descriptor(system: CaSystem, ca_pid: u16, private: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(4 + private.len());
    p.extend_from_slice(&system.id().to_be_bytes());
    p.extend_from_slice(&pid_field(ca_pid));
    p.extend_from_slice(private);
    descriptor(CA_DESCRIPTOR_TAG, &p)
}

/// The descriptor loop for a list of [`CaRef`]s.
pub(crate) fn ca_loop(refs: &[CaRef]) -> Vec<u8> {
    refs.iter()
        .flat_map(|c| ca_descriptor(c.system, c.ca_pid.value(), &c.private))
        .collect()
}

/// The CAT descriptor for an EMM stream. The EMM provider ID (0x0000 or
/// 0x0001, AGENTS.md §8) is carried as two private bytes. **That
/// placement is `hypothesized`**: it is Oob's construction, not a
/// documented DCII layout, and exists so the guardrailed value is visible
/// to the validator in #18 and to a later d 16 comparison.
pub(crate) fn emm_descriptor(e: &EmmStream) -> Vec<u8> {
    ca_descriptor(
        e.system,
        e.pid.value(),
        &e.provider_id.value().to_be_bytes(),
    )
}
