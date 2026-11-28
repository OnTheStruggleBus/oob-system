//! Applying a committed revision to the running system (#29 implements it).
//!
//! The store is the truth; applying is a **post-commit side effect** on
//! another component (the mux's budget, its pacer, PSI re-submission), so
//! it is deliberately not a store `Deriver`. The command layer calls
//! [`ConfigApplier::apply`] only after the save committed and returns the
//! result beside the revision: "saved, applied", "saved, restart required",
//! or "saved, **not applied**: reason". A failed apply never un-saves; the
//! previous good configuration keeps running, and the daemon's
//! head-versus-applied revision metrics make the divergence visible.

use super::doc::Doc;
use super::key::ObjectKey;

/// What applying a revision did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyStatus {
    /// The running system uses it now.
    Applied,
    /// Stored; takes effect at the next start (the mux's queue sizes).
    RestartRequired,
    /// Stored but not applied; the previous configuration keeps running.
    Failed {
        /// Why (for example a C0 floor below the live C0 carousel demand,
        /// a check that only exists at apply time).
        reason: String,
    },
    /// Nothing running consumes this object (units: targeting reads the
    /// store directly).
    NotApplicable,
}

/// Applies committed revisions. Implemented by the daemon (#29).
///
/// The configuration runtime calls it inside the critical section that
/// also covers the commit, so revisions arrive in commit order. An applier
/// must therefore **return promptly** and **must not submit a
/// configuration command** (`Command::Admin`) itself: that would wait on
/// the section it is running in and deadlock. It may read the store.
pub trait ConfigApplier: Send + Sync {
    /// Applies `doc`, now revision `revision` of `key`. Called after commit
    /// only; must not panic on a document the running system cannot take.
    fn apply(&self, key: ObjectKey, doc: &Doc, revision: u32) -> ApplyStatus;
}

/// An applier for a process with nothing running yet: every revision is
/// [`ApplyStatus::NotApplicable`]. The daemon (#29) replaces it.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoApplier;

impl ConfigApplier for NoApplier {
    fn apply(&self, _key: ObjectKey, _doc: &Doc, _revision: u32) -> ApplyStatus {
        ApplyStatus::NotApplicable
    }
}
