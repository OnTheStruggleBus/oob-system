//! The configuration store (issue #25, ADR-003 §2, r6 F6-3, F6-4, F6-5).
//!
//! Operating a headend is runtime configuration (ADR-003 Layer 1): the
//! lineup, the PID plan, the class budgets, the mux settings and every
//! unit's operational record live in oob-dac's store, change through the
//! command layer, and keep a full revision history. The running system
//! needs no git and opens no PRs.
//!
//! # Kinds
//!
//! | Kind | Key | Document | Head |
//! | --- | --- | --- | --- |
//! | `lineup` | singleton | [`doc::Lineup`] | latest revision |
//! | `pid_plan` | singleton | [`doc::PidPlan`] | latest revision |
//! | `class_budgets` | singleton | [`doc::ClassBudgets`] | latest revision |
//! | `mux_settings` | singleton | [`doc::MuxSettings`] | latest revision |
//! | `unit` | terminal ID | [`doc::UnitDoc`] | the `terminal` row, with a revision per change |
//!
//! The list is code: there is **no kind for box types**. A unit names a box
//! type from the read-only upstream [`catalog`]; an unknown one is refused
//! and nothing creates a box type at runtime (Marshal's QA-6 ruling).
//!
//! # Rules
//!
//! - **Every change is a revision.** `config_revision` is precious and
//!   append-only (no update, no delete, no overwrite, by trigger). Revision
//!   numbers are contiguous per object; the head is the highest one.
//! - **Optimistic concurrency.** Every change names the head it edited
//!   ([`BaseRevision`]); a stale one is [`ConfigError::Conflict`]. The store
//!   has one writer and `BEGIN IMMEDIATE` transactions, so saves serialize.
//! - **Validation on every change.** The mandatory [`ConfigValidator`] sees
//!   the whole candidate configuration ([`check`]); a refusal stores
//!   nothing.
//! - **Rollback is a save** of an older revision's document: validated like
//!   any save, written as a new revision (`origin = 'rollback'`). History is
//!   never rewritten and never bypasses the guardrails.
//! - **The journal is `audit_log`.** Each revision's audit row (actor, role,
//!   front door, `kind[/id]@revision`, the full before and after documents)
//!   is written in the same transaction as the revision and its
//!   [`crate::store::Event::ConfigSaved`].
//! - **Units are dual-represented on purpose.** The `terminal` row is the
//!   current record targeting reads; every change to its mutable fields
//!   also writes a `unit` revision in the same transaction, and only this
//!   module writes those columns. Identity (terminal ID, unit address,
//!   check digits, serial) is immutable after enrollment. Retiring sets
//!   `retired_utc` and never deletes; a retired unit is out of targeting.
//! - **The first-run seed** ([`seed`]) runs only on a store with no
//!   singleton revision at all, holds no unit, and never overwrites a head.
//! - **Applying is a side effect after commit** ([`apply`]); a failed apply
//!   never un-saves.
//!
//! # Entry
//!
//! Changes enter through `Command::Admin(`[`AdminOp`]`)` on the
//! [`CommandBus`](crate::command::CommandBus): the policy layer (#23) then
//! applies the owner role, guest refusal, the `mark-sacrificial` token and
//! the audit row to configuration as to every other command. The service
//! that does the work is crate-internal. [`ConfigAdmin`] is what the daemon
//! (#29) builds and hands to the bus; its public methods are bootstrap only
//! (the first-run seed and the boot-time apply). Reads are on
//! [`Reader`](crate::store::Reader) (`config_head`, `config_history`,
//! `config_revision`).
//!
//! See `architecture/oob-dac-config.md` in the book.

pub mod apply;
pub mod catalog;
pub mod check;
pub mod doc;
pub mod key;
pub mod seed;

mod admin;
mod persist;
mod read;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use crate::command::CommandContext;
use crate::store::{AuditEntry, ConstraintKind, Event, NewTerminal, StoreError, TerminalId, Tx};

