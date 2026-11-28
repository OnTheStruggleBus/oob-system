//! The consistency surface: what a validator sees and how it refuses.
//!
//! #25 defines the shapes; **#26 implements the rules** (`lineup-above-860mhz`,
//! `std-hrc-mismatch`, `emm-provider-id`, `c0-floor-zero`, …). Every save,
//! batch, rollback, enroll, update, retire, reinstate and the seed calls
//! [`ConfigValidator::validate`] before anything is written, and a refusal
//! stores nothing.
//!
//! - **The whole candidate, not just the changed document.** The
//!   [`ConfigSnapshot`] is the current heads with the change applied, so a
//!   `pid_plan` save can be refused because of `class_budgets`. An absent
//!   object (a fresh store before seeding) skips the rules that need it:
//!   incomplete is allowed, wrong is not.
//! - **`rule` is an opaque, stable kebab-case string** owned by the
//!   validator's catalog; it is also the API error code. There is no enum
//!   coupling between #25 and #26.
//! - **`hard`** marks an ADR-003 §5 guardrail. Both kinds block the save;
//!   the flag is for the Console's presentation and for tests. No override
//!   flag exists anywhere.
//! - **A validator is mandatory.** [`ConfigAdmin::new`](super::ConfigAdmin::new)
//!   takes one and has no default. #25 ships [`StructuralValidator`]; no
//!   validator that admits everything is compiled outside `#[cfg(test)]`.

use crate::store::{NewTerminal, TerminalId, TerminalType, UnitAddress};

use super::catalog::BoxTypeCatalog;
use super::doc::{ClassBudgets, Doc, Lineup, MuxSettings, PidPlan, UnitDoc};
use super::key::{ConfigKind, ObjectKey};
use super::seed::SeedBundle;

/// A read-only view of the candidate unit set.
pub trait UnitView {
    /// Every unit's terminal ID, ascending, retired ones included.
    fn ids(&self) -> Vec<TerminalId>;
    /// A unit's operational record.
    fn get(&self, id: TerminalId) -> Option<&UnitDoc>;
    /// Whether the unit is retired (in the candidate).
    fn is_retired(&self, id: TerminalId) -> bool;
}

/// The candidate configuration a change would produce.
pub struct ConfigSnapshot<'a> {
    /// The lineup, if any.
    pub lineup: Option<&'a Lineup>,
    /// The PID plan, if any.
    pub pid_plan: Option<&'a PidPlan>,
    /// The class budgets, if any.
    pub class_budgets: Option<&'a ClassBudgets>,
    /// The mux settings, if any.
    pub mux_settings: Option<&'a MuxSettings>,
    /// Every unit, with the change applied.
    pub units: &'a dyn UnitView,
    /// The box-type catalog the install shipped with.
    pub catalog: &'a dyn BoxTypeCatalog,
}

/// A new unit: its burned-in identity and its first operational record.
/// The terminal ID is allocated by the store, never supplied.
#[derive(Clone, PartialEq, Eq)]
pub struct NewUnit {
    /// Unit address (burned into NVRAM, printed on the unit).
    pub unit_address: UnitAddress,
    /// Serial number, 12 ASCII alphanumerics.
    pub serial: String,
    /// The operational record. `sacrificial` must be `false`: marking is a
    /// separate D-hard update.
    pub doc: UnitDoc,
}

impl std::fmt::Debug for NewUnit {
    // Unit addresses and serials are box-identifying (ADR-003 §4): they
    // never reach a log line through `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewUnit")
            .field("unit_address", &"<redacted>")
            .field("serial", &"<redacted>")
            .field("doc", &self.doc)
            .finish()
    }
}

impl NewUnit {
    /// The store record for this unit (no auto-registration date: that is
    /// system-set).
    pub(crate) fn record(&self) -> Result<NewTerminal, String> {
        let d = &self.doc;
        let terminal_type: TerminalType = d
            .terminal_type
            .parse()
            .map_err(|_| format!("terminal type {:?}", d.terminal_type))?;
        Ok(NewTerminal {
            unit_address: self.unit_address,
            serial: self.serial.clone(),
            terminal_type,
            box_type: d.box_type.clone(),
            account_number: d.account_number.clone(),
            on_plant: d.on_plant,
            install_date: d.install_date.clone(),
            auto_registration_date: None,
            activated: d.activated,
            autodiscovered: d.autodiscovered,
            ip_address: d.ip_address.clone(),
            bsi_name: d.bsi_name.clone(),
            downstream_plant: d.downstream_plant.clone(),
            upstream_plant: d.upstream_plant.clone(),
            sacrificial: d.sacrificial,
        })
    }
}

