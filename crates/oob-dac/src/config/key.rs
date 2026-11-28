//! What a configuration object is called: its kind, its key, its revisions.

use std::fmt;

use crate::store::TerminalId;

use super::doc::Doc;

/// The Layer 1 object kinds (ADR-003 §2). The list is code (Layer 3): there
/// is deliberately no kind for box types, which come only from the upstream
/// catalog ([`super::catalog`], Marshal's QA-6 ruling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConfigKind {
    /// The RF lineup, SEM outputs, VCM and playout bindings (singleton).
    Lineup,
    /// The OOB stream PID plan (singleton).
    PidPlan,
    /// The traffic-class insertion budgets C0–C5 (singleton, F6-3).
    ClassBudgets,
    /// The pacer and queue settings of the OOB mux (singleton).
    MuxSettings,
    /// One unit's mutable operational record, keyed by terminal ID.
    Unit,
}

impl ConfigKind {
    /// Every kind.
    pub const ALL: [Self; 5] = [
        Self::Lineup,
        Self::PidPlan,
        Self::ClassBudgets,
        Self::MuxSettings,
        Self::Unit,
    ];

    /// The four singleton kinds (one object each, no object ID).
    pub const SINGLETONS: [Self; 4] = [
        Self::Lineup,
        Self::PidPlan,
        Self::ClassBudgets,
        Self::MuxSettings,
    ];

    /// The stored name (`config_revision.kind`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lineup => "lineup",
            Self::PidPlan => "pid_plan",
            Self::ClassBudgets => "class_budgets",
            Self::MuxSettings => "mux_settings",
            Self::Unit => "unit",
        }
    }

    /// The kind stored as `s`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Whether there is exactly one object of this kind.
    pub fn is_singleton(self) -> bool {
        self != Self::Unit
    }
}

impl fmt::Display for ConfigKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One configuration object: a singleton kind, or a unit by terminal ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectKey {
    /// The kind.
    pub kind: ConfigKind,
    /// The terminal ID for a unit; `None` for a singleton.
    pub object_id: Option<TerminalId>,
}

impl ObjectKey {
    /// The singleton of `kind`. Use [`unit`](Self::unit) for units.
    pub fn singleton(kind: ConfigKind) -> Self {
        Self {
            kind,
            object_id: None,
        }
    }

    /// A unit's operational record.
    pub fn unit(id: TerminalId) -> Self {
        Self {
            kind: ConfigKind::Unit,
            object_id: Some(id),
        }
    }

    /// Whether the key is well formed: a singleton has no object ID, a unit
    /// has one.
    pub fn is_well_formed(&self) -> bool {
        self.kind.is_singleton() == self.object_id.is_none()
    }

    /// The stored `object_id` text: `''` or the decimal terminal ID.
    pub(crate) fn object_text(&self) -> String {
        self.object_id
            .map(|id| id.get().to_string())
            .unwrap_or_default()
    }

    /// The audit target, `kind[/object_id]@revision`.
    pub(crate) fn target(&self, revision: u32) -> String {
        format!("{self}@{revision}")
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.object_id {
            Some(id) => write!(f, "{}/{}", self.kind, id.get()),
            None => write!(f, "{}", self.kind),
        }
    }
}

/// The optimistic-concurrency token: the head revision the caller edited.
/// There is no "whatever is there" value: a client must send what it read,
/// and a stale value is refused with [`ConfigError::Conflict`] (HTTP 409 and
/// `If-Match` in #27).
///
/// [`ConfigError::Conflict`]: super::ConfigError::Conflict
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseRevision {
    /// The object has no revision yet.
    None,
    /// The caller edited this revision, which must still be the head.
    At(u32),
}

impl BaseRevision {
    /// The base for a head of `head` (`None` when there is no revision).
    pub fn of(head: Option<u32>) -> Self {
        head.map_or(Self::None, Self::At)
    }
}

/// How a revision came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// An operator save (also unit enroll and update).
    Save,
    /// A rollback: a save of an older revision's document.
    Rollback,
    /// The first-run seed.
    Seed,
}

impl Origin {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::Rollback => "rollback",
            Self::Seed => "seed",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        [Self::Save, Self::Rollback, Self::Seed]
            .into_iter()
            .find(|o| o.as_str() == s)
    }
}

/// One revision's metadata (a history row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionMeta {
    /// The object.
    pub key: ObjectKey,
    /// Its revision number, from 1, contiguous per object.
    pub revision: u32,
    /// SHA-256 hex of the canonical document.
    pub doc_sha256: String,
    /// The document schema version it was stored at.
    pub doc_schema: u32,
    /// Save, rollback or seed.
    pub origin: Origin,
    /// For a rollback, the revision it restored.
    pub rolled_back_to: Option<u32>,
    /// Who made it.
    pub actor: String,
    /// When (UTC, ISO 8601).
    pub ts_utc: String,
    /// The journal (`audit_log`) row written with it.
    pub audit_seq: i64,
}

/// One revision with its document (upgraded to the current schema).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// The metadata.
    pub meta: RevisionMeta,
    /// The document.
    pub doc: Doc,
}
