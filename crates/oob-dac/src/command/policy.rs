//! The policy hook into #23.
//!
//! Every command passes through [`Policy::check`] after validation and
//! target resolution, before anything is persisted or submitted. #23
//! implements it: owner role, the confirmation token bound to (op, serial),
//! the sacrificial-only CDL lock, the bulk dry-run default, and refusal of
//! guest-role writes (r6 F6-10). It also receives every outcome through
//! [`Policy::audit`], so audit rows are the policy layer's, not scattered.
//!
//! [`AllowAllPolicy`] is a **test double**. ADR-002 action item 5 lets this
//! hook land with an allow-all double only because #23 follows before any
//! adapter (REST #27, WireLink-wire #28): [`CommandBus::new`] has no
//! default policy, so an adapter must pass one explicitly, and no adapter
//! may ship with the double.
//!
//! [`CommandBus::new`]: super::CommandBus::new

use super::target::TerminalRef;
use super::{Command, CommandContext};

/// Proof that policy admitted a command. Carries no data yet; #23 may add
/// a bound confirmation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    _private: (),
}

impl Grant {
    /// A grant. Only a [`Policy`] implementation should mint one.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for Grant {
    fn default() -> Self {
        Self::new()
    }
}

/// Why policy refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDenied {
    /// A stable machine-readable code (for REST and WireLink error codes).
    pub reason: &'static str,
    /// A human-readable explanation.
    pub detail: String,
}

/// One audit note for [`Policy::audit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditNote {
    /// The command's kind name.
    pub command: &'static str,
    /// How many terminals it resolved to (0 for non-PDU commands).
    pub terminals: usize,
    /// `"ok"`, `"dry-run"`, or a [`CommandError::reason`](super::CommandError::reason).
    pub outcome: &'static str,
}

/// The #23 hook.
pub trait Policy: Send + Sync {
    /// Admits or refuses `cmd` for `ctx`, given the terminals it resolved
    /// to (empty for non-PDU commands).
    ///
    /// # Errors
    ///
    /// [`PolicyDenied`].
    fn check(
        &self,
        ctx: &CommandContext,
        cmd: &Command,
        resolved: &[TerminalRef],
    ) -> Result<Grant, PolicyDenied>;

    /// Records the outcome of every submitted command: admitted, dry-run,
    /// refused here or earlier, or failed afterwards. Required, not
    /// defaulted, so an implementation cannot forget auditing.
    fn audit(&self, ctx: &CommandContext, note: &AuditNote);
}

/// A policy that admits everything and audits nothing. **TEST DOUBLE** (see
/// the module docs).
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllPolicy;

impl Policy for AllowAllPolicy {
    fn check(
        &self,
        _ctx: &CommandContext,
        _cmd: &Command,
        _resolved: &[TerminalRef],
    ) -> Result<Grant, PolicyDenied> {
        Ok(Grant::new())
    }

    fn audit(&self, _ctx: &CommandContext, _note: &AuditNote) {}
}