pub(crate) use admin::marks_sacrificial;
pub use admin::{AdminOp, AdminOutcome, BootHead, BootStatus, ConfigAdmin, SaveChange, SavedItem};
pub use apply::{ApplyStatus, ConfigApplier, NoApplier};
pub use catalog::{BoxTypeCatalog, CatalogError, FileCatalog, InMemoryCatalog};
pub use check::{
    Change, ConfigSnapshot, ConfigValidator, NewUnit, ObjectRef, Refusal, STRUCTURAL_RULES,
    StructuralValidator, UnitView, Violation,
};
pub use doc::Doc;
pub use key::{BaseRevision, ConfigKind, ObjectKey, Origin, Revision, RevisionMeta};
pub use seed::{SeedBundle, SeedReport};

use self::doc::{ClassBudgets, Lineup, MuxSettings, PidPlan, UnitDoc, Upgrade, UpgradeError};
use self::persist::{NewRevision, Units};

/// The default cap on a document's canonical JSON size (1 MiB), so a client
/// cannot bloat a precious table.
pub const DEFAULT_MAX_DOC_BYTES: usize = 1 << 20;

/// The largest history page a read returns.
pub const HISTORY_PAGE_MAX: u32 = 500;

/// Install settings for the configuration store (bootstrap configuration,
/// #29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigLimits {
    /// The cap on a document's canonical JSON, in bytes.
    pub max_doc_bytes: usize,
}

impl Default for ConfigLimits {
    fn default() -> Self {
        Self {
            max_doc_bytes: DEFAULT_MAX_DOC_BYTES,
        }
    }
}

/// What a save did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saved {
    /// A new revision was written.
    Revised {
        /// The new head.
        revision: u32,
    },
    /// The document equals the head (same digest); nothing was written.
    Unchanged {
        /// The head, unchanged.
        revision: u32,
    },
}

impl Saved {
    /// The head after the save.
    pub fn revision(self) -> u32 {
        match self {
            Self::Revised { revision } | Self::Unchanged { revision } => revision,
        }
    }
}

/// Why a configuration change was refused or failed. Every variant has a
/// stable [`reason`](Self::reason) for REST and WireLink errors.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The base revision is not the head (HTTP 409); `head` is 0 when the
    /// object has no revision.
    Conflict {
        /// The current head.
        head: u32,
    },
    /// The validator refused; nothing was stored.
    Refused(Refusal),
    /// The unit's box type is not in the upstream catalog.
    UnknownBoxType(String),
    /// The object has no such revision.
    UnknownRevision {
        /// The object.
        key: ObjectKey,
        /// The revision asked for.
        revision: u32,
    },
    /// No such object (an unknown terminal ID).
    UnknownObject(ObjectKey),
    /// The document's canonical JSON is over the size cap.
    DocTooLarge {
        /// Its size.
        bytes: usize,
        /// The cap.
        max: usize,
    },
    /// The unit is retired; reinstate it first.
    Retired(TerminalId),
    /// The key and the document do not fit together (a unit saved as a
    /// singleton, a document of another kind, a key twice in a batch).
    WrongKind(String),
    /// A unit is enrolled non-sacrificial; marking it is a separate D-hard
    /// update (`mark-sacrificial`).
    SacrificialAtEnroll,
    /// A stored document's schema is newer than this build.
    SchemaTooNew {
        /// The stored schema.
        found: u32,
        /// The newest this build reads.
        supported: u32,
    },
    /// A field outside its domain, or a store integrity rule (duplicate
    /// unit address or serial).
    Constraint(ConstraintKind),
    /// A stored document could not be read (a bug or damage).
    Undecodable(String),
    /// The store failed.
    Store(StoreError),
}

