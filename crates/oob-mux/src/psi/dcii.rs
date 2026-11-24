//! DCII constants, each with its trust label (AGENTS.md §2, r4 F4-6).
//!
//! These are code (ADR-003 Layer 3): an operator cannot add a CA system
//! at runtime; a new one is an upstream PR like a box type. Nothing here
//! is `verified`: the box-side check is d 15 / d 16 and needs a capture.

use crate::status::Status;

/// The CA systems Oob knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CaSystem {
    /// General Instrument / Motorola DigiCipher II.
    GiDigiCipher,
}

/// CA_system_id for [`CaSystem::GiDigiCipher`]. **`hypothesized`**: the
/// DVB CA_System_ID registry is commonly reported to place General
/// Instrument / DigiCipher in the 0x07xx block, but no source in hand
/// gives the value a DCT2000 expects; it must come from the DAC/SEM
/// manuals' CA-parameter sections or a d 16 dump (design note on #17,
/// open sourcing item for oob-expert).
pub const GI_CA_SYSTEM_ID: u16 = 0x0700;

impl CaSystem {
    /// The 16-bit CA_system_id.
    pub const fn id(self) -> u16 {
        match self {
            Self::GiDigiCipher => GI_CA_SYSTEM_ID,
        }
    }

    /// The trust label of [`CaSystem::id`].
    pub const fn status(self) -> Status {
        match self {
            Self::GiDigiCipher => Status::Hypothesized,
        }
    }
}
