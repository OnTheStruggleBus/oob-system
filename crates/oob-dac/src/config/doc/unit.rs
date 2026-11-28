//! The `unit` document: a unit's **mutable operational record**.
//!
//! The Define Terminal identity (terminal ID, unit address, check digits,
//! serial) is **not** in it: identity is immutable after enrollment and a
//! wrong one is corrected by retiring the unit and enrolling it again. The
//! `terminal` row stays the authoritative current record (targeting reads
//! it); every change also writes a revision of this document in the same
//! transaction, and the two are kept equal by test. The
//! auto-registration date is system-set and not part of the document.

use serde::{Deserialize, Serialize};

use crate::store::NewTerminal;

/// A unit's operational record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitDoc {
    /// One of the five Define Terminal types (`One-Way`, `Fone-Way`,
    /// `Two-Way`, `Ethernet`, `DOCSIS`).
    pub terminal_type: String,
    /// A box type from the upstream catalog (Marshal's QA-6 ruling).
    pub box_type: String,
    /// Account number.
    #[serde(default)]
    pub account_number: Option<String>,
    /// On-Plant flag.
    #[serde(default)]
    pub on_plant: bool,
    /// Install date, `YYYY-MM-DD`.
    #[serde(default)]
    pub install_date: Option<String>,
    /// Activated flag.
    #[serde(default)]
    pub activated: bool,
    /// Autodiscovered (DSG/DOCSIS).
    #[serde(default)]
    pub autodiscovered: bool,
    /// IP address (DSG/DOCSIS).
    #[serde(default)]
    pub ip_address: Option<String>,
    /// BSI name.
    #[serde(default)]
    pub bsi_name: Option<String>,
    /// Downstream plant.
    #[serde(default)]
    pub downstream_plant: Option<String>,
    /// Upstream plant.
    #[serde(default)]
    pub upstream_plant: Option<String>,
    /// The sacrificial unit for destructive operations (AGENTS.md §7).
    /// Setting it is D-hard (`mark-sacrificial`, owner plus a token).
    #[serde(default)]
    pub sacrificial: bool,
}

impl UnitDoc {
    /// The document for a stored record's mutable fields.
    pub fn from_record(t: &NewTerminal) -> Self {
        Self {
            terminal_type: t.terminal_type.as_str().to_owned(),
            box_type: t.box_type.clone(),
            account_number: t.account_number.clone(),
            on_plant: t.on_plant,
            install_date: t.install_date.clone(),
            activated: t.activated,
            autodiscovered: t.autodiscovered,
            ip_address: t.ip_address.clone(),
            bsi_name: t.bsi_name.clone(),
            downstream_plant: t.downstream_plant.clone(),
            upstream_plant: t.upstream_plant.clone(),
            sacrificial: t.sacrificial,
        }
    }

    /// A minimal record: the type and box type, everything else empty.
    pub fn new(terminal_type: impl Into<String>, box_type: impl Into<String>) -> Self {
        Self {
            terminal_type: terminal_type.into(),
            box_type: box_type.into(),
            account_number: None,
            on_plant: false,
            install_date: None,
            activated: false,
            autodiscovered: false,
            ip_address: None,
            bsi_name: None,
            downstream_plant: None,
            upstream_plant: None,
            sacrificial: false,
        }
    }
}
