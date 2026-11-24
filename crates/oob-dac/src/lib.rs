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

pub mod command;
pub mod store;
