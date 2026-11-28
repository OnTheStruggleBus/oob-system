//! The `class_budgets` document: the insertion budget shared by traffic
//! classes C0–C5 (r6 F6-3, ADR-002 decision 4), in the raw shape of the
//! mux's budget configuration (#16). The validator (#26) converts it and
//! refuses a budget that starves C0 by rule name. Rates are kbit/s of
//! 188-byte transport packets on the wire.

use serde::{Deserialize, Serialize};

/// The class budgets singleton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassBudgets {
    /// The total non-null insertion budget in kbit/s (~1000).
    pub insertion_kbps: u32,
    /// One budget per class; every class is present.
    pub classes: PerClass,
    /// Lab mode: C5 is refused and drained while this is `false`.
    pub lab_mode: bool,
}

/// One budget per traffic class, by name, so none can be missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerClass {
    /// C0, system (time, connect/config).
    pub c0: ClassBudget,
    /// C1, addressed operations.
    pub c1: ClassBudget,
    /// C2, EAS (served by preemption; its floor must be 0).
    pub c2: ClassBudget,
    /// C3, guide and bulk data.
    pub c3: ClassBudget,
    /// C4, code download.
    pub c4: ClassBudget,
    /// C5, Discovery Lab.
    pub c5: ClassBudget,
}

/// One class's share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassBudget {
    /// Reserved rate in kbit/s.
    pub floor_kbps: u32,
    /// Ceiling in kbit/s; absent means "up to the insertion budget".
    #[serde(default)]
    pub cap_kbps: Option<u32>,
    /// Bucket depth in packets.
    pub burst_packets: u16,
}
