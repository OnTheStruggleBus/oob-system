//! The policy layer: the real implementation of the command layer's
//! [`Policy`] hook (r6 F6-10, ADR-003 section 5, issue #23).
//!
//! [`StorePolicy`] is the server-side guardrail every client sits behind,
//! because every front door reaches oob-dac only through
//! [`CommandBus::submit`](crate::command::CommandBus::submit). It enforces:
//!
//! - **Roles.** A guest (museum mode) is read-only: every command is refused
//!   (`guest-read-only`). A destructive class needs the **owner** role
//!   through the REST or Lab front door; the billing and WireLink-wire
//!   doors, and the DAC's own timers, can never originate one.
//! - **Confirmation tokens** ([`token`]), bound to (operation, target),
//!   short-lived, single-use, issued by a separate call that checks the
//!   typed phrase on the server.
//! - **Dry-run default.** [`DefineOperation::new`] builds a dry run; a bulk
//!   operation executes only when its caller opts in
//!   ([`DefineOperation::execute`]). A destructive bulk execute also needs a
//!   token bound to the plan digest the dry run reported.
//! - **The sacrificial-only CDL lock** ([`StorePolicy::set_cdl_lock`]):
//!   a per-install owner setting, seeded **ON**; while on, a CDL campaign
//!   with any non-sacrificial target is refused. Turning it **off** takes
//!   the owner role and a confirmation token and nothing else (Marshal,
//!   QA-7: the product imposes no D0 evidence check; AGENTS.md section 7
//!   still binds this project's own lab).
//! - **Audit.** Every command outcome, token issuance and setting change is
//!   appended to the precious `audit_log`. A refusal names its rule.
//!
//! The destructive classes are the table in [`classes`]; the mdBook page
//! `architecture/oob-dac-policy.md` is drift-tested against it.
//!
//! # Boundaries
//!
//! - Policy calls the store through a [`StoreHandle`] with
//!   [`call_blocking`](StoreHandle::call_blocking), so [`StorePolicy`]
//!   methods must not run on an async runtime thread (the same rule as the
//!   bus's other synchronous calls).
//! - A store failure **fails closed** (`policy-store-error`).
//! - [`AllowAllPolicy`](crate::command::policy::AllowAllPolicy) stays as a
//!   test double. [`CommandBus::new`](crate::command::CommandBus::new) takes
//!   a policy explicitly and has no default, so an adapter ships with a
//!   [`StorePolicy`].
//!
//! [`Policy`]: crate::command::policy::Policy
//! [`DefineOperation::new`]: crate::command::DefineOperation::new
//! [`DefineOperation::execute`]: crate::command::DefineOperation::execute

pub mod classes;
pub mod token;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rusqlite::OptionalExtension;

use crate::command::policy::{AuditNote, Grant, Policy, PolicyDenied};
use crate::command::target::TerminalRef;
use crate::command::{Clock, Command, CommandContext, ConfirmationToken, FrontDoor, Role};
use crate::store::{AuditEntry, StoreError, StoreHandle, TerminalId, Tx};

pub use classes::{CLASSES, ClassRow, DestructiveOp, TargetKind, Tier, classify_terminal_op};
pub use token::{TokenTarget, confirmation_phrase, plan_digest};

// The tests build a real scheduler, so they live under `command/`: the
// source scan `the_scheduler_is_named_only_in_the_command_layer` allows
// oob-mux to be named only there.
#[cfg(test)]
#[path = "../command/policy_tests.rs"]
mod tests;

/// The server-side ceiling on a token's lifetime.
pub const TOKEN_TTL_CAP: Duration = Duration::from_secs(600);

/// The default token lifetime.
pub const TOKEN_TTL_DEFAULT: Duration = Duration::from_secs(120);

const LOCK_KEY: &str = "cdl_sacrificial_only";
const INSTALL_KEY: &str = "install_id";

