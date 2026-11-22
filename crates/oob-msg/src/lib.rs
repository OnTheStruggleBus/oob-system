//! GI message layer — the O4 protocol document as code.
//!
//! Every message family carries a discovery status: `documented`
//! (manual-sourced), `hypothesized` (constructed, unverified), or
//! `verified` (box-accepted, backed by a capture in `lab/captures/`).
//! Wire formats land with Slice C; this crate currently establishes the
//! workspace boundary. See AGENTS.md §4 for the crate map.

// Intentionally empty: Slice C (W3) seeds the message catalog here.
