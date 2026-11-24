//! Encoding under `catch_unwind`.
//!
//! Each PDU is produced by a family [`Encoder`] (`oob-msg`, #19). An
//! encoder panic fails **that one command**: it is caught here, per
//! encode, never per process (ADR-002 Consequences), turned into an
//! an encode failure, logged by the bus, and the daemon keeps
//! running. This relies on `panic = "unwind"`, the default; no Oob build
//! profile may set `panic = "abort"` (the same rule as oob-mux's stamper).

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

use oob_mux::Pdu;

use super::catalog::{AutoOp, CatalogEntry, OperationKind, TerminalOp};
use super::target::TerminalRef;

/// What an encoder is asked to produce.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Work {
    /// A per-terminal operation.
    Terminal(TerminalOp),
    /// A Define Operations kind.
    Operation(OperationKind),
    /// An auto-generated operation (no terminal).
    Auto(AutoOp),
    /// A Discovery Lab injection of the named template (C5).
    Lab(String),
}

impl Work {
    /// The stable family name used in errors and logs.
    pub fn name(&self) -> String {
        match self {
            Self::Terminal(op) => op.name().to_owned(),
            Self::Operation(k) => k.name().to_owned(),
            Self::Auto(a) => a.name().to_owned(),
            Self::Lab(t) => format!("lab:{t}"),
        }
    }
}

/// An encoder's refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeError {
    /// Why it could not encode.
    pub reason: String,
}

/// Turns work for one terminal into one PDU. Implemented by `oob-msg`'s
/// family encoders (#19); tests use doubles.
///
/// The returned PDU's `release_at` and `deadline` are overwritten by the
/// command layer (spacing is its job alone), and its class is set from the
/// catalog.
pub trait Encoder: Send + Sync {
    /// Encodes `work` for `terminal` (`None` for auto-generated work).
    ///
    /// # Errors
    ///
    /// [`EncodeError`] when the family cannot be encoded.
    fn encode(&self, work: &Work, terminal: Option<&TerminalRef>) -> Result<Pdu, EncodeError>;
}

/// Why an encode did not yield a PDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EncodeFailure {
    /// The encoder returned an error.
    Error(String),
    /// The encoder panicked; the payload text, if it was a string.
    Panicked(String),
}

/// Runs the encoder with panics contained.
pub(super) fn encode_guarded(
    encoder: &dyn Encoder,
    work: &Work,
    terminal: Option<&TerminalRef>,
) -> Result<Pdu, EncodeFailure> {
    match catch_unwind(AssertUnwindSafe(|| encoder.encode(work, terminal))) {
        Ok(Ok(pdu)) => Ok(pdu),
        Ok(Err(e)) => Err(EncodeFailure::Error(e.reason)),
        Err(payload) => {
            let msg = panic_text(&*payload);
            // Leak the payload rather than drop it: its `Drop` could panic
            // again, outside any guard.
            std::mem::forget(payload);
            Err(EncodeFailure::Panicked(msg))
        }
    }
}

fn panic_text(p: &(dyn Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}
