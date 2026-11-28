//! The `mux_settings` document: the OOB mux's pacer and queue settings
//! (#14, #15; the fifth kind added by oob-po on #25).
//!
//! The **pacer** fields are hot-applicable through the pacer control
//! channel (#49, used by the daemon #29). The **queue** fields size the
//! mux's queues at start, so a change to them is reported
//! restart-required by the applier. Process bootstrap values (the seam
//! socket path and the like) are not configuration objects (#29 note §2).

use serde::{Deserialize, Serialize};

/// The mux settings singleton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MuxSettings {
    /// Hot: the pull seam's pacing.
    pub pacer: PacerSettings,
    /// Restart-required: queue and channel sizes.
    pub queues: QueueSettings,
}

/// The pacer's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacerSettings {
    /// The rate the flowgraph pulls at, in bit/s (2 048 000 for the OOB
    /// stream; still a parameter after O7, r6 F6-2).
    pub packet_rate_bps: u32,
    /// The largest pull accepted, in packets (at least 1).
    pub max_pull: u32,
    /// The documented worst-case duration of one pull, in microseconds
    /// (non-zero).
    pub pull_budget_us: u32,
}

/// The mux's queue and channel sizes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueSettings {
    /// Maximum one-shot PDUs waiting per class (at least 1).
    pub queue_depth: u32,
    /// Maximum registered carousels (0 disables carousels).
    pub max_carousels: u32,
    /// Capacity of the outcome channel (at least 1).
    pub outcome_capacity: u32,
}