/// The stable name of the rule that refused. Each is a kebab-case code for
/// REST and WireLink errors ([`as_str`](Self::as_str)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RuleId {
    /// The destructive class, or this front door, needs the owner role.
    OwnerRequired,
    /// A guest may read only.
    GuestReadOnly,
    /// The DAC's own timers may not originate a destructive operation.
    SystemCannotOriginate,
    /// A destructive operation arrived with no confirmation token.
    TokenMissing,
    /// The token is not one this install issued (or was purged).
    TokenUnknown,
    /// The token's lifetime ended.
    TokenExpired,
    /// The token was already spent.
    TokenReused,
    /// The token was issued for a different operation.
    TokenOpMismatch,
    /// The token was issued for a different target.
    TokenTargetMismatch,
    /// The token was issued for a bulk set that is not the one resolved now.
    TokenPlanChanged,
    /// The typed confirmation phrase did not match.
    ConfirmTextMismatch,
    /// The CDL lock is on and a target is not the sacrificial unit.
    CdlLockNonSacrificial,
    /// No such terminal.
    UnknownTerminal,
    /// The store failed; policy fails closed.
    StoreUnavailable,
}

impl RuleId {
    /// The stable code.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OwnerRequired => "owner-required",
            Self::GuestReadOnly => "guest-read-only",
            Self::SystemCannotOriginate => "system-cannot-originate",
            Self::TokenMissing => "token-missing",
            Self::TokenUnknown => "token-unknown",
            Self::TokenExpired => "token-expired",
            Self::TokenReused => "token-reused",
            Self::TokenOpMismatch => "token-op-mismatch",
            Self::TokenTargetMismatch => "token-target-mismatch",
            Self::TokenPlanChanged => "token-plan-changed",
            Self::ConfirmTextMismatch => "confirm-text-mismatch",
            Self::CdlLockNonSacrificial => "cdl-lock-non-sacrificial",
            Self::UnknownTerminal => "unknown-terminal",
            Self::StoreUnavailable => "policy-store-error",
        }
    }

    /// Every rule, for the docs drift test.
    pub const ALL: [Self; 14] = [
        Self::OwnerRequired,
        Self::GuestReadOnly,
        Self::SystemCannotOriginate,
        Self::TokenMissing,
        Self::TokenUnknown,
        Self::TokenExpired,
        Self::TokenReused,
        Self::TokenOpMismatch,
        Self::TokenTargetMismatch,
        Self::TokenPlanChanged,
        Self::ConfirmTextMismatch,
        Self::CdlLockNonSacrificial,
        Self::UnknownTerminal,
        Self::StoreUnavailable,
    ];

    fn deny(self, detail: impl Into<String>) -> PolicyDenied {
        PolicyDenied {
            reason: self.as_str(),
            detail: detail.into(),
        }
    }

    fn deny_default(self) -> PolicyDenied {
        self.deny(match self {
            Self::OwnerRequired => "this needs the owner role through the REST or Lab front door",
            Self::GuestReadOnly => "a guest may read only",
            Self::SystemCannotOriginate => {
                "the DAC's own timers cannot originate a destructive operation"
            }
            Self::TokenMissing => "this operation needs a confirmation token",
            Self::TokenUnknown => "the confirmation token is not valid",
            Self::TokenExpired => "the confirmation token has expired",
            Self::TokenReused => "the confirmation token was already used",
            Self::TokenOpMismatch => "the confirmation token was issued for a different operation",
            Self::TokenTargetMismatch => "the confirmation token was issued for a different target",
            Self::TokenPlanChanged => {
                "the target set changed since the token was issued; run the dry run again"
            }
            Self::ConfirmTextMismatch => "the typed confirmation phrase does not match",
            Self::CdlLockNonSacrificial => {
                "the sacrificial-only CDL lock is on and a target is not sacrificial"
            }
            Self::UnknownTerminal => "no such terminal",
            Self::StoreUnavailable => "the policy store failed; refusing (fail closed)",
        })
    }
}