/// What is being changed. The candidate already includes it.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Change<'a> {
    /// A singleton save.
    Save {
        /// The object.
        key: ObjectKey,
        /// The new document.
        doc: &'a Doc,
    },
    /// Several singleton saves validated as one candidate.
    Batch(&'a [(ObjectKey, Doc)]),
    /// A rollback: a save of revision `to`'s document.
    Rollback {
        /// The object.
        key: ObjectKey,
        /// The revision restored.
        to: u32,
        /// Its document (upgraded to the current schema).
        doc: &'a Doc,
    },
    /// A new unit (not yet in the candidate unit set: it has no ID yet).
    UnitEnroll(&'a NewUnit),
    /// A unit's operational record changes.
    UnitUpdate {
        /// The unit.
        id: TerminalId,
        /// Its new record.
        doc: &'a UnitDoc,
    },
    /// A unit is retired (marked retired in the candidate).
    UnitRetire(TerminalId),
    /// A retired unit returns to service.
    UnitReinstate(TerminalId),
    /// The first-run seed (all four singletons at once).
    Seed(&'a SeedBundle),
}

/// Where in the configuration a violation is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRef {
    /// The object kind.
    pub kind: ConfigKind,
    /// The unit, for a unit document.
    pub object_id: Option<TerminalId>,
    /// A JSON pointer into the document (`/rf_channels/3/freq_khz`); empty
    /// for the whole document.
    pub path: String,
}

impl ObjectRef {
    /// A reference to `path` in `key`'s document.
    pub fn new(key: ObjectKey, path: impl Into<String>) -> Self {
        Self {
            kind: key.kind,
            object_id: key.object_id,
            path: path.into(),
        }
    }
}

/// One broken rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The stable kebab-case rule ID (also the API error code).
    pub rule: &'static str,
    /// The objects involved.
    pub objects: Vec<ObjectRef>,
    /// A human-readable explanation.
    pub detail: String,
    /// An ADR-003 §5 guardrail (both kinds block the save).
    pub hard: bool,
}

/// Why a validator refused. Never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    violations: Vec<Violation>,
}

impl Refusal {
    /// A refusal of `violations`; `None` when the list is empty (an empty
    /// refusal would refuse without saying why).
    pub fn new(violations: Vec<Violation>) -> Option<Self> {
        (!violations.is_empty()).then_some(Self { violations })
    }

    /// Wraps a validator's violation list: `Ok` when it is empty.
    ///
    /// # Errors
    ///
    /// The refusal when `violations` is non-empty.
    pub fn check(violations: Vec<Violation>) -> Result<(), Self> {
        Self::new(violations).map_or(Ok(()), Err)
    }

    /// The violations, at least one.
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    /// The rule IDs, in order.
    pub fn rules(&self) -> Vec<&'static str> {
        self.violations.iter().map(|v| v.rule).collect()
    }
}

/// The #26 hook.
pub trait ConfigValidator: Send + Sync {
    /// Admits or refuses `change`, given the `candidate` it would produce.
    ///
    /// # Errors
    ///
    /// A [`Refusal`] naming every broken rule.
    fn validate(&self, candidate: &ConfigSnapshot<'_>, change: &Change<'_>) -> Result<(), Refusal>;
}

/// The rules [`StructuralValidator`] owns (docs drift-tested).
pub const STRUCTURAL_RULES: [&str; 4] = [
    "unknown-box-type",
    "terminal-type",
    "unit-date",
    "unit-ip-address",
];

/// The validator #25 ships: the structure of the unit records a change
/// touches (box type in the catalog, Define Terminal type, dates, IP
/// address). It checks **no** guardrail or consistency rule; #26's
/// validator does, and the configuration store has no front door (#27)
/// until #26 lands (design note §7: #25 and #26 land back to back).
#[derive(Debug, Clone, Copy, Default)]
pub struct StructuralValidator;

fn calendar_date(s: &str) -> bool {
    crate::store::is_calendar_date(s)
}

