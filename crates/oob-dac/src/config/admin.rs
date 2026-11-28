//! `Command::Admin`: configuration changes through the command bus.
//!
//! [`AdminOp`] is what an adapter submits as `Command::Admin`. The bus runs
//! the policy check first (owner role, guest refusal, the
//! `mark-sacrificial` token where a unit becomes sacrificial), then hands
//! the op to [`ConfigAdmin`], which runs one store transaction and, after
//! it commits, the [`ConfigApplier`].
//!
//! **Naming trap.** [`AdminOp::UnitEnroll`] is the Define Terminal *record*
//! (WireLink 660 "Add New Set-Top": a store write). The on-air Unit Creation
//! message that creates the unit **in the box** is a separate terminal
//! command. Enrollment order stays factory-init → unit-creation → connect
//! (r4 F4-4); a record in the store is the precondition for targeting a
//! box, not a claim that the box has it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::command::CommandContext;
use crate::store::{AuditEntry, StoreError, StoreHandle, TerminalId, Tx};

use super::apply::{ApplyStatus, ConfigApplier};
use super::catalog::BoxTypeCatalog;
use super::check::{ConfigValidator, NewUnit, Refusal};
use super::doc::{self, Doc, UnitDoc};
use super::key::{BaseRevision, ConfigKind, ObjectKey};
use super::seed::{SeedBundle, SeedReport};
use super::{Actor, ConfigError, ConfigLimits, ConfigService, Saved, Written, persist};

/// One singleton save inside a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveChange {
    /// The singleton.
    pub key: ObjectKey,
    /// Its new document.
    pub doc: Doc,
    /// The head the caller edited.
    pub base: BaseRevision,
}

/// A configuration change, submitted as `Command::Admin`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AdminOp {
    /// Save a singleton document.
    ConfigSave {
        /// The singleton.
        key: ObjectKey,
        /// Its new document.
        doc: Doc,
        /// The head the caller edited.
        base: BaseRevision,
    },
    /// Save several singletons atomically, validated as one candidate.
    ConfigSaveBatch(Vec<SaveChange>),
    /// Roll an object (a singleton or a unit) back to revision `to`.
    ConfigRollback {
        /// The object.
        key: ObjectKey,
        /// The revision whose document is restored.
        to: u32,
        /// The head the caller saw.
        base: BaseRevision,
    },
    /// Enroll a unit (the Define Terminal record).
    UnitEnroll(NewUnit),
    /// Change a unit's operational record.
    UnitUpdate {
        /// The unit.
        id: TerminalId,
        /// Its new record.
        doc: UnitDoc,
        /// The head the caller edited.
        base: BaseRevision,
    },
    /// Retire a unit (never deleted; out of targeting).
    UnitRetire(TerminalId),
    /// Return a retired unit to service.
    UnitReinstate(TerminalId),
}

