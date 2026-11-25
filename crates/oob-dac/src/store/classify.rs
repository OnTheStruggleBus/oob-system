//! The classification registry (r6 F6-5): every table in the store, its
//! durability class, and why.
//!
//! This is code (ADR-003 Layer 3), not configuration. A migration that
//! creates a table adds its entry here in the same commit; a test compares
//! this list with `sqlite_master` and fails on any unclassified or stale
//! entry.

/// How a table survives backup, restore and rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// Backed up; never rebuilt; never dropped. Nothing can regenerate it.
    Precious,
    /// A pure fold of `event_log`: dropping it and replaying the log
    /// rebuilds it exactly.
    DerivedFromEvents,
    /// A cache of what the plant reports: rebuilt with no observations and
    /// refilled by polls and d-screen scrapes.
    DerivedObserved,
}

/// One registry row.
#[derive(Debug, Clone, Copy)]
pub struct TableClass {
    /// The sqlite table name.
    pub name: &'static str,
    /// Its durability class.
    pub durability: Durability,
    /// Whether triggers forbid `UPDATE` and `DELETE` (a journal).
    pub append_only: bool,
    /// Why it has this class (shown in the docs and in review).
    pub why: &'static str,
}

/// Every table in the store. Order is irrelevant.
pub const TABLES: &[TableClass] = &[
    TableClass {
        name: "terminal",
        durability: Durability::Precious,
        append_only: false,
        why: "the Define Terminal record (Plan §3.2); operator-entered, burned-in \
              identifiers; the unit DB is the enrollment mechanism",
    },
    TableClass {
        name: "terminal_status",
        durability: Durability::DerivedObserved,
        append_only: false,
        why: "last response, failures, power leveling, non-participating counts, \
              chronic flag, d-screen cache: all plant-reported (F6-5 derived list)",
    },
    TableClass {
        name: "event_log",
        durability: Durability::Precious,
        append_only: true,
        why: "the outbox: precious events that every derived table is folded from",
    },
    TableClass {
        name: "derive_watermark",
        durability: Durability::DerivedFromEvents,
        append_only: false,
        why: "per-deriver replay position; reset to 0 on rebuild",
    },
    TableClass {
        name: "audit_log",
        durability: Durability::Precious,
        append_only: true,
        why: "the bsilog equivalent (DAC Reference §8); F6-5 precious list",
    },
    TableClass {
        name: "confirmation_token",
        durability: Durability::DerivedObserved,
        append_only: false,
        why: "short-lived F6-10 tokens (SHA-256 only); worthless after a restart, \
              so losing them on a restore is correct",
    },
    TableClass {
        name: "install_setting",
        durability: Durability::Precious,
        append_only: false,
        why: "per-install owner settings (the sacrificial-only CDL lock, seeded ON; \
              the install id); changes are audited by the policy layer",
    },
];

/// Tables whose rows are box instances or box-identifying data (unit
/// addresses, serials). No box instance comes from git (ADR-003 §6): no
/// migration and no seed may insert into these, and a fresh store has none.
/// A later table holding box-identifying data joins this list.
pub const BOX_IDENTIFYING: &[&str] = &["terminal"];

/// The registry entry for `name`, if any.
pub fn class_of(name: &str) -> Option<&'static TableClass> {
    TABLES.iter().find(|t| t.name == name)
}