impl ConfigError {
    /// A stable code naming the refusal.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Conflict { .. } => "conflict",
            Self::Refused(_) => "config-refused",
            Self::UnknownBoxType(_) => "unknown-box-type",
            Self::UnknownRevision { .. } => "unknown-revision",
            Self::UnknownObject(_) => "unknown-object",
            Self::DocTooLarge { .. } => "doc-too-large",
            Self::Retired(_) => "unit-retired",
            Self::WrongKind(_) => "wrong-kind",
            Self::SacrificialAtEnroll => "sacrificial-at-enroll",
            Self::SchemaTooNew { .. } => "doc-schema-too-new",
            Self::Constraint(ConstraintKind::DuplicateUnitAddress) => "duplicate-unit-address",
            Self::Constraint(ConstraintKind::DuplicateSerial) => "duplicate-serial",
            Self::Constraint(ConstraintKind::TerminalIdExhausted) => "terminal-id-exhausted",
            Self::Constraint(_) => "invalid-field",
            Self::Undecodable(_) => "doc-undecodable",
            Self::Store(_) => "store-error",
        }
    }

    /// Every stable code, for the docs drift test.
    pub const REASONS: [&'static str; 16] = [
        "conflict",
        "config-refused",
        "unknown-box-type",
        "unknown-revision",
        "unknown-object",
        "doc-too-large",
        "unit-retired",
        "wrong-kind",
        "sacrificial-at-enroll",
        "doc-schema-too-new",
        "duplicate-unit-address",
        "duplicate-serial",
        "terminal-id-exhausted",
        "invalid-field",
        "doc-undecodable",
        "store-error",
    ];
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { head } => {
                write!(f, "the object changed: the head is now revision {head}")
            }
            Self::Refused(r) => write!(f, "refused: {}", r.rules().join(", ")),
            Self::UnknownBoxType(t) => write!(
                f,
                "box type {t:?} is not in the upstream catalog; a new box type is added only \
                 through an upstream PR or a fork"
            ),
            Self::UnknownRevision { key, revision } => {
                write!(f, "{key} has no revision {revision}")
            }
            Self::UnknownObject(key) => write!(f, "no such object {key}"),
            Self::DocTooLarge { bytes, max } => {
                write!(f, "the document is {bytes} bytes; the cap is {max}")
            }
            Self::Retired(id) => write!(f, "unit {} is retired", id.get()),
            Self::WrongKind(s) => f.write_str(s),
            Self::SacrificialAtEnroll => f.write_str(
                "a unit is enrolled non-sacrificial; mark it with an update (owner plus token)",
            ),
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "a stored document is at schema {found}; this build reads up to {supported}"
            ),
            Self::Constraint(k) => write!(f, "constraint violated: {k:?}"),
            Self::Undecodable(s) => write!(f, "a stored document does not decode: {s}"),
            Self::Store(e) => write!(f, "store error: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(e) => Some(e),
            _ => None,
        }
    }
}

impl From<StoreError> for ConfigError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Constraint(k) => Self::Constraint(k),
            e => Self::Store(e),
        }
    }
}

impl From<UpgradeError> for ConfigError {
    fn from(e: UpgradeError) -> Self {
        match e {
            UpgradeError::TooNew { found, supported } => Self::SchemaTooNew { found, supported },
            UpgradeError::Failed(s) => Self::Undecodable(s),
        }
    }
}

/// Who a journal row names.
#[derive(Debug, Clone)]
pub(crate) struct Actor {
    pub(crate) actor: String,
    pub(crate) role: String,
    pub(crate) front_door: String,
}

impl Actor {
    /// The caller, labelled as the policy layer labels it.
    pub(crate) fn of(ctx: &CommandContext) -> Self {
        Self {
            actor: ctx.actor.clone(),
            role: format!("{:?}", ctx.role),
            front_door: format!("{:?}", ctx.front_door),
        }
    }

    fn seed(bundle_sha: &str) -> Self {
        Self {
            actor: format!("seed:{}", &bundle_sha[..12]),
            role: "System".into(),
            front_door: "Seed".into(),
        }
    }
}

/// A committed (or unchanged) change: what the applier gets.
#[derive(Debug, Clone)]
pub(crate) struct Written {
    pub(crate) key: ObjectKey,
    pub(crate) saved: Saved,
    pub(crate) doc: Doc,
}

/// The current singleton heads, as the candidate starts.
#[derive(Debug, Clone, Default)]
struct Heads {
    lineup: Option<Lineup>,
    pid_plan: Option<PidPlan>,
    class_budgets: Option<ClassBudgets>,
    mux_settings: Option<MuxSettings>,
}

impl Heads {
    fn set(&mut self, doc: Doc) {
        match doc {
            Doc::Lineup(d) => self.lineup = Some(d),
            Doc::PidPlan(d) => self.pid_plan = Some(d),
            Doc::ClassBudgets(d) => self.class_budgets = Some(d),
            Doc::MuxSettings(d) => self.mux_settings = Some(d),
            // Units are in the unit view, not here.
            Doc::Unit(_) => {}
        }
    }