impl AdminOp {
    /// The stable operation name for the bus audit row.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ConfigSave { .. } => "config-save",
            Self::ConfigSaveBatch(_) => "config-save-batch",
            Self::ConfigRollback { .. } => "config-rollback",
            Self::UnitEnroll(_) => "unit-enroll",
            Self::UnitUpdate { .. } => "unit-update",
            Self::UnitRetire(_) => "unit-retire",
            Self::UnitReinstate(_) => "unit-reinstate",
        }
    }

    /// What it targets, for audit: a key or `unit/<terminal id>`; never a
    /// unit address or serial.
    pub fn target(&self) -> Option<String> {
        Some(match self {
            Self::ConfigSave { key, .. } | Self::ConfigRollback { key, .. } => key.to_string(),
            Self::ConfigSaveBatch(c) => {
                let keys: Vec<String> = c.iter().map(|c| c.key.to_string()).collect();
                format!("batch:{}", keys.join(","))
            }
            Self::UnitEnroll(_) => "unit/new".to_owned(),
            Self::UnitUpdate { id, .. } | Self::UnitRetire(id) | Self::UnitReinstate(id) => {
                ObjectKey::unit(*id).to_string()
            }
        })
    }

    fn journal_command(&self) -> &'static str {
        match self {
            Self::ConfigSave { .. } | Self::ConfigSaveBatch(_) => "config.save",
            Self::ConfigRollback { .. } => "config.rollback",
            Self::UnitEnroll(_) => "unit.enroll",
            Self::UnitUpdate { .. } => "unit.update",
            Self::UnitRetire(_) => "unit.retire",
            Self::UnitReinstate(_) => "unit.reinstate",
        }
    }

    /// The proposal as JSON, for a refusal's journal row (no identity).
    fn proposal(&self) -> String {
        match self {
            Self::ConfigSave { doc, .. } => doc.canonical_json(),
            Self::ConfigSaveBatch(c) => {
                let items: Vec<serde_json::Value> = c
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "key": c.key.to_string(),
                            "doc": serde_json::from_str::<serde_json::Value>(&c.doc.canonical_json())
                                .unwrap_or(serde_json::Value::Null),
                        })
                    })
                    .collect();
                doc::canonical(&items)
            }
            Self::ConfigRollback { to, .. } => format!("{{\"rollback_to\":{to}}}"),
            Self::UnitEnroll(u) => doc::canonical(&u.doc),
            Self::UnitUpdate { doc: d, .. } => doc::canonical(d),
            Self::UnitRetire(_) => "{\"retired\":true}".to_owned(),
            Self::UnitReinstate(_) => "{\"retired\":false}".to_owned(),
        }
    }
}

/// One saved object and what applying it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedItem {
    /// The object.
    pub key: ObjectKey,
    /// Revised or unchanged, with the head.
    pub saved: Saved,
    /// The applier's result; `None` when nothing changed (not applied).
    pub applied: Option<ApplyStatus>,
}

/// What a `Command::Admin` did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdminOutcome {
    /// A save or rollback.
    Saved(SavedItem),
    /// A batch, in submission order of the changed and unchanged items.
    Batch(Vec<SavedItem>),
    /// A unit was enrolled.
    Enrolled {
        /// Its store-allocated terminal ID.
        id: TerminalId,
        /// Its revision 1.
        item: SavedItem,
    },
    /// A unit was retired or reinstated.
    UnitState {
        /// The unit.
        id: TerminalId,
        /// Whether it is retired now.
        retired: bool,
        /// Whether anything changed.
        changed: bool,
    },
}

/// A singleton's state at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootStatus {
    /// No revision yet (a fresh store before seeding).
    Unconfigured,
    /// The head fails today's validation (a guardrail tightened after it
    /// was saved). It was **not** applied; the daemon serves nothing for
    /// that subsystem, reports it in health and metrics, and waits for the
    /// owner to save a fix. Never a crash.
    Invalid(Refusal),
    /// The head was handed to the applier.
    Applied(ApplyStatus),
}

/// One singleton's boot result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootHead {
    /// The singleton.
    pub key: ObjectKey,
    /// Its head revision, if any.
    pub revision: Option<u32>,
    /// What happened.
    pub status: BootStatus,
}

/// The configuration store's runtime: the daemon (#29) builds one and
/// hands it to the bus (`CommandBus::with_config_admin`). Its public
/// methods are bootstrap only; every change goes through the bus so policy
/// and audit apply.
///
/// Like the policy layer it reaches the store with
/// [`StoreHandle::call_blocking`], so it must not be called from an async
/// runtime thread.
pub struct ConfigAdmin {
    store: StoreHandle,
    service: Arc<ConfigService>,
    applier: Arc<dyn ConfigApplier>,
    journal_failures: AtomicU64,
}

impl std::fmt::Debug for ConfigAdmin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigAdmin").finish_non_exhaustive()
    }
}

impl ConfigAdmin {
    /// The runtime over `store`. The validator is mandatory and has no
    /// default (#26 supplies the real one).
    pub fn new(
        store: StoreHandle,
        validator: Arc<dyn ConfigValidator>,
        catalog: Arc<dyn BoxTypeCatalog>,
        applier: Arc<dyn ConfigApplier>,
        limits: ConfigLimits,
    ) -> Self {
        Self::from_service(
            store,
            ConfigService::new(validator, catalog, limits),
            applier,
        )
    }

