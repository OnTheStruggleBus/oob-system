//! The configuration documents: oob-dac's own serde structs, one per kind.
//!
//! - **Raw values only.** Integers and strings, no mux or message-layer
//!   types inside, so a document always parses and a bad value surfaces as
//!   a **named rule** from the validator (#26) rather than a serde error.
//!   Converting a document into a mux value is the validator's first step.
//!   These structs are the source of the REST schemas (#27).
//! - **No floats.** Frequencies are integer kHz (`freq_khz`), rates integer
//!   bit/s or kbit/s, durations integer milliseconds or microseconds. The
//!   860 MHz guardrail is `freq_khz <= 860_000`, and the canonical JSON is
//!   byte-stable, so [`Doc::sha256`] is reproducible.
//! - **Unknown fields are refused** (`deny_unknown_fields`): a field the
//!   schema does not know would otherwise be dropped silently on save.
//! - **Canonical JSON** ([`Doc::canonical_json`]): keys sorted at every
//!   level, no whitespace. It is what `config_revision.doc` stores and what
//!   the digest covers.
//! - **Schema versions** ([`DOC_SCHEMA`]): every kind is at 1. A revision
//!   stored at an older schema is upgraded **in memory** when read; the
//!   stored row is never rewritten. A newer one is refused.

pub mod class_budgets;
pub mod lineup;
pub mod mux_settings;
pub mod pid_plan;
pub mod unit;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use class_budgets::{ClassBudget, ClassBudgets, PerClass};
pub use lineup::{Lineup, PlayoutBinding, RfChannel, Sem, SemOutput, VcmEntry};
pub use mux_settings::{MuxSettings, PacerSettings, QueueSettings};
pub use pid_plan::{CaRef, ElementaryStream, EmmStream, PidPlan, Program};
pub use unit::UnitDoc;

use super::key::ConfigKind;

/// The current document schema version of `kind`. Code (Layer 3); a change
/// to a document's shape bumps it and adds an upgrade step.
pub const fn doc_schema(kind: ConfigKind) -> u32 {
    match kind {
        ConfigKind::Lineup
        | ConfigKind::PidPlan
        | ConfigKind::ClassBudgets
        | ConfigKind::MuxSettings
        | ConfigKind::Unit => 1,
    }
}

/// [`doc_schema`] for every kind, for the docs page.
pub const DOC_SCHEMA: [(ConfigKind, u32); 5] = [
    (ConfigKind::Lineup, doc_schema(ConfigKind::Lineup)),
    (ConfigKind::PidPlan, doc_schema(ConfigKind::PidPlan)),
    (
        ConfigKind::ClassBudgets,
        doc_schema(ConfigKind::ClassBudgets),
    ),
    (ConfigKind::MuxSettings, doc_schema(ConfigKind::MuxSettings)),
    (ConfigKind::Unit, doc_schema(ConfigKind::Unit)),
];

/// One document upgrade step: `kind` at schema `from` becomes `from + 1`.
#[derive(Clone, Copy)]
pub(crate) struct Upgrade {
    pub(crate) kind: ConfigKind,
    pub(crate) from: u32,
    pub(crate) step: fn(Value) -> Result<Value, String>,
}

/// The released upgrade steps. Empty while every kind is at schema 1.
pub(crate) const UPGRADES: &[Upgrade] = &[];

/// A configuration document of any kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doc {
    /// The lineup.
    Lineup(Lineup),
    /// The PID plan.
    PidPlan(PidPlan),
    /// The class budgets.
    ClassBudgets(ClassBudgets),
    /// The mux settings.
    MuxSettings(MuxSettings),
    /// A unit's operational record.
    Unit(UnitDoc),
}

fn to_value<T: Serialize>(t: &T) -> Value {
    // Every document is plain data (strings, integers, bools, lists, maps
    // with string keys), so serialization cannot fail.
    serde_json::to_value(t).unwrap_or(Value::Null)
}

/// `v` with every object's keys sorted, recursively (whatever map type
/// serde_json was built with).
pub(crate) fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(String, Value)> = m.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// Canonical JSON text of any serializable value.
pub(crate) fn canonical<T: Serialize>(t: &T) -> String {
    sorted(to_value(t)).to_string()
}

/// SHA-256 hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn parse<T: DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| e.to_string())
}

impl Doc {
    /// Its kind.
    pub fn kind(&self) -> ConfigKind {
        match self {
            Self::Lineup(_) => ConfigKind::Lineup,
            Self::PidPlan(_) => ConfigKind::PidPlan,
            Self::ClassBudgets(_) => ConfigKind::ClassBudgets,
            Self::MuxSettings(_) => ConfigKind::MuxSettings,
            Self::Unit(_) => ConfigKind::Unit,
        }
    }

    fn value(&self) -> Value {
        match self {
            Self::Lineup(d) => to_value(d),
            Self::PidPlan(d) => to_value(d),
            Self::ClassBudgets(d) => to_value(d),
            Self::MuxSettings(d) => to_value(d),
            Self::Unit(d) => to_value(d),
        }
    }

    /// The canonical JSON form: sorted keys, no whitespace.
    pub fn canonical_json(&self) -> String {
        sorted(self.value()).to_string()
    }

    /// SHA-256 hex of [`canonical_json`](Self::canonical_json).
    pub fn sha256(&self) -> String {
        sha256_hex(self.canonical_json().as_bytes())
    }

    /// Parses a current-schema document of `kind` from JSON.
    ///
    /// # Errors
    ///
    /// The serde error text (a wrong type or an unknown field).
    pub fn from_json(kind: ConfigKind, json: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        Self::from_value(kind, v)
    }

    pub(crate) fn from_value(kind: ConfigKind, v: Value) -> Result<Self, String> {
        Ok(match kind {
            ConfigKind::Lineup => Self::Lineup(parse(v)?),
            ConfigKind::PidPlan => Self::PidPlan(parse(v)?),
            ConfigKind::ClassBudgets => Self::ClassBudgets(parse(v)?),
            ConfigKind::MuxSettings => Self::MuxSettings(parse(v)?),
            ConfigKind::Unit => Self::Unit(parse(v)?),
        })
    }

    /// The unit document, if this is one.
    pub fn as_unit(&self) -> Option<&UnitDoc> {
        match self {
            Self::Unit(u) => Some(u),
            _ => None,
        }
    }
}

/// Why a stored document could not be read at the current schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpgradeError {
    /// Stored by a newer build.
    TooNew { found: u32, supported: u32 },
    /// No step from `from`, or a step or the final parse failed.
    Failed(String),
}

/// Reads a stored document of `kind` at schema `stored`, applying `steps`
/// in memory up to `current` (the service passes [`doc_schema`]).
pub(crate) fn upgrade(
    kind: ConfigKind,
    stored: u32,
    current: u32,
    json: &str,
    steps: &[Upgrade],
) -> Result<Doc, UpgradeError> {
    if stored > current {
        return Err(UpgradeError::TooNew {
            found: stored,
            supported: current,
        });
    }
    let mut v: Value =
        serde_json::from_str(json).map_err(|e| UpgradeError::Failed(e.to_string()))?;
    for from in stored..current {
        let step = steps
            .iter()
            .find(|u| u.kind == kind && u.from == from)
            .ok_or_else(|| {
                UpgradeError::Failed(format!("no upgrade for {kind} from schema {from}"))
            })?;
        v = (step.step)(v).map_err(UpgradeError::Failed)?;
    }
    Doc::from_value(kind, v).map_err(UpgradeError::Failed)
}
