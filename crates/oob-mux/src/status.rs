//! Wire-format trust labels (AGENTS.md §2).

use core::fmt;

/// How far a wire-format claim is established: the AGENTS.md §2 labels.
/// Promotion needs the evidence the label names.
///
/// This is the one definition of the enum. `oob-msg` is still an empty
/// stub; when Slice C seeds it, it must `pub use oob_mux::Status` and not
/// define a second enum (oob-msg builds mux submissions, so it depends on
/// oob-mux, never the reverse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Status {
    /// Sourced from a manual or specification in hand.
    Documented,
    /// Constructed from published structure or inference, not yet accepted
    /// by a box.
    Hypothesized,
    /// Accepted by a box, with a capture in `lab/captures/` referenced.
    /// Nothing in `oob-mux::psi` may carry this label without a capture.
    Verified,
}

impl Status {
    /// The label as it is written in the docs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Documented => "documented",
            Self::Hypothesized => "hypothesized",
            Self::Verified => "verified",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