    pub(crate) fn from_service(
        store: StoreHandle,
        service: ConfigService,
        applier: Arc<dyn ConfigApplier>,
    ) -> Self {
        Self {
            store,
            service: Arc::new(service),
            applier,
            journal_failures: AtomicU64::new(0),
        }
    }

    /// The digest of the box-type catalog in force (logged at start and put
    /// in export manifests, #24).
    pub fn catalog_digest(&self) -> String {
        self.service.catalog().digest()
    }

    /// How many refusal journal rows could not be written (the bus audit
    /// row still records the refusal).
    pub fn journal_failures(&self) -> u64 {
        self.journal_failures.load(Ordering::Relaxed)
    }

    fn txn<T: Send + 'static>(
        &self,
        f: impl FnOnce(&ConfigService, &Tx<'_>) -> Result<T, ConfigError> + Send + 'static,
    ) -> Result<T, ConfigError> {
        let svc = Arc::clone(&self.service);
        self.store
            .call_blocking(move |s| Ok(s.write_with(|tx| f(&svc, tx))))
            .map_err(ConfigError::from)?
    }

    /// The first-run seed: writes revision 1 of each singleton from
    /// `bundle` when none of them has a revision, and nothing otherwise.
    /// Bootstrap only (the daemon's `--seed`, #29); journaled with actor
    /// `seed:<12 hex of the bundle digest>`.
    ///
    /// # Errors
    ///
    /// [`ConfigError`]; a refusal writes nothing.
    pub fn seed_if_empty(&self, bundle: &SeedBundle) -> Result<SeedReport, ConfigError> {
        let bundle = bundle.clone();
        self.txn(move |svc, tx| svc.seed_if_empty(tx, &bundle))
    }

    /// Boot-time apply: every singleton head that today's validator
    /// accepts is handed to the applier; one it refuses is reported
    /// [`BootStatus::Invalid`] and not applied.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] if the store cannot be read.
    pub fn boot(&self) -> Result<Vec<BootHead>, ConfigError> {
        let heads = self.txn(|svc, tx| svc.boot_heads(tx))?;
        Ok(heads
            .into_iter()
            .map(|(key, head)| match head {
                None => BootHead {
                    key,
                    revision: None,
                    status: BootStatus::Unconfigured,
                },
                Some((revision, _, Err(refusal))) => BootHead {
                    key,
                    revision: Some(revision),
                    status: BootStatus::Invalid(refusal),
                },
                Some((revision, doc, Ok(()))) => BootHead {
                    key,
                    revision: Some(revision),
                    status: BootStatus::Applied(self.applier.apply(key, &doc, revision)),
                },
            })
            .collect())
    }

    /// Runs after commit only.
    fn item(&self, w: &Written) -> SavedItem {
        let applied = match w.saved {
            Saved::Revised { revision } => Some(self.applier.apply(w.key, &w.doc, revision)),
            Saved::Unchanged { .. } => None,
        };
        SavedItem {
            key: w.key,
            saved: w.saved,
            applied,
        }
    }

