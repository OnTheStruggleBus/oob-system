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
//! - **Configuration** (`Command::Admin`, #25) is owner-only; turning a unit
//!   sacrificial (an update or a unit rollback) is the D-hard
//!   `mark-sacrificial` class.
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
use crate::config::{AdminOp, SacrificialChange};
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
    /// A destructive bulk operation asked to repeat or run for a period.
    DestructiveBulkRepeat,
    /// The CDL lock is on and a target is not the sacrificial unit.
    CdlLockNonSacrificial,
    /// No such terminal.
    UnknownTerminal,
    /// The store failed; policy fails closed.
    StoreUnavailable,
    /// The terminal is retired: no token, no destructive operation (#25).
    TerminalRetired,
    /// A configuration change's base revision is not the head, so it cannot
    /// commit; its token is not spent (#25).
    Conflict,
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
            Self::DestructiveBulkRepeat => "destructive-bulk-no-repeat",
            Self::CdlLockNonSacrificial => "cdl-lock-non-sacrificial",
            Self::UnknownTerminal => "unknown-terminal",
            Self::StoreUnavailable => "policy-store-error",
            Self::TerminalRetired => "terminal-retired",
            Self::Conflict => "conflict",
        }
    }

    /// Every rule, for the docs drift test.
    pub const ALL: [Self; 17] = [
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
        Self::DestructiveBulkRepeat,
        Self::CdlLockNonSacrificial,
        Self::UnknownTerminal,
        Self::StoreUnavailable,
        Self::TerminalRetired,
        Self::Conflict,
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
            Self::DestructiveBulkRepeat => {
                "a destructive bulk operation runs once: no repeat-every and no run-for"
            }
            Self::CdlLockNonSacrificial => {
                "the sacrificial-only CDL lock is on and a target is not sacrificial"
            }
            Self::UnknownTerminal => "no such terminal",
            Self::StoreUnavailable => "the policy store failed; refusing (fail closed)",
            Self::TerminalRetired => "the terminal is retired",
            Self::Conflict => {
                "the configuration changed since it was read; read the head and try again \
                 (no token was spent)"
            }
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

    /// Revalidates the target, then spends the token, in **one** writer
    /// transaction (#59 CP-2). The sacrificial flags, the CDL lock, the
    /// token consume and the `authorized` audit row are all read or written
    /// inside that single transaction, so an unmark-sacrificial or a
    /// lock-enable cannot slip in between the check and the spend: whichever
    /// the single writer serializes first wins, and a refusal spends no token.
    /// The `authorized` row shares the transaction too (QA-3): if it cannot
    /// be written, the token is not spent and the command is refused.
    ///
    /// `cdl` is `Some(resolved)` for a CDL class: while the lock is on, the
    /// set must be non-empty (fails closed) and every terminal must be
    /// sacrificial **in the store now**. A flag carried by `resolved` can
    /// only narrow that, never widen it. `must_exist` names a terminal that
    /// has to be in the store (`unknown-terminal` otherwise). With `token`
    /// `None` the checks still run first, then `token-missing` is returned.
    fn spend_checked(
        &self,
        ctx: &CommandContext,
        token: Option<&ConfirmationToken>,
        op: DestructiveOp,
        target: TokenTarget,
        scope: SpendScope,
    ) -> Result<(), PolicyDenied> {
        let (now, tok) = (self.now_ms(), token.map(|t| t.0.clone()));
        let row = Self::entry(ctx, op.name(), Some(target.key()), "authorized");
        let SpendScope {
            cdl,
            must_exist,
            live,
            admin,
        } = scope;
        let denied = self
            .store
            .call_blocking(move |s| {
                s.write(|tx| {
                    // (sacrificial, retired) as the store has it now.
                    let state = |id: TerminalId| -> Result<Option<(bool, bool)>, StoreError> {
                        Ok(tx
                            .conn()
                            .query_row(
                                "SELECT sacrificial, retired_utc IS NOT NULL FROM terminal \
                                 WHERE terminal_id = ?1",
                                [id.get()],
                                |r| Ok((r.get::<_, bool>(0)?, r.get::<_, bool>(1)?)),
                            )
                            .optional()?)
                    };
                    let sac = |id: TerminalId| -> Result<Option<bool>, StoreError> {
                        // A retired unit is never a target, so never
                        // sacrificial for the lock.
                        Ok(state(id)?.map(|(s, retired)| s && !retired))
                    };
                    // QA-5: every terminal the operation names must exist
                    // and be in service, read in the spend transaction.
                    for id in must_exist.iter().chain(live.iter()) {
                        match state(*id)? {
                            None => return Ok(Some(RuleId::UnknownTerminal.deny_default())),
                            Some((_, true)) => {
                                return Ok(Some(
                                    RuleId::TerminalRetired
                                        .deny(format!("terminal ID {} is retired", id.get())),
                                ));
                            }
                            Some((_, false)) => {}
                        }
                    }
                    // QA-10: a configuration change that cannot commit
                    // (stale base, unknown or retired unit) spends nothing.
                    if let Some(op) = &admin
                        && let Some(doomed) = crate::config::admin_precheck(tx, op)?
                    {
                        return Ok(Some(match doomed {
                            crate::config::Doomed::Conflict => RuleId::Conflict.deny_default(),
                            crate::config::Doomed::Retired => {
                                RuleId::TerminalRetired.deny_default()
                            }
                            crate::config::Doomed::Unknown => {
                                RuleId::UnknownTerminal.deny_default()
                            }
                        }));
                    }
                    if let Some(resolved) = cdl
                        && lock_on(tx)?
                    {
                        if resolved.is_empty() {
                            return Ok(Some(RuleId::CdlLockNonSacrificial.deny(
                                "the sacrificial-only CDL lock is on and the target set is \
                                 not resolved; failing closed",
                            )));
                        }
                        let mut now_state = Vec::with_capacity(resolved.len());
                        for t in &resolved {
                            let mut t = t.clone();
                            t.sacrificial = t.sacrificial && sac(t.id)?.unwrap_or(false);
                            now_state.push(t);
                        }
                        let bad = classes::non_sacrificial(&now_state);
                        if !bad.is_empty() {
                            let ids: Vec<String> =
                                bad.iter().map(|t| t.get().to_string()).collect();
                            return Ok(Some(RuleId::CdlLockNonSacrificial.deny(format!(
                                "the sacrificial-only CDL lock is on; not sacrificial: \
                                 terminal ID {}",
                                ids.join(", ")
                            ))));
                        }
                    }
                    let Some(tok) = tok else {
                        return Ok(Some(RuleId::TokenMissing.deny_default()));
                    };
                    if let Some(rule) = token::consume(tx, &tok, op, &target, now)? {
                        return Ok(Some(rule.deny_default()));
                    }
                    tx.append_audit(&row)?;
                    Ok(None)
                })
            })
            .map_err(|e| store_denied(&e))?;
        denied.map_or(Ok(()), Err)
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
            // Terminal targets resolve to the terminal itself; a plan
            // carries no terminals here, so the lock refuses it while on
            // (fails closed) and lets it through to the token while off.
            let (cdl, must_exist) = match &target {
                TokenTarget::Terminal(id) if op.is_cdl() => {
                    let mut t = TerminalRef::new(*id, crate::store::TerminalType::OneWay);
                    // Narrowed to the store's flag inside the transaction.
                    t.sacrificial = true;
                    (Some(vec![t]), Some(*id))
                }
                _ if op.is_cdl() => (Some(Vec::new()), None),
                _ => (None, None),
            };
            // Revalidated inside the spend transaction (#59 CP-2).
            self.spend_checked(
                ctx,
                ctx.confirmation.as_ref(),
                op,
                target.clone(),
                SpendScope {
                    cdl,
                    must_exist,
                    ..SpendScope::default()
                },
            )
        };
        match run() {
            Ok(()) => {
                self.write_audit(Self::entry(ctx, op.name(), key, "ok"));
                Ok(())
            }
            Err(d) => Err(self.refuse(ctx, op.name(), key, d)),
        }
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
    /// `unknown-terminal` (a terminal target that is not in the store),
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
            let terminal = match &req.target {
                TokenTarget::Terminal(id) => Some(*id),
                _ => None,
            };
            let token = self
                .store
                .call_blocking(move |s| {
                    s.write(|tx| {
                        // CP-3: the terminal must exist, checked in the
                        // transaction that inserts the token.
                        if let Some(id) = terminal {
                            let retired: Option<bool> = tx
                                .conn()
                                .query_row(
                                    "SELECT retired_utc IS NOT NULL FROM terminal \
                                     WHERE terminal_id = ?1",
                                    [id.get()],
                                    |r| r.get(0),
                                )
                                .optional()?;
                            match retired {
                                None => return Ok(Err(RuleId::UnknownTerminal)),
                                // QA-5: no token for a retired unit.
                                Some(true) => return Ok(Err(RuleId::TerminalRetired)),
                                Some(false) => {}
                            }
                        }
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
                        Ok(Ok(raw))
                    })
                })
                .map_err(|e| store_denied(&e))?
                .map_err(RuleId::deny_default)?;
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
                        // CP-4: a present but malformed value (anything but
                        // exactly "0" or "1") reads as ON; overwriting it
                        // would report success over a damaged row. Fail
                        // closed instead: the error rolls back the token
                        // spend, the write and the audit row.
                        if let Some(v) = setting(tx, LOCK_KEY)?
                            && v != "0"
                            && v != "1"
                        {
                            return Err(StoreError::Corrupt(format!(
                                "the {LOCK_KEY} setting is malformed; the lock was not changed"
                            )));
                        }
                        let before = lock_on(tx)?;
                        let rows = tx.conn().execute(
                            "UPDATE install_setting SET value = ?1 WHERE key = ?2",
                            rusqlite::params![if on { "1" } else { "0" }, LOCK_KEY],
                        )?;
                        if rows != 1 {
                            // CP-4: a missing setting row would read as ON
                            // while we reported a change. Returning the
                            // error rolls back the token spend and the audit.
                            return Err(StoreError::Corrupt(format!(
                                "the {LOCK_KEY} setting row is missing; the lock was not changed"
                            )));
                        }
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

    /// The gate for a configuration change (`Command::Admin`, #25): the
    /// owner role (an operator is refused; the DAC's own timers cannot
    /// originate one), and when the change turns a unit sacrificial, the
    /// `mark-sacrificial` token bound to that terminal, spent in the same
    /// transaction that revalidates it exists. Unmarking, retiring and
    /// reinstating need the owner role only.
    fn check_admin(&self, ctx: &CommandContext, op: &AdminOp) -> Result<Grant, PolicyDenied> {
        let probe = op.clone();
        let touch = self
            .store
            .call_blocking(move |s| s.write(|tx| crate::config::sacrificial_change(tx, &probe)))
            .map_err(|e| store_denied(&e))?;
        match touch {
            SacrificialChange::None => {
                if ctx.front_door == FrontDoor::Auto {
                    return Err(RuleId::OwnerRequired
                        .deny("the DAC's own timers may issue auto-generated operations only"));
                }
                if ctx.role != Role::Owner {
                    return Err(
                        RuleId::OwnerRequired.deny("a configuration change needs the owner role")
                    );
                }
            }
            // QA-9: unmarking keeps the door gate the removed
            // `mark_sacrificial` had (owner through REST or Lab); A-4: from
            // the timers it is `system-cannot-originate`.
            SacrificialChange::Unmark(_) => Self::gate_destructive(ctx)?,
            SacrificialChange::Mark(id) => {
                Self::gate_destructive(ctx)?;
                self.spend_checked(
                    ctx,
                    ctx.confirmation.as_ref(),
                    DestructiveOp::MarkSacrificial,
                    TokenTarget::Terminal(id),
                    SpendScope {
                        must_exist: Some(id),
                        admin: Some(op.clone()),
                        ..SpendScope::default()
                    },
                )?;
            }
        }
        Ok(Grant::new())
    }
}