fn store_denied(e: &StoreError) -> PolicyDenied {
    RuleId::StoreUnavailable.deny(format!("the policy store failed: {e}"))
}

/// Runtime policy settings (ADR-003 Layer 1).
#[derive(Debug, Clone, Copy)]
pub struct PolicySettings {
    token_ttl: Duration,
}

impl PolicySettings {
    /// Settings with `token_ttl` clamped to `1 ms ..= `[`TOKEN_TTL_CAP`].
    pub fn new(token_ttl: Duration) -> Self {
        Self {
            token_ttl: token_ttl.clamp(Duration::from_millis(1), TOKEN_TTL_CAP),
        }
    }

    /// The token lifetime in force.
    pub fn token_ttl(&self) -> Duration {
        self.token_ttl
    }
}

impl Default for PolicySettings {
    fn default() -> Self {
        Self::new(TOKEN_TTL_DEFAULT)
    }
}

/// What a caller asks for to get a confirmation token.
#[derive(Debug, Clone)]
pub struct TokenRequest {
    /// The operation to authorize.
    pub op: DestructiveOp,
    /// What it is bound to.
    pub target: TokenTarget,
    /// The typed phrase, which must equal [`confirmation_phrase`].
    pub confirm_text: String,
}

/// A freshly issued token; the only time it is visible.
#[derive(Debug, Clone)]
pub struct IssuedToken {
    /// Pass it as [`CommandContext::confirmation`].
    pub token: ConfirmationToken,
    /// How long it stays valid.
    pub ttl: Duration,
}

/// The store-backed policy. Cheap to share behind an `Arc`.
pub struct StorePolicy {
    store: StoreHandle,
    clock: Arc<dyn Clock>,
    settings: PolicySettings,
    audit_failures: AtomicU64,
}

impl std::fmt::Debug for StorePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorePolicy")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

fn setting(tx: &Tx<'_>, key: &str) -> Result<Option<String>, StoreError> {
    Ok(tx
        .conn()
        .query_row(
            "SELECT value FROM install_setting WHERE key = ?1",
            [key],
            |r| r.get(0),
        )
        .optional()?)
}

/// The lock reads ON unless the stored value is exactly `0`: a missing or
/// damaged setting fails closed.
fn lock_on(tx: &Tx<'_>) -> Result<bool, StoreError> {
    Ok(setting(tx, LOCK_KEY)?.as_deref() != Some("0"))
}

fn install_id(tx: &Tx<'_>) -> Result<String, StoreError> {
    setting(tx, INSTALL_KEY)?.ok_or_else(|| StoreError::Corrupt("install_id is missing".into()))
}

fn label(d: impl std::fmt::Debug) -> String {
    format!("{d:?}")
}

impl StorePolicy {
    /// A policy over `store`. Purges every stored token: token times are
    /// monotonic-clock milliseconds, which do not survive a restart, and an
    /// unspent token is worthless after one.
    ///
    /// # Errors
    ///
    /// [`StoreError`] if the purge fails.
    pub fn new(
        store: StoreHandle,
        clock: Arc<dyn Clock>,
        settings: PolicySettings,
    ) -> Result<Self, StoreError> {
        store.call_blocking(|s| {
            s.write(|tx| {
                tx.conn().execute("DELETE FROM confirmation_token", [])?;
                Ok(())
            })
        })?;
        Ok(Self {
            store,
            clock,
            settings,
            audit_failures: AtomicU64::new(0),
        })
    }

    /// How many audit rows could not be written. [`Policy::audit`] cannot
    /// return an error, so a failure is counted here for the health check.
    pub fn audit_failures(&self) -> u64 {
        self.audit_failures.load(Ordering::Relaxed)
    }

    fn now_ms(&self) -> i64 {
        i64::try_from(self.clock.mono().since_origin().as_millis()).unwrap_or(i64::MAX)
    }

