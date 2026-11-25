//! Class semantics: the part of the class model that is code, not
//! configuration (ADR-003 Layer 3, #16 design note §3 and §5).
//!
//! The class set C0–C5, the service order, which class preempts, which
//! classes must carry a cap and which is gated by lab mode are fixed here.
//! Floors, caps, bursts and the lab-mode switch are runtime settings
//! ([`BudgetConfig`](super::BudgetConfig)).

use crate::pdu::TrafficClass;

/// The only class that preempts: C2 (EAS) is served ahead of the C3–C5
/// floors, bounded by its cap (r6 F6-3; Marshal's ruling on the #16 design
/// note, 2026-11-23).
pub const PREEMPTOR: TrafficClass = TrafficClass::C2Eas;

/// The class that is off outside lab mode.
pub const LAB_GATED: TrafficClass = TrafficClass::C5Lab;

/// Classes served in floor phase A: guaranteed, and never touched by
/// preemption.
pub(crate) const GUARANTEED: [TrafficClass; 2] =
    [TrafficClass::C0System, TrafficClass::C1Addressed];

/// Classes served in floor phase C, after the preemptor.
pub(crate) const BELOW_PREEMPTOR: [TrafficClass; 3] = [
    TrafficClass::C3Guide,
    TrafficClass::C4Cdl,
    TrafficClass::C5Lab,
];

/// Whether `class` must carry a cap, so it cannot permanently eat the
/// classes below it (C2) or the budget (C4, C5).
pub const fn requires_cap(class: TrafficClass) -> bool {
    matches!(
        class,
        TrafficClass::C2Eas | TrafficClass::C4Cdl | TrafficClass::C5Lab
    )
}

/// Whether `class` is served at all given the lab-mode switch.
pub const fn enabled(class: TrafficClass, lab_mode: bool) -> bool {
    match class {
        LAB_GATED => lab_mode,
        _ => true,
    }
}