/// What a token spend must revalidate in its own transaction.
#[derive(Default)]
struct SpendScope {
    /// For a CDL class: the resolved set the lock checks.
    cdl: Option<Vec<TerminalRef>>,
    /// A terminal that must exist and be in service.
    must_exist: Option<TerminalId>,
    /// Every terminal the operation resolved to: each must exist and be in
    /// service (QA-5).
    live: Vec<TerminalId>,
    /// A configuration change whose save must still be able to commit
    /// (QA-10).
    admin: Option<AdminOp>,
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
        if let Command::Admin(op) = cmd {
            return self.check_admin(ctx, op);
        }
        let Some(need) = classes::need(cmd, resolved) else {
            Self::gate_plain(ctx, cmd)?;
            return Ok(Grant::new());
        };
        Self::gate_destructive(ctx)?;
        if let Command::Operation(def) = cmd
            && (def.repeat_every.is_some() || def.run_for.is_some())
        {
            return Err(RuleId::DestructiveBulkRepeat.deny_default());
        }
        // Lock, sacrificial flags and token spend in one transaction
        // (#59 CP-2); `resolved` flags are revalidated against the store.
        self.spend_checked(
            ctx,
            ctx.confirmation.as_ref(),
            need.op,
            need.target,
            SpendScope {
                cdl: need.op.is_cdl().then(|| resolved.to_vec()),
                must_exist: None,
                live: resolved.iter().map(|t| t.id).collect(),
                admin: None,
            },
        )?;
        Ok(Grant::new())
    }

    fn audit(&self, ctx: &CommandContext, note: &AuditNote) {
        self.write_audit(Self::entry(
            ctx,
            note.operation,
            note.target.clone(),
            note.outcome,
        ));
    }
}
