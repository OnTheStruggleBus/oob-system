//! DAC reproduction — unit DB (sqlite, DAC Define Terminal schema),
//! entitlements, scheduler, and the WireLink-verb admin API (tokio+axum,
//! utoipa → `openapi.json` with drift checks).
//!
//! Unit-DB durability (derived vs. precious state) is open under r5
//! F5-12, pending Marshal's disposition. Slice D lands the schema and
//! API; this crate currently establishes the workspace boundary.

// Intentionally empty: Slice D lands the unit DB and admin API here.