    /// Whether the sacrificial-only CDL lock is on.
    ///
    /// # Errors
    ///
    /// [`StoreError`].
    pub fn cdl_lock_on(&self) -> Result<bool, StoreError> {
        self.store.call_blocking(|s| s.write(lock_on))
    }

    /// This installation's ID, the target of `CDL LOCK OFF <id>`.
    ///
    /// # Errors
    ///
    /// [`StoreError`].
    pub fn install_id(&self) -> Result<String, StoreError> {
        self.store.call_blocking(|s| s.write(install_id))
    }

    fn write_audit(&self, e: AuditEntry) {
        if self
            .store
            .call_blocking(move |s| s.write(|tx| tx.append_audit(&e).map(|_| ())))
            .is_err()
        {
            self.audit_failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn entry(
        ctx: &CommandContext,
        command: &str,
        target: Option<String>,
        outcome: &str,
    ) -> AuditEntry {
        AuditEntry {
            actor: ctx.actor.clone(),
            role: label(ctx.role),
            front_door: label(ctx.front_door),
            command: command.to_owned(),
            target,
            outcome: outcome.to_owned(),
            before: None,
            after: None,
        }
    }

    /// Audits a refusal made outside the bus (token issue, setting change).
    fn refuse(
        &self,
        ctx: &CommandContext,
        command: &str,
        target: Option<String>,
        d: PolicyDenied,
    ) -> PolicyDenied {
        self.write_audit(Self::entry(ctx, command, target, d.reason));
        d
    }

    /// The role and front-door gate for a destructive class.
    fn gate_destructive(ctx: &CommandContext) -> Result<(), PolicyDenied> {
        if ctx.role == Role::Guest {
            return Err(RuleId::GuestReadOnly.deny_default());
        }
        if ctx.front_door == FrontDoor::Auto {
            return Err(RuleId::SystemCannotOriginate.deny_default());
        }
        if ctx.role != Role::Owner || !matches!(ctx.front_door, FrontDoor::Rest | FrontDoor::Lab) {
            return Err(RuleId::OwnerRequired.deny_default());
        }
        Ok(())
    }

    /// The gate for a command that is not destructive.
    fn gate_plain(ctx: &CommandContext, cmd: &Command) -> Result<(), PolicyDenied> {
        if ctx.front_door == FrontDoor::Auto {
            return if matches!(cmd, Command::Auto(_)) {
                Ok(())
            } else {
                Err(RuleId::OwnerRequired
                    .deny("the DAC's own timers may issue auto-generated operations only"))
            };
        }
        match ctx.role {
            Role::Owner => Ok(()),
            Role::Operator if !matches!(cmd, Command::Lab(_)) => Ok(()),
            _ => Err(RuleId::OwnerRequired.deny_default()),
        }
    }

    /// Spends `token` for (`op`, `target`) in its own transaction.
    fn spend(
        &self,
        token: &ConfirmationToken,
        op: DestructiveOp,
        target: TokenTarget,
    ) -> Result<(), PolicyDenied> {
        let (now, tok) = (self.now_ms(), token.0.clone());
        match self
            .store
            .call_blocking(move |s| s.write(|tx| token::consume(tx, &tok, op, &target, now)))
        {
            Ok(None) => Ok(()),
            Ok(Some(rule)) => Err(rule.deny_default()),
            Err(e) => Err(store_denied(&e)),
        }
    }

    /// Applies the lock to `resolved`. Fails closed on an empty set: a
    /// target set the command layer could not resolve cannot be shown
    /// sacrificial.
    fn check_cdl_lock(&self, resolved: &[TerminalRef]) -> Result<(), PolicyDenied> {
        if !self.cdl_lock_on().map_err(|e| store_denied(&e))? {
            return Ok(());
        }
        if resolved.is_empty() {
            return Err(RuleId::CdlLockNonSacrificial.deny(
                "the sacrificial-only CDL lock is on and the target set is not resolved; \
                 failing closed",
            ));
        }
        let bad = classes::non_sacrificial(resolved);
        if bad.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = bad.iter().map(|t| t.get().to_string()).collect();
        Err(RuleId::CdlLockNonSacrificial.deny(format!(
            "the sacrificial-only CDL lock is on; not sacrificial: terminal ID {}",
            ids.join(", ")
        )))
    }

    /// Authorizes a destructive operation that does not arrive as a bus
    /// command yet (a parameter-level `code-download-enable`, #24's store
    /// restore): owner gate, the CDL lock for CDL classes, then the token.
    /// Audited as `command = <op name>`.
    ///
    /// # Errors
    ///
    /// [`PolicyDenied`] naming the rule.
    pub fn authorize(
        &self,
        ctx: &CommandContext,
        op: DestructiveOp,
        target: TokenTarget,
    ) -> Result<(), PolicyDenied> {
        let key = Some(target.key());
        let run = || -> Result<(), PolicyDenied> {
            Self::gate_destructive(ctx)?;
            if !op.row().targets.contains(&target.kind()) {
                return Err(RuleId::TokenTargetMismatch.deny(format!(
                    "{} cannot be bound to a {} target",
                    op.name(),
                    target.kind().label()
                )));
            }
            if op.is_cdl() {
                let TokenTarget::Terminal(id) = &target else {
                    return Err(RuleId::CdlLockNonSacrificial
                        .deny("the CDL lock needs a resolved terminal; failing closed"));
                };
                let sac = self.sacrificial(*id)?;
                self.check_cdl_lock(&[{
                    let mut t = TerminalRef::new(*id, crate::store::TerminalType::OneWay);
                    t.sacrificial = sac;
                    t
                }])?;
            }
            let t = ctx
                .confirmation
                .as_ref()
                .ok_or_else(|| RuleId::TokenMissing.deny_default())?;
            self.spend(t, op, target.clone())
        };
        match run() {
            Ok(()) => {
                self.write_audit(Self::entry(ctx, op.name(), key, "ok"));
                Ok(())
            }
            Err(d) => Err(self.refuse(ctx, op.name(), key, d)),
        }
    }

    fn sacrificial(&self, id: TerminalId) -> Result<bool, PolicyDenied> {
        let got = self
            .store
            .call_blocking(move |s| {
                s.write(|tx| {
                    Ok(tx
                        .conn()
                        .query_row(
                            "SELECT sacrificial FROM terminal WHERE terminal_id = ?1",
                            [id.get()],
                            |r| r.get::<_, bool>(0),
                        )
                        .optional()?)
                })
            })
            .map_err(|e| store_denied(&e))?;
        got.ok_or_else(|| RuleId::UnknownTerminal.deny_default())
    }

    /// Issues a confirmation token (owner only, audited). The caller must
    /// have typed the phrase ([`confirmation_phrase`]); the server compares
    /// it, so a script cannot skip the typed confirmation (AGENTS.md
    /// section 7).
    ///
    /// # Errors
    ///
    /// [`PolicyDenied`]: `guest-read-only`, `owner-required`,
    /// `system-cannot-originate`, `token-target-mismatch` (a target kind the
    /// class cannot take, or an install ID that is not this one),
    /// `confirm-text-mismatch`, or `policy-store-error`.
    pub fn issue_token(
        &self,
        ctx: &CommandContext,
        req: &TokenRequest,
    ) -> Result<IssuedToken, PolicyDenied> {
        let cmd = format!("issue-token:{}", req.op.name());
        let key = Some(req.target.key());
        let run = || -> Result<IssuedToken, PolicyDenied> {
            Self::gate_destructive(ctx)?;
            if !req.op.row().targets.contains(&req.target.kind()) {
                return Err(RuleId::TokenTargetMismatch.deny(format!(
                    "{} cannot be bound to a {} target",
                    req.op.name(),
                    req.target.kind().label()
                )));
            }
            if let TokenTarget::Install(id) = &req.target
                && *id != self.install_id().map_err(|e| store_denied(&e))?
            {
                return Err(RuleId::TokenTargetMismatch.deny("not this installation's ID"));
            }
            if req.confirm_text != confirmation_phrase(req.op, &req.target) {
                return Err(RuleId::ConfirmTextMismatch.deny_default());
            }
            let ttl = self.settings.token_ttl;
            let now = self.now_ms();
            let expires = now.saturating_add(i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX));
            let (op, target, actor) = (req.op, req.target.key(), ctx.actor.clone());
            let audit = Self::entry(ctx, &cmd, key.clone(), "ok");
            let token = self
                .store
                .call_blocking(move |s| {
                    s.write(|tx| {
                        tx.conn().execute(
                            "DELETE FROM confirmation_token WHERE expires_ms <= ?1",
                            [now],
                        )?;
                        let raw: String = tx.conn().query_row(
                            "SELECT 'oobtok-' || lower(hex(randomblob(16)))",
                            [],
                            |r| r.get(0),
                        )?;
                        tx.conn().execute(
                            "INSERT INTO confirmation_token \
                             (token_hash, operation, target, issued_by, issued_ms, expires_ms) \
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            rusqlite::params![
                                token::token_hash(&raw),
                                op.name(),
                                target,
                                actor,
                                now,
                                expires
                            ],
                        )?;
                        tx.append_audit(&audit)?;
                        Ok(raw)
                    })
                })
                .map_err(|e| store_denied(&e))?;
            Ok(IssuedToken {
                token: ConfirmationToken(token),
                ttl,
            })
        };
        run().map_err(|d| self.refuse(ctx, &cmd, key, d))
    }