    fn get(&self, kind: ConfigKind) -> Option<Doc> {
        match kind {
            ConfigKind::Lineup => self.lineup.clone().map(Doc::Lineup),
            ConfigKind::PidPlan => self.pid_plan.clone().map(Doc::PidPlan),
            ConfigKind::ClassBudgets => self.class_budgets.clone().map(Doc::ClassBudgets),
            ConfigKind::MuxSettings => self.mux_settings.clone().map(Doc::MuxSettings),
            ConfigKind::Unit => None,
        }
    }

    fn snapshot<'a>(
        &'a self,
        units: &'a Units,
        catalog: &'a dyn BoxTypeCatalog,
    ) -> ConfigSnapshot<'a> {
        ConfigSnapshot {
            lineup: self.lineup.as_ref(),
            pid_plan: self.pid_plan.as_ref(),
            class_budgets: self.class_budgets.as_ref(),
            mux_settings: self.mux_settings.as_ref(),
            units,
            catalog,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// Fail after the revision row is inserted, before the event.
    AfterRevision,
}

/// One singleton at boot: its key and, when it has a head, the revision,
/// the document and today's verdict on it.
pub(crate) type BootRow = (ObjectKey, Option<(u32, Doc, Result<(), Refusal>)>);

/// One row read for a change: the head's metadata and stored text.
type HeadRow = Option<(RevisionMeta, String)>;

/// The configuration service: every write to configuration, inside a
/// caller's store transaction. Crate-internal: the public path is
/// `Command::Admin` through the command bus.
pub(crate) struct ConfigService {
    validator: Arc<dyn ConfigValidator>,
    catalog: Arc<dyn BoxTypeCatalog>,
    limits: ConfigLimits,
    schema: fn(ConfigKind) -> u32,
    upgrades: &'static [Upgrade],
    #[cfg(test)]
    fault: Option<Fault>,
}

fn check_base(head: Option<u32>, base: BaseRevision) -> Result<(), ConfigError> {
    match (head, base) {
        (None, BaseRevision::None) => Ok(()),
        (Some(h), BaseRevision::At(b)) if h == b => Ok(()),
        (h, _) => Err(ConfigError::Conflict {
            head: h.unwrap_or(0),
        }),
    }
}

fn singleton_checks(key: ObjectKey, doc: &Doc) -> Result<(), ConfigError> {
    if key.kind == ConfigKind::Unit {
        return Err(ConfigError::WrongKind(
            "a unit changes through UnitUpdate (its terminal row is the head)".into(),
        ));
    }
    if !key.is_well_formed() || doc.kind() != key.kind {
        return Err(ConfigError::WrongKind(format!(
            "a {} document cannot be saved as {key}",
            doc.kind()
        )));
    }
    Ok(())
}

fn retired_json(retired: bool) -> String {
    format!("{{\"retired\":{retired}}}")
}