fn check_unit(key: ObjectKey, d: &UnitDoc, catalog: &dyn BoxTypeCatalog, out: &mut Vec<Violation>) {
    let at = |p: &str| vec![ObjectRef::new(key, p)];
    if !catalog.contains(&d.box_type) {
        out.push(Violation {
            rule: "unknown-box-type",
            objects: at("/box_type"),
            detail: format!(
                "box type {:?} is not in the upstream catalog; a new box type is added only \
                 through an upstream PR or a fork",
                d.box_type
            ),
            hard: true,
        });
    }
    if d.terminal_type.parse::<TerminalType>().is_err() {
        out.push(Violation {
            rule: "terminal-type",
            objects: at("/terminal_type"),
            detail: format!(
                "{:?} is not a Define Terminal type (One-Way, Fone-Way, Two-Way, Ethernet, \
                 DOCSIS)",
                d.terminal_type
            ),
            hard: false,
        });
    }
    if let Some(date) = &d.install_date
        && !calendar_date(date)
    {
        out.push(Violation {
            rule: "unit-date",
            objects: at("/install_date"),
            detail: format!("install date {date:?} is not a YYYY-MM-DD calendar date"),
            hard: false,
        });
    }
    if let Some(ip) = &d.ip_address
        && ip.parse::<std::net::IpAddr>().is_err()
    {
        out.push(Violation {
            rule: "unit-ip-address",
            objects: at("/ip_address"),
            detail: format!("{ip:?} is not an IPv4 or IPv6 address"),
            hard: false,
        });
    }
}

impl ConfigValidator for StructuralValidator {
    fn validate(&self, candidate: &ConfigSnapshot<'_>, change: &Change<'_>) -> Result<(), Refusal> {
        let mut v = Vec::new();
        match change {
            Change::UnitEnroll(u) => {
                // No ID yet; the pointer is into the proposed record.
                let key = ObjectKey {
                    kind: ConfigKind::Unit,
                    object_id: None,
                };
                check_unit(key, &u.doc, candidate.catalog, &mut v);
            }
            Change::UnitUpdate { id, doc } => {
                check_unit(ObjectKey::unit(*id), doc, candidate.catalog, &mut v);
            }
            Change::Rollback {
                key,
                doc: Doc::Unit(d),
                ..
            } => check_unit(*key, d, candidate.catalog, &mut v),
            _ => {}
        }
        Refusal::check(v)
    }
}

/// Test doubles. **Never compiled outside tests**: a validator that admits
/// everything must not exist in a build an adapter could ship (design note
/// §4).
#[cfg(test)]
pub(crate) mod doubles {
    use std::sync::Mutex;

    use super::*;

    type RefuseFn = dyn Fn(&ConfigSnapshot<'_>, &Change<'_>) -> Vec<Violation> + Send + Sync;

    /// Admits everything.
    pub(crate) struct AllowAll;

    impl ConfigValidator for AllowAll {
        fn validate(&self, _: &ConfigSnapshot<'_>, _: &Change<'_>) -> Result<(), Refusal> {
            Ok(())
        }
    }

    /// What one validate call saw.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Seen {
        pub(crate) change: String,
        pub(crate) lineup: Option<Lineup>,
        pub(crate) pid_plan: Option<PidPlan>,
        pub(crate) class_budgets: Option<ClassBudgets>,
        pub(crate) mux_settings: Option<MuxSettings>,
        pub(crate) units: Vec<(TerminalId, UnitDoc, bool)>,
    }

    /// Records every candidate; refuses with `refuse` when it returns
    /// violations.
    pub(crate) struct Recording {
        pub(crate) seen: Mutex<Vec<Seen>>,
        pub(crate) refuse: Box<RefuseFn>,
    }

    impl Recording {
        pub(crate) fn new(
            refuse: impl Fn(&ConfigSnapshot<'_>, &Change<'_>) -> Vec<Violation> + Send + Sync + 'static,
        ) -> Self {
            Self {
                seen: Mutex::new(Vec::new()),
                refuse: Box::new(refuse),
            }
        }

        pub(crate) fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl ConfigValidator for Recording {
        fn validate(&self, c: &ConfigSnapshot<'_>, change: &Change<'_>) -> Result<(), Refusal> {
            let units = c
                .units
                .ids()
                .into_iter()
                .map(|id| (id, c.units.get(id).unwrap().clone(), c.units.is_retired(id)))
                .collect();
            self.seen.lock().unwrap().push(Seen {
                change: format!("{change:?}"),
                lineup: c.lineup.cloned(),
                pid_plan: c.pid_plan.cloned(),
                class_budgets: c.class_budgets.cloned(),
                mux_settings: c.mux_settings.cloned(),
                units,
            });
            Refusal::check((self.refuse)(c, change))
        }
    }
}