    /// Turns the sacrificial-only CDL lock on or off (owner only, audited).
    ///
    /// **Turning it off** needs the owner role and a confirmation token for
    /// `CDL LOCK OFF <install id>` (in `ctx.confirmation`) and **nothing
    /// else**: no evidence check, no other precondition is consulted (QA-7).
    /// The token is spent in the same transaction that changes the setting.
    /// **Turning it on** needs the owner role only.
    ///
    /// # Errors
    ///
    /// [`PolicyDenied`] naming the rule.
    pub fn set_cdl_lock(&self, ctx: &CommandContext, on: bool) -> Result<(), PolicyDenied> {
        let cmd = "set-cdl-lock";
        let mut key = None;
        let mut run = || -> Result<(), PolicyDenied> {
            Self::gate_destructive(ctx)?;
            let target = TokenTarget::Install(self.install_id().map_err(|e| store_denied(&e))?);
            key = Some(target.key());
            let token = if on {
                None
            } else {
                Some(
                    ctx.confirmation
                        .as_ref()
                        .ok_or_else(|| RuleId::TokenMissing.deny_default())?
                        .0
                        .clone(),
                )
            };
            let now = self.now_ms();
            let mut audit = Self::entry(ctx, cmd, key.clone(), "ok");
            let refused = self
                .store
                .call_blocking(move |s| {
                    s.write(|tx| {
                        if let Some(t) = token {
                            let c =
                                token::consume(tx, &t, DestructiveOp::CdlLockOff, &target, now)?;
                            if c.is_some() {
                                return Ok(c);
                            }
                        }
                        let before = lock_on(tx)?;
                        tx.conn().execute(
                            "UPDATE install_setting SET value = ?1 WHERE key = ?2",
                            rusqlite::params![if on { "1" } else { "0" }, LOCK_KEY],
                        )?;
                        audit.before = Some(format!("{{\"cdl_sacrificial_only\":{before}}}"));
                        audit.after = Some(format!("{{\"cdl_sacrificial_only\":{on}}}"));
                        tx.append_audit(&audit)?;
                        Ok(None)
                    })
                })
                .map_err(|e| store_denied(&e))?;
            refused.map_or(Ok(()), |rule| Err(rule.deny_default()))
        };
        let res = run();
        res.map_err(|d| self.refuse(ctx, cmd, key, d))
    }

