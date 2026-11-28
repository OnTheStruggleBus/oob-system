//! DAC reproduction — unit DB (sqlite, DAC Define Terminal schema),
//! entitlements, scheduler, and the WireLink-verb admin API (tokio+axum,
//! utoipa → `openapi.json` with drift checks).
//!
//! # The store (r6 F6-5, ADR-003 §6)
//!
//! oob-dac owns all state in **one sqlite file** ([`store`]). The file is
//! the installation: it is backed up and restored whole (#24), never
//! rebuilt from git, and no box instance is ever seeded from git — every
//! terminal record is created at runtime with a store-allocated terminal ID.
//! Unit addresses and serials are box-identifying data and live only in the
//! file (ADR-003 §4).
//!
//! Durability is classified **per table**, not per database: each table is
//! precious (backed up, never rebuilt), derived from events (a fold of the
//! precious `event_log`, rebuilt exactly by replay), or derived from plant
//! observations (rebuilt empty and refilled by polls). The registry is
//! [`store::TABLES`]. This replaces the r5 F5-12 wording that called the
//! whole unit DB derived state; r6 F6-5 and ADR-003 §6 amended it.
//!
//! # The command layer (r6 F6-11, ADR-002 decision 1)
//!
//! Every operation enters through one internal command layer
//! ([`command`]): the §11 catalog and Define Operations, targeting, the
//! status lifecycle, campaign pacing as per-PDU release times, and the only
//! path into the `oob-mux` scheduler. The admin API and every other front
//! door are adapters over it, in later issues.
//!
//! # The policy layer (r6 F6-10, ADR-003 section 5)
//!
//! [`policy::StorePolicy`] is the real implementation of the command layer's
//! policy hook: owner role, confirmation tokens bound to (operation,
//! target), dry-run default, the sacrificial-only CDL lock (seeded ON,
//! turned off with owner plus token alone, QA-7), and an audit row for every
//! outcome.
//!
//! # The configuration store (ADR-003 section 2, r6 F6-3, F6-4, F6-5)
//!
//! [`config`] holds the Layer 1 objects an operator changes at runtime (the
//! lineup, the PID plan, the class budgets, the mux settings and each
//! unit's operational record) with revision history, rollback, an
//! append-only journal on the same audit path as F6-10, and a mandatory
//! validation hook (#26). Changes enter as `Command::Admin` through the
//! command bus.

pub mod command;
pub mod config;
pub mod policy;
pub mod store;
