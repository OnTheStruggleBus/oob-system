//! The first-run seed (ADR-003 §6).
//!
//! A [`SeedBundle`] holds one document for each of the four singleton kinds
//! and **nothing else**: it has no units field, and unknown fields are
//! refused, so no box instance can come from git. The store is seeded only
//! when all four singleton kinds have zero revisions; afterwards the store
//! is authoritative and a seed never overwrites a head. The revisions
//! themselves (origin `seed`) are the "has been seeded" marker.
//!
//! Until #30 produces the real seed files the bundle is a JSON fixture
//! (`crates/oob-dac/tests/fixtures/seed-bundle.json`); the daemon takes it
//! as `--seed <path>` (#29).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::doc::{ClassBudgets, Doc, Lineup, MuxSettings, PidPlan, canonical, sha256_hex};
use super::key::ObjectKey;

/// The seed documents. No units, by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedBundle {
    /// The demo lineup.
    pub lineup: Lineup,
    /// The PID plan.
    pub pid_plan: PidPlan,
    /// The class budgets.
    pub class_budgets: ClassBudgets,
    /// The mux settings default.
    pub mux_settings: MuxSettings,
}

impl SeedBundle {
    /// Parses a bundle.
    ///
    /// # Errors
    ///
    /// The serde error text: a missing kind, a wrong type, or any field the
    /// bundle does not have (a `units` list included).
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    /// Reads and parses the bundle at `path`.
    ///
    /// # Errors
    ///
    /// As [`from_json`](Self::from_json), or the I/O error text.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::from_json(&text)
    }

    /// SHA-256 hex of the bundle's canonical JSON. The seed's actor is
    /// `seed:` plus its first 12 hex digits.
    pub fn sha256(&self) -> String {
        sha256_hex(canonical(self).as_bytes())
    }

    /// The four documents with their keys, in kind order.
    pub fn docs(&self) -> [(ObjectKey, Doc); 4] {
        use super::key::ConfigKind as K;
        [
            (
                ObjectKey::singleton(K::Lineup),
                Doc::Lineup(self.lineup.clone()),
            ),
            (
                ObjectKey::singleton(K::PidPlan),
                Doc::PidPlan(self.pid_plan.clone()),
            ),
            (
                ObjectKey::singleton(K::ClassBudgets),
                Doc::ClassBudgets(self.class_budgets.clone()),
            ),
            (
                ObjectKey::singleton(K::MuxSettings),
                Doc::MuxSettings(self.mux_settings.clone()),
            ),
        ]
    }
}

/// What [`seed_if_empty`](super::ConfigAdmin::seed_if_empty) did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedReport {
    /// Revision 1 of each singleton kind was written.
    Seeded {
        /// The seed's actor, `seed:<12 hex>`.
        actor: String,
        /// The keys written.
        keys: Vec<ObjectKey>,
    },
    /// At least one singleton kind already had a revision; nothing was
    /// written.
    AlreadySeeded,
}