    /// Marks or unmarks a terminal as the sacrificial unit. Marking widens
    /// what CDL can reach, so it is D-hard (owner plus a token for
    /// `MARK SACRIFICIAL <terminal id>`); unmarking is owner-only.
    ///
    /// # Errors
    ///
    /// [`PolicyDenied`] naming the rule.
    pub fn mark_sacrificial(
        &self,
        ctx: &CommandContext,
        terminal: TerminalId,
        sacrificial: bool,
    ) -> Result<(), PolicyDenied> {
        let cmd = "mark-sacrificial";
        let target = TokenTarget::Terminal(terminal);
        let key = Some(target.key());
        let run = || -> Result<(), PolicyDenied> {
            Self::gate_destructive(ctx)?;
            let token = if sacrificial {
                Some(
                    ctx.confirmation
                        .as_ref()
                        .ok_or_else(|| RuleId::TokenMissing.deny_default())?
                        .0
                        .clone(),
                )
            } else {
                None
            };
            let now = self.now_ms();
            let mut audit = Self::entry(ctx, cmd, key.clone(), "ok");
            let tgt = target.clone();
            let res = self
                .store
                .call_blocking(move |s| {
                    s.write(|tx| {
                        let before: Option<bool> = tx
                            .conn()
                            .query_row(
                                "SELECT sacrificial FROM terminal WHERE terminal_id = ?1",
                                [terminal.get()],
                                |r| r.get(0),
                            )
                            .optional()?;
                        let Some(before) = before else {
                            return Ok(Some(RuleId::UnknownTerminal));
                        };
                        if let Some(t) = token {
                            let c =
                                token::consume(tx, &t, DestructiveOp::MarkSacrificial, &tgt, now)?;
                            if c.is_some() {
                                return Ok(c);
                            }
                        }
                        tx.conn().execute(
                            "UPDATE terminal SET sacrificial = ?1 WHERE terminal_id = ?2",
                            rusqlite::params![sacrificial, terminal.get()],
                        )?;
                        audit.before = Some(format!("{{\"sacrificial\":{before}}}"));
                        audit.after = Some(format!("{{\"sacrificial\":{sacrificial}}}"));
                        tx.append_audit(&audit)?;
                        Ok(None)
                    })
                })
                .map_err(|e| store_denied(&e))?;
            res.map_or(Ok(()), |rule| Err(rule.deny_default()))
        };
        run().map_err(|d| self.refuse(ctx, cmd, key.clone(), d))
    }
}

impl Policy for StorePolicy {
    fn check(
        &self,
        ctx: &CommandContext,
        cmd: &Command,
        resolved: &[TerminalRef],
    ) -> Result<Grant, PolicyDenied> {
        if ctx.role == Role::Guest {
            return Err(RuleId::GuestReadOnly.deny_default());
        }
        let Some(need) = classes::need(cmd, resolved) else {
            Self::gate_plain(ctx, cmd)?;
            return Ok(Grant::new());
        };
        Self::gate_destructive(ctx)?;
        if need.op.is_cdl() {
            self.check_cdl_lock(resolved)?;
        }
        let token = ctx
            .confirmation
            .as_ref()
            .ok_or_else(|| RuleId::TokenMissing.deny_default())?;
        self.spend(token, need.op, need.target)?;
        Ok(Grant::new())
    }

    fn audit(&self, ctx: &CommandContext, note: &AuditNote) {
        let target = (note.terminals > 0).then(|| format!("{} terminal(s)", note.terminals));
        self.write_audit(Self::entry(ctx, note.command, target, note.outcome));
    }
}