impl ConfigService {
    pub(crate) fn new(
        validator: Arc<dyn ConfigValidator>,
        catalog: Arc<dyn BoxTypeCatalog>,
        limits: ConfigLimits,
    ) -> Self {
        Self {
            validator,
            catalog,
            limits,
            schema: doc::doc_schema,
            upgrades: doc::UPGRADES,
            #[cfg(test)]
            fault: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_schema(
        mut self,
        schema: fn(ConfigKind) -> u32,
        upgrades: &'static [Upgrade],
    ) -> Self {
        self.schema = schema;
        self.upgrades = upgrades;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_fault(mut self, fault: Fault) -> Self {
        self.fault = Some(fault);
        self
    }

    pub(crate) fn catalog(&self) -> &dyn BoxTypeCatalog {
        &*self.catalog
    }

    /// A stored document, upgraded to the current schema in memory.
    fn load(&self, meta: &RevisionMeta, json: &str) -> Result<Doc, ConfigError> {
        let kind = meta.key.kind;
        Ok(doc::upgrade(
            kind,
            meta.doc_schema,
            (self.schema)(kind),
            json,
            self.upgrades,
        )?)
    }

    fn heads(&self, tx: &Tx<'_>) -> Result<Heads, ConfigError> {
        let mut h = Heads::default();
        for kind in ConfigKind::SINGLETONS {
            if let Some((meta, json)) = persist::head(tx.conn(), ObjectKey::singleton(kind))? {
                h.set(self.load(&meta, &json)?);
            }
        }
        Ok(h)
    }

    fn validate(
        &self,
        heads: &Heads,
        units: &Units,
        change: &Change<'_>,
    ) -> Result<(), ConfigError> {
        self.validator
            .validate(&heads.snapshot(units, &*self.catalog), change)
            .map_err(ConfigError::Refused)
    }

    /// Canonical JSON and digest, under the size cap.
    fn encode(&self, doc: &Doc) -> Result<(String, String), ConfigError> {
        let json = doc.canonical_json();
        if json.len() > self.limits.max_doc_bytes {
            return Err(ConfigError::DocTooLarge {
                bytes: json.len(),
                max: self.limits.max_doc_bytes,
            });
        }
        let sha = doc::sha256_hex(json.as_bytes());
        Ok((json, sha))
    }

    fn require_box_type(&self, d: &UnitDoc) -> Result<(), ConfigError> {
        if self.catalog.contains(&d.box_type) {
            Ok(())
        } else {
            Err(ConfigError::UnknownBoxType(d.box_type.clone()))
        }
    }

    /// The journal row, the revision row and the `ConfigSaved` event, in
    /// the caller's transaction. Returns the new revision number.
    #[allow(clippy::too_many_arguments)]
    fn commit(
        &self,
        tx: &Tx<'_>,
        who: &Actor,
        key: ObjectKey,
        prior: Option<u32>,
        before: Option<String>,
        (json, sha): (&str, &str),
        origin: Origin,
        rolled_back_to: Option<u32>,
        command: &str,
    ) -> Result<u32, ConfigError> {
        let revision = prior
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| ConfigError::Store(StoreError::Corrupt("revision overflow".into())))?;
        let audit_seq = tx.append_audit(&AuditEntry {
            actor: who.actor.clone(),
            role: who.role.clone(),
            front_door: who.front_door.clone(),
            command: command.to_owned(),
            target: Some(key.target(revision)),
            outcome: "ok".to_owned(),
            before,
            after: Some(json.to_owned()),
        })?;
        persist::insert(
            tx.conn(),
            &NewRevision {
                key,
                revision,
                doc: json,
                sha,
                schema: (self.schema)(key.kind),
                origin,
                rolled_back_to,
                actor: &who.actor,
                audit_seq,
            },
        )?;
        #[cfg(test)]
        if self.fault == Some(Fault::AfterRevision) {
            return Err(ConfigError::Store(StoreError::Corrupt(
                "injected failure after the revision insert".into(),
            )));
        }
        tx.append_event(
            &who.actor,
            &Event::ConfigSaved {
                config_kind: key.kind.as_str().to_owned(),
                object_id: key.object_id.map(TerminalId::get),
                revision,
            },
        )?;
        Ok(revision)
    }

    /// Saves a singleton document.
    pub(crate) fn save(
        &self,
        tx: &Tx<'_>,
        ctx: &CommandContext,
        key: ObjectKey,
        doc: &Doc,
        base: BaseRevision,
    ) -> Result<Written, ConfigError> {
        singleton_checks(key, doc)?;
        let head = persist::head(tx.conn(), key)?;
        check_base(head.as_ref().map(|(m, _)| m.revision), base)?;
        let enc = self.encode(doc)?;
        if let Some((m, _)) = &head
            && m.doc_sha256 == enc.1
        {
            return Ok(Written {
                key,
                saved: Saved::Unchanged {
                    revision: m.revision,
                },
                doc: doc.clone(),
            });
        }
        let mut heads = self.heads(tx)?;
        heads.set(doc.clone());
        let units = Units::load(tx.conn())?;
        self.validate(&heads, &units, &Change::Save { key, doc })?;
        let (prior, before) = split(head);
        let revision = self.commit(
            tx,
            &Actor::of(ctx),
            key,
            prior,
            before,
            (&enc.0, &enc.1),
            Origin::Save,
            None,
            "config.save",
        )?;
        Ok(Written {
            key,
            saved: Saved::Revised { revision },
            doc: doc.clone(),
        })
    }

    /// Saves several singletons, validated as one candidate, in one
    /// transaction. Unchanged documents write nothing.
    pub(crate) fn save_batch(
        &self,
        tx: &Tx<'_>,
        ctx: &CommandContext,
        changes: &[SaveChange],
    ) -> Result<Vec<Written>, ConfigError> {
        let mut keys = BTreeSet::new();
        let mut pending = Vec::new();
        let mut out = Vec::new();
        for c in changes {
            singleton_checks(c.key, &c.doc)?;
            if !keys.insert(c.key) {
                return Err(ConfigError::WrongKind(format!(
                    "{} appears twice in one batch",
                    c.key
                )));
            }
            let head = persist::head(tx.conn(), c.key)?;
            check_base(head.as_ref().map(|(m, _)| m.revision), c.base)?;
            let enc = self.encode(&c.doc)?;
            match &head {
                Some((m, _)) if m.doc_sha256 == enc.1 => out.push(Written {
                    key: c.key,
                    saved: Saved::Unchanged {
                        revision: m.revision,
                    },
                    doc: c.doc.clone(),
                }),
                _ => pending.push((c, head, enc)),
            }
        }
        if pending.is_empty() {
            return Ok(out);
        }
        let mut heads = self.heads(tx)?;
        let changed: Vec<(ObjectKey, Doc)> = pending
            .iter()
            .map(|(c, _, _)| (c.key, c.doc.clone()))
            .collect();
        for (_, d) in &changed {
            heads.set(d.clone());
        }
        let units = Units::load(tx.conn())?;
        self.validate(&heads, &units, &Change::Batch(&changed))?;
        let who = Actor::of(ctx);
        for (c, head, enc) in pending {
            let (prior, before) = split(head);
            let revision = self.commit(
                tx,
                &who,
                c.key,
                prior,
                before,
                (&enc.0, &enc.1),
                Origin::Save,
                None,
                "config.save",
            )?;
            out.push(Written {
                key: c.key,
                saved: Saved::Revised { revision },
                doc: c.doc.clone(),
            });
        }
        Ok(out)
    }

    /// Rolls `key` back to revision `to`: a validated save of that
    /// revision's document (upgraded in memory if it is at an older schema).
    pub(crate) fn rollback(
        &self,
        tx: &Tx<'_>,
        ctx: &CommandContext,
        key: ObjectKey,
        to: u32,
        base: BaseRevision,
    ) -> Result<Written, ConfigError> {
        if !key.is_well_formed() {
            return Err(ConfigError::WrongKind(format!(
                "{key} is not a well-formed key"
            )));
        }
        let (target, json) = persist::revision(tx.conn(), key, to)?
            .ok_or(ConfigError::UnknownRevision { key, revision: to })?;
        let doc = self.load(&target, &json)?;
        let head = persist::head(tx.conn(), key)?;
        check_base(head.as_ref().map(|(m, _)| m.revision), base)?;
        let enc = self.encode(&doc)?;
        if let Some((m, _)) = &head
            && m.doc_sha256 == enc.1
        {
            return Ok(Written {
                key,
                saved: Saved::Unchanged {
                    revision: m.revision,
                },
                doc,
            });
        }
        let mut heads = self.heads(tx)?;
        let mut units = Units::load(tx.conn())?;
        if let (Some(id), Doc::Unit(u)) = (key.object_id, &doc) {
            match units.map.get_mut(&id) {
                None => return Err(ConfigError::UnknownObject(key)),
                Some((_, true)) => return Err(ConfigError::Retired(id)),
                Some((slot, false)) => *slot = u.clone(),
            }
            self.require_box_type(u)?;
        } else {
            heads.set(doc.clone());
        }
        self.validate(&heads, &units, &Change::Rollback { key, to, doc: &doc })?;
        if let (Some(id), Doc::Unit(u)) = (key.object_id, &doc) {
            persist::write_unit_row(tx.conn(), id, u)?;
        }
        let (prior, before) = split(head);
        let revision = self.commit(
            tx,
            &Actor::of(ctx),
            key,
            prior,
            before,
            (&enc.0, &enc.1),
            Origin::Rollback,
            Some(to),
            "config.rollback",
        )?;
        Ok(Written {
            key,
            saved: Saved::Revised { revision },
            doc,
        })
    }

    /// Enrolls a unit (the Define Terminal record, WireLink 660 "Add New
    /// Set-Top"): the terminal row, its event, revision 1 and the journal
    /// row, in one transaction. This is a store record, not a claim that
    /// the box has it (the on-air Unit Creation is a separate command).
    pub(crate) fn enroll_unit(
        &self,
        tx: &Tx<'_>,
        ctx: &CommandContext,
        new: &NewUnit,
    ) -> Result<(TerminalId, Written), ConfigError> {
        if new.doc.sacrificial {
            return Err(ConfigError::SacrificialAtEnroll);
        }
        self.require_box_type(&new.doc)?;
        let doc = Doc::Unit(new.doc.clone());
        let enc = self.encode(&doc)?;
        let heads = self.heads(tx)?;
        let units = Units::load(tx.conn())?;
        // The validator first, so a bad field is refused by its rule name;
        // the store's own domain check stays as the backstop.
        self.validate(&heads, &units, &Change::UnitEnroll(new))?;
        let record = new
            .record()
            .map_err(|s| ConfigError::Constraint(ConstraintKind::Invalid(s)))?;
        record.validate()?;
        let who = Actor::of(ctx);
        let id = tx.create_terminal(&who.actor, &record)?;
        let key = ObjectKey::unit(id);
        let revision = self.commit(
            tx,
            &who,
            key,
            None,
            None,
            (&enc.0, &enc.1),
            Origin::Save,
            None,
            "unit.enroll",
        )?;
        Ok((
            id,
            Written {
                key,
                saved: Saved::Revised { revision },
                doc,
            },
        ))
    }

    /// The stored record of `id`, refusing an unknown or retired unit.
    fn live_unit(tx: &Tx<'_>, id: TerminalId) -> Result<crate::store::Terminal, ConfigError> {
        let t = crate::store::terminal_of(tx.conn(), id)?
            .ok_or(ConfigError::UnknownObject(ObjectKey::unit(id)))?;
        if t.retired_utc.is_some() {
            return Err(ConfigError::Retired(id));
        }
        Ok(t)
    }

    /// Changes a unit's operational record. Identity is not in the
    /// document, so it cannot change.
    pub(crate) fn update_unit(
        &self,
        tx: &Tx<'_>,
        ctx: &CommandContext,
        id: TerminalId,
        new: &UnitDoc,
        base: BaseRevision,
    ) -> Result<Written, ConfigError> {
        let key = ObjectKey::unit(id);
        let row = Self::live_unit(tx, id)?;
        let head = persist::head(tx.conn(), key)?;
        check_base(head.as_ref().map(|(m, _)| m.revision), base)?;
        self.require_box_type(new)?;
        let doc = Doc::Unit(new.clone());
        let enc = self.encode(&doc)?;
        if let Some((m, _)) = &head
            && m.doc_sha256 == enc.1
        {
            return Ok(Written {
                key,
                saved: Saved::Unchanged {
                    revision: m.revision,
                },
                doc,
            });
        }
        let heads = self.heads(tx)?;
        let mut units = Units::load(tx.conn())?;
        units.map.insert(id, (new.clone(), false));
        self.validate(&heads, &units, &Change::UnitUpdate { id, doc: new })?;
        // The store's domain check, as the backstop behind the validator.
        let candidate = NewUnit {
            unit_address: row.record.unit_address,
            serial: row.record.serial.clone(),
            doc: new.clone(),
        };
        let mut record: NewTerminal = candidate
            .record()
            .map_err(|s| ConfigError::Constraint(ConstraintKind::Invalid(s)))?;
        record.auto_registration_date = row.record.auto_registration_date.clone();
        record.validate()?;
        persist::write_unit_row(tx.conn(), id, new)?;
        let (prior, before) = match head {
            Some((m, json)) => (Some(m.revision), Some(json)),
            // A row from before #25 (tests only): journal the row itself.
            None => (
                None,
                Some(doc::canonical(&UnitDoc::from_record(&row.record))),
            ),
        };
        let revision = self.commit(
            tx,
            &Actor::of(ctx),
            key,
            prior,
            before,
            (&enc.0, &enc.1),
            Origin::Save,
            None,
            "unit.update",
        )?;
        Ok(Written {
            key,
            saved: Saved::Revised { revision },
            doc,
        })
    }

    /// Retires (`retire = true`) or reinstates a unit. Never deletes; the
    /// operational record and its revisions are untouched. Returns whether
    /// anything changed.
    pub(crate) fn set_retired(
        &self,
        tx: &Tx<'_>,
        ctx: &CommandContext,
        id: TerminalId,
        retire: bool,
    ) -> Result<bool, ConfigError> {
        let row = crate::store::terminal_of(tx.conn(), id)?
            .ok_or(ConfigError::UnknownObject(ObjectKey::unit(id)))?;
        let was = row.retired_utc.is_some();
        if was == retire {
            return Ok(false);
        }
        let heads = self.heads(tx)?;
        let mut units = Units::load(tx.conn())?;
        if let Some(slot) = units.map.get_mut(&id) {
            slot.1 = retire;
        }
        let change = if retire {
            Change::UnitRetire(id)
        } else {
            Change::UnitReinstate(id)
        };
        self.validate(&heads, &units, &change)?;
        persist::set_retired(tx.conn(), id, retire)?;
        let who = Actor::of(ctx);
        tx.append_audit(&AuditEntry {
            actor: who.actor,
            role: who.role,
            front_door: who.front_door,
            command: if retire {
                "unit.retire"
            } else {
                "unit.reinstate"
            }
            .to_owned(),
            target: Some(ObjectKey::unit(id).to_string()),
            outcome: "ok".to_owned(),
            before: Some(retired_json(was)),
            after: Some(retired_json(retire)),
        })?;
        Ok(true)
    }

    /// Seeds the four singletons from `bundle` when **none** of them has a
    /// revision; otherwise does nothing. Validated as one candidate.
    pub(crate) fn seed_if_empty(
        &self,
        tx: &Tx<'_>,
        bundle: &SeedBundle,
    ) -> Result<SeedReport, ConfigError> {
        for kind in ConfigKind::SINGLETONS {
            if persist::count(tx.conn(), kind)? > 0 {
                return Ok(SeedReport::AlreadySeeded);
            }
        }
        let docs = bundle.docs();
        let mut heads = Heads::default();
        let mut encoded = Vec::with_capacity(docs.len());
        for (key, doc) in &docs {
            heads.set(doc.clone());
            encoded.push((*key, self.encode(doc)?));
        }
        let units = Units::load(tx.conn())?;
        self.validate(&heads, &units, &Change::Seed(bundle))?;
        let who = Actor::seed(&bundle.sha256());
        let mut keys = Vec::new();
        for (key, enc) in encoded {
            self.commit(
                tx,
                &who,
                key,
                None,
                None,
                (&enc.0, &enc.1),
                Origin::Seed,
                None,
                "config.seed",
            )?;
            keys.push(key);
        }
        Ok(SeedReport::Seeded {
            actor: who.actor,
            keys,
        })
    }

    /// Each singleton head and whether the validator accepts it today
    /// (boot-time apply: a guardrail tightened after a save must not crash
    /// the daemon).
    pub(crate) fn boot_heads(&self, tx: &Tx<'_>) -> Result<Vec<BootRow>, ConfigError> {
        let heads = self.heads(tx)?;
        let units = Units::load(tx.conn())?;
        let mut out = Vec::new();
        for kind in ConfigKind::SINGLETONS {
            let key = ObjectKey::singleton(kind);
            let Some((meta, _)) = persist::head(tx.conn(), key)? else {
                out.push((key, None));
                continue;
            };
            let Some(doc) = heads.get(kind) else {
                out.push((key, None));
                continue;
            };
            let verdict = self.validator.validate(
                &heads.snapshot(&units, &*self.catalog),
                &Change::Save { key, doc: &doc },
            );
            out.push((key, Some((meta.revision, doc, verdict))));
        }
        Ok(out)
    }
}

fn split(head: HeadRow) -> (Option<u32>, Option<String>) {
    match head {
        Some((m, json)) => (Some(m.revision), Some(json)),
        None => (None, None),
    }
}