    /// Runs one admin op. Called by the bus after the policy check.
    pub(crate) fn run(
        &self,
        ctx: &CommandContext,
        op: &AdminOp,
    ) -> Result<AdminOutcome, ConfigError> {
        let c = ctx.clone();
        let result = match op.clone() {
            AdminOp::ConfigSave { key, doc, base } => self
                .txn(move |s, tx| s.save(tx, &c, key, &doc, base))
                .map(|w| AdminOutcome::Saved(self.item(&w))),
            AdminOp::ConfigSaveBatch(changes) => self
                .txn(move |s, tx| s.save_batch(tx, &c, &changes))
                .map(|ws| AdminOutcome::Batch(ws.iter().map(|w| self.item(w)).collect())),
            AdminOp::ConfigRollback { key, to, base } => self
                .txn(move |s, tx| s.rollback(tx, &c, key, to, base))
                .map(|w| AdminOutcome::Saved(self.item(&w))),
            AdminOp::UnitEnroll(new) => {
                self.txn(move |s, tx| s.enroll_unit(tx, &c, &new))
                    .map(|(id, w)| AdminOutcome::Enrolled {
                        id,
                        item: self.item(&w),
                    })
            }
            AdminOp::UnitUpdate { id, doc, base } => self
                .txn(move |s, tx| s.update_unit(tx, &c, id, &doc, base))
                .map(|w| AdminOutcome::Saved(self.item(&w))),
            AdminOp::UnitRetire(id) => {
                self.txn(move |s, tx| s.set_retired(tx, &c, id, true))
                    .map(|changed| AdminOutcome::UnitState {
                        id,
                        retired: true,
                        changed,
                    })
            }
            AdminOp::UnitReinstate(id) => self
                .txn(move |s, tx| s.set_retired(tx, &c, id, false))
                .map(|changed| AdminOutcome::UnitState {
                    id,
                    retired: false,
                    changed,
                }),
        };
        if let Err(ConfigError::Refused(r)) = &result {
            self.journal_refusal(ctx, op, r);
        }
        result
    }

    /// A refusal stores nothing, so its journal row is written in its own
    /// transaction: the target, the rules, and the proposal it refused.
    fn journal_refusal(&self, ctx: &CommandContext, op: &AdminOp, r: &Refusal) {
        let who = Actor::of(ctx);
        let entry = AuditEntry {
            actor: who.actor,
            role: who.role,
            front_door: who.front_door,
            command: op.journal_command().to_owned(),
            target: op.target(),
            outcome: format!("refused:{}", r.rules().join(",")),
            before: None,
            after: Some(op.proposal()),
        };
        if self
            .store
            .call_blocking(move |s| s.write(|tx| tx.append_audit(&entry).map(|_| ())))
            .is_err()
        {
            self.journal_failures.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn unit_doc_at(tx: &Tx<'_>, id: TerminalId, n: u32) -> Result<Option<UnitDoc>, StoreError> {
    let Some((meta, json)) = persist::revision(tx.conn(), ObjectKey::unit(id), n)? else {
        return Ok(None);
    };
    let kind = ConfigKind::Unit;
    Ok(doc::upgrade(
        kind,
        meta.doc_schema,
        doc::doc_schema(kind),
        &json,
        doc::UPGRADES,
    )
    .ok()
    .and_then(|d| d.as_unit().cloned()))
}

/// Whether the base revision the caller edited already had the unit
/// sacrificial. Unknown (no base, missing or undecodable) reads as `false`,
/// so the policy asks for a token: fail closed.
fn base_sacrificial(tx: &Tx<'_>, id: TerminalId, base: BaseRevision) -> Result<bool, StoreError> {
    Ok(match base {
        BaseRevision::None => false,
        BaseRevision::At(n) => unit_doc_at(tx, id, n)?.is_some_and(|d| d.sacrificial),
    })
}

/// The unit an op would turn sacrificial, if any (the `mark-sacrificial`
/// D-hard class, #23). Decided against the **base** revision, which is
/// immutable history; the save itself refuses a base that is no longer the
/// head, so the transition the policy priced is the one that commits.
pub(crate) fn marks_sacrificial(
    tx: &Tx<'_>,
    op: &AdminOp,
) -> Result<Option<TerminalId>, StoreError> {
    match op {
        AdminOp::UnitUpdate { id, doc, base } if doc.sacrificial => {
            Ok((!base_sacrificial(tx, *id, *base)?).then_some(*id))
        }
        AdminOp::ConfigRollback {
            key:
                ObjectKey {
                    kind: ConfigKind::Unit,
                    object_id: Some(id),
                },
            to,
            base,
        } => {
            // An undecodable target fails closed: it may be sacrificial.
            let target = unit_doc_at(tx, *id, *to)?.is_none_or(|d| d.sacrificial);
            Ok((target && !base_sacrificial(tx, *id, *base)?).then_some(*id))
        }
        _ => Ok(None),
    }
}
