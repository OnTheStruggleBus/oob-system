//! Validation errors for runtime configuration (ADR-003 Layer 1).
//!
//! The values come from oob-dac's store, so a bad row must be refused when
//! it is applied, not discovered later as a silently broken stream (QA-7).

/// A runtime setting that cannot work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// `SchedulerConfig::queue_depth` is 0: every one-shot would be refused.
    ZeroQueueDepth,
    /// `SchedulerConfig::outcome_capacity` is 0: a rendezvous channel, so
    /// outcomes would be lost whenever the receiver is not parked.
    ZeroOutcomeCapacity,
    /// `PacerConfig::max_pull` is 0: every pull would be refused.
    ZeroMaxPull,
    /// `PacerConfig::pull_budget` is 0: every pull would be late.
    ZeroPullBudget,
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::ZeroQueueDepth => "queue_depth must be at least 1",
            Self::ZeroOutcomeCapacity => "outcome_capacity must be at least 1",
            Self::ZeroMaxPull => "max_pull must be at least 1",
            Self::ZeroPullBudget => "pull_budget must be non-zero",
        })
    }
}

impl std::error::Error for ConfigError {}
