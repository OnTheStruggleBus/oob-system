//! DAC reproduction — unit DB (sqlite, DAC Define Terminal schema),
//! entitlements, scheduler, and the WireLink-verb admin API (tokio+axum,
//! utoipa → `openapi.json` with drift checks).
//!
//! The unit DB is derived state (F5-12): rebuildable from `ops/` config
//! plus box re-enrollment. Slice D lands the schema and API; this crate
//! currently establishes the workspace boundary.

// Intentionally empty: Slice D lands the unit DB and admin API here.
