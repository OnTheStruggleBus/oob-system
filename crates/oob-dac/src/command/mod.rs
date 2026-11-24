//! The command layer: the one internal API every front door adapts over.
//!
//! r6 F6-11 and ADR-002 decision 1: **every operation enters `oob-dac`
//! through [`CommandBus::submit`]**: the REST API (#27), the WireLink-wire
//! listener (#28), billing, the Discovery Lab (decision 7: lab injection is
//! C5 through the same path) and the timers that issue auto-generated
//! operations. Adapters hold a `CommandBus` and nothing else. The bus is
//! also the **only** path into the `oob-mux` scheduler, and the only place
//! campaign spacing exists (as per-PDU release times, [`campaign`]).
//!
//! # What is here
//!
//! | Module | Role |
//! | --- | --- |
//! | [`catalog`] | the DAC §11 per-terminal ops, Define Terminal actions and Define Operations kinds, each with a support status |
//! | [`target`] | All / Terminal-ID range / Single plus the Plan §2 filters, over a [`TerminalDirectory`] (store-backed or fake) |
//! | [`lifecycle`] | the operation status state machine (`documented` names, `hypothesized` transitions) |
//! | [`campaign`] | release-time planning: delay ≥ 250 ms, Repeat-Every |
//! | [`policy`] | the hook #23 implements |
//! | [`exec`] | encoding under `catch_unwind` |
//!
//! # Pipeline
//!
//! 1. **Validate**: catalog support, then parameters (delay ≥ 250 ms,
//!    Repeat-Every covering one campaign).
//! 2. **Resolve** the targets through the directory; empty is an error.
//! 3. **Policy** ([`Policy::check`]); a dry run stops after this and
//!    returns a [`DryRunReport`], so nothing reaches `oob-mux`.
//! 4. **Plan** release times, `t0 + i × delay`, from the bus clock.
//! 5. **Encode** each PDU under `catch_unwind`, then **submit** it.
//! 6. **Track**: scheduler [`OutcomeEvent`]s feed
//!    [`CommandBus::on_outcome`], which drives the lifecycle.
//!
//! # Non-PDU commands (the seam for #25)
//!
//! [`Command`] and [`CommandError`] are `#[non_exhaustive]`, and a command
//! that produces no PDU is dispatched to a [`NonPduHandler`] after the same
//! policy check. #25 adds `Command::Admin(AdminOp)` (configuration saves) by
//! adding a variant and registering its handler: owner role, guest refusal
//! and audit from #23 then apply to config saves with no change to the bus.
//! [`ControlCommand`] is the stub that proves the path today.
//!
//! # What is not here yet
//!
//! - **Persistence.** Operations are tracked in memory. The `operation`
//!   ledger table (precious, F6-5), the audit rows and restart-resume need a
//!   store migration and land with the policy layer (#23) and the daemon
//!   (#29); until then a restart forgets in-flight operations.
//! - **One-shot withdrawal.** `oob-mux` can withdraw carousels but not
//!   queued one-shot PDUs. Every addressed PDU therefore carries a deadline
//!   ([`BusConfig::pdu_deadline`]) after which the scheduler drops and
//!   reports it, bounding what a failed or ended operation leaves queued.
//!
//! See `architecture/oob-dac-command.md` in the book.

pub mod campaign;
pub mod catalog;
pub mod exec;
pub mod lifecycle;
pub mod policy;
pub mod target;

mod sink;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use oob_mux::{
    Clock, MonoTime, Outcome, OutcomeEvent, Pdu, PduId, SchedulerHandle, Submission, SubmitError,
    TrafficClass,
};

use crate::store::{StoreError, TerminalId};

use self::campaign::{
    MIN_INTER_TERMINAL_DELAY, plan_release_times, validate_delay, validate_repeat,
};
use self::catalog::{AutoOp, CatalogEntry, OperationKind, Support, TerminalOp, auto_class};
use self::exec::{EncodeFailure, Encoder, Work, encode_guarded};
use self::lifecycle::{OpState, OpStatus, Overlay};
use self::policy::{AuditNote, Policy, PolicyDenied};
use self::sink::{PduSink, SchedulerSink};
use self::target::{Filters, Target, TerminalDirectory, TerminalRef};

#[cfg(test)]
mod tests;

/// Identifies one tracked operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationId(pub u64);

/// Which front door a command came through (for audit and policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrontDoor {
    /// The REST admin API.
    Rest,
    /// The WireLink-wire listener.
    WireLinkWire,
    /// The billing interface.
    Billing,
    /// The Discovery Lab harness.
    Lab,
    /// The DAC's own timers (auto-generated operations).
    Auto,
}

/// The caller's role. #23 defines what each may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Role {
    /// The install owner.
    Owner,
    /// An operator.
    Operator,
    /// Read-only; policy refuses writes.
    Guest,
}

/// A short-lived confirmation token for destructive operations (r6
/// F6-10). Opaque here; #23 validates it. Its `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct ConfirmationToken(pub String);

impl fmt::Debug for ConfirmationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConfirmationToken(..)")
    }
}

/// Who is asking, in what role, through which door.
#[derive(Debug, Clone)]
pub struct CommandContext {
    /// The actor's name (audit).
    pub actor: String,
    /// The actor's role.
    pub role: Role,
    /// The entry point.
    pub front_door: FrontDoor,
    /// A confirmation token, where the operation needs one.
    pub confirmation: Option<ConfirmationToken>,
}

/// A user-defined or imported message template's name (Layer 2, #19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateId(pub String);

/// A Discovery Lab injection: one template to the targets, in C5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabInjection {
    /// The template to inject.
    pub template: TemplateId,
    /// Who receives it.
    pub target: Target,
    /// Narrowing filters.
    pub filters: Filters,
}

/// A Define Operations entry (Plan §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefineOperation {
    /// What to run.
    pub kind: OperationKind,
    /// All / Terminal-ID range / Single.
    pub target: Target,
    /// The Plan §2 filters.
    pub filters: Filters,
    /// Delay between terminals; at least 250 ms.
    pub delay_between: Duration,
    /// Repeat the whole campaign every period.
    pub repeat_every: Option<Duration>,
    /// Stop after this long; an operation still Active then is Marked
    /// Completed (p. 11-4).
    pub run_for: Option<Duration>,
    /// Report what would happen and send nothing. Bulk operations default
    /// to this in the Console; #23 enforces the default (F6-10).
    pub dry_run: bool,
}

/// A command that produces no PDU.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControlCommand {
    /// Answers `Done`; proves a command can cross the bus without a PDU.
    Ping,
    /// Cancels a non-final operation (it fails, with no more specific
    /// status). Handled by the bus itself.
    Cancel(OperationId),
}

/// Everything that can enter the command layer.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Command {
    /// One per-terminal operation (§11 and the Define Terminal actions).
    Terminal {
        /// The operation.
        op: TerminalOp,
        /// The terminal.
        terminal: TerminalId,
    },
    /// A Define Operations entry.
    Operation(DefineOperation),
    /// An auto-generated operation (issued by the DAC's own timers).
    Auto(AutoOp),
    /// A Discovery Lab injection (C5). Needs lab mode.
    Lab(LabInjection),
    /// A command that produces no PDU, dispatched through the seam.
    Control(ControlCommand),
}

impl Command {
    /// A stable kind name for audit rows.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Terminal { .. } => "terminal",
            Self::Operation(_) => "operation",
            Self::Auto(_) => "auto",
            Self::Lab(_) => "lab",
            Self::Control(_) => "control",
        }
    }
}

/// What a non-PDU command is dispatched to. #25 registers the handler for
/// `Command::Admin`; the bus has already applied the policy check when
/// [`handle`](Self::handle) runs.
///
/// A non-PDU command crossing the bus (the stub `ControlCommand::Ping`):
///
/// ```
/// use std::sync::Arc;
/// use oob_dac::command::policy::AllowAllPolicy;
/// use oob_dac::command::target::InMemoryDirectory;
/// use oob_dac::command::exec::{EncodeError, Encoder, Work};
/// use oob_dac::command::target::TerminalRef;
/// use oob_dac::command::{
///     Command, CommandBus, CommandContext, CommandError, ControlCommand, FrontDoor,
///     NonPduHandler, Role, Submitted,
/// };
/// use oob_mux::{ManualClock, Pdu, Scheduler, SchedulerConfig};
///
/// struct NoEncoder;
/// impl Encoder for NoEncoder {
///     fn encode(&self, _: &Work, _: Option<&TerminalRef>) -> Result<Pdu, EncodeError> {
///         Err(EncodeError { reason: "none".into() })
///     }
/// }
/// struct Answer;
/// impl NonPduHandler for Answer {
///     fn handle(&self, _: &CommandContext, _: &Command) -> Result<Submitted, CommandError> {
///         Ok(Submitted::Done)
///     }
/// }
///
/// let (_sched, handle, _outcomes) = Scheduler::new(SchedulerConfig {
///     queue_depth: 8, max_carousels: 1, outcome_capacity: 8, disabled_classes: vec![],
/// }).unwrap();
/// let bus = CommandBus::new(
///     handle,
///     Arc::new(AllowAllPolicy), // a test double; no adapter may ship with it
///     Arc::new(InMemoryDirectory::default()),
///     Arc::new(NoEncoder),
///     Arc::new(ManualClock::default()),
/// )
/// .with_non_pdu_handler(Arc::new(Answer));
/// let ctx = CommandContext {
///     actor: "doc".into(), role: Role::Owner, front_door: FrontDoor::Rest, confirmation: None,
/// };
/// assert_eq!(
///     bus.submit(&ctx, Command::Control(ControlCommand::Ping)).unwrap(),
///     Submitted::Done,
/// );
/// ```
pub trait NonPduHandler: Send + Sync {
    /// Handles `cmd`, a command the bus does not turn into PDUs.
    ///
    /// # Errors
    ///
    /// Any [`CommandError`]; a variant the handler does not know should be
    /// [`CommandError::Unsupported`].
    fn handle(&self, ctx: &CommandContext, cmd: &Command) -> Result<Submitted, CommandError>;
}

/// The built-in handler: answers [`ControlCommand::Ping`], refuses the rest
/// by name.
struct DefaultHandler;

impl NonPduHandler for DefaultHandler {
    fn handle(&self, _ctx: &CommandContext, cmd: &Command) -> Result<Submitted, CommandError> {
        match cmd {
            Command::Control(ControlCommand::Ping) => Ok(Submitted::Done),
            _ => Err(CommandError::Unsupported {
                what: "non-pdu-command",
                reason: "no handler is registered for it",
            }),
        }
    }
}

/// What a successful [`CommandBus::submit`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Submitted {
    /// Accepted; track it with [`CommandBus::status`].
    Queued(OperationId),
    /// A dry run: nothing was sent.
    DryRun(DryRunReport),
    /// A non-PDU command completed.
    Done,
}

/// What a dry run would have done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunReport {
    /// How many terminals it resolved to.
    pub terminals: usize,
    /// When the first PDU would be released.
    pub first_release: MonoTime,
    /// When the last PDU would be released.
    pub last_release: MonoTime,
    /// The traffic class.
    pub class: TrafficClass,
    /// The message family.
    pub family: String,
}

/// Why a command was refused or failed. Every variant has a stable
/// [`reason`](Self::reason) code for REST and WireLink errors.
#[derive(Debug)]
#[non_exhaustive]
pub enum CommandError {
    /// In the catalog, not accepted here, with the reason.
    Unsupported {
        /// The entry or filter.
        what: &'static str,
        /// Why it is rejected.
        reason: &'static str,
    },
    /// The targets resolved to nothing.
    EmptyTarget,
    /// A `Single` ID the directory does not hold.
    UnknownTerminal(TerminalId),
    /// Inter-terminal delay below the 250 ms minimum.
    DelayBelowMinimum {
        /// What was asked.
        got: Duration,
        /// The minimum.
        min: Duration,
    },
    /// Repeat-Every shorter than one campaign.
    RepeatTooShort {
        /// What was asked.
        got: Duration,
        /// The shortest allowed (`terminals × delay`).
        min: Duration,
    },
    /// Policy (#23) refused.
    Policy(PolicyDenied),
    /// A lab injection while lab mode is off.
    LabModeOff,
    /// An encoder returned an error.
    Encode {
        /// The family.
        work: String,
        /// The encoder's reason.
        reason: String,
    },
    /// An encoder panicked; the command failed, the process continues.
    EncoderPanic {
        /// The family.
        work: String,
    },
    /// The scheduler refused a PDU.
    Scheduler(SubmitError),
    /// The store failed.
    Store(StoreError),
    /// A status transition the lifecycle table does not allow.
    IllegalTransition {
        /// Current status.
        from: OpStatus,
        /// Requested status.
        to: OpStatus,
    },
    /// An overlay on a final or New operation.
    IllegalOverlay {
        /// The operation's status.
        status: OpStatus,
    },
    /// No such operation.
    UnknownOperation(OperationId),
}

impl CommandError {
    /// A stable code naming the refusal.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Unsupported { .. } => "unsupported",
            Self::EmptyTarget => "empty-target",
            Self::UnknownTerminal(_) => "unknown-terminal",
            Self::DelayBelowMinimum { .. } => "delay-below-minimum",
            Self::RepeatTooShort { .. } => "repeat-too-short",
            Self::Policy(p) => p.reason,
            Self::LabModeOff => "lab-mode-off",
            Self::Encode { .. } => "encode-failed",
            Self::EncoderPanic { .. } => "encoder-panic",
            Self::Scheduler(_) => "scheduler-refused",
            Self::Store(_) => "store-error",
            Self::IllegalTransition { .. } => "illegal-transition",
            Self::IllegalOverlay { .. } => "illegal-overlay",
            Self::UnknownOperation(_) => "unknown-operation",
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { what, reason } => write!(f, "{what} is unsupported: {reason}"),
            Self::EmptyTarget => f.write_str("the target resolved to no terminals"),
            Self::UnknownTerminal(id) => write!(f, "no terminal {}", id.get()),
            Self::DelayBelowMinimum { got, min } => {
                write!(
                    f,
                    "inter-terminal delay {got:?} is below the {min:?} minimum"
                )
            }
            Self::RepeatTooShort { got, min } => {
                write!(
                    f,
                    "Repeat-Every {got:?} is shorter than one campaign ({min:?})"
                )
            }
            Self::Policy(p) => write!(f, "refused by policy ({}): {}", p.reason, p.detail),
            Self::LabModeOff => f.write_str("lab mode is off"),
            Self::Encode { work, reason } => write!(f, "could not encode {work}: {reason}"),
            Self::EncoderPanic { work } => write!(f, "the {work} encoder panicked"),
            Self::Scheduler(e) => write!(f, "scheduler refused: {e}"),
            Self::Store(e) => write!(f, "store error: {e}"),
            Self::IllegalTransition { from, to } => {
                write!(f, "illegal transition {} -> {}", from.label(), to.label())
            }
            Self::IllegalOverlay { status } => {
                write!(f, "no overlay on a {} operation", status.label())
            }
            Self::UnknownOperation(id) => write!(f, "no operation {}", id.0),
        }
    }
}

impl std::error::Error for CommandError {}

/// Bus tunables (runtime settings, #25).
#[derive(Debug, Clone)]
pub struct BusConfig {
    /// How long after its release time a queued addressed PDU may wait
    /// before the scheduler drops it as expired.
    pub pdu_deadline: Duration,
    /// Immediate retries after `QueueFull` before the operation times out.
    pub queue_full_retries: u32,
}

impl Default for BusConfig {
    fn default() -> Self {
        Self {
            pdu_deadline: Duration::from_secs(60),
            queue_full_retries: 3,
        }
    }
}

/// A read-only view of one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationView {
    /// Its ID.
    pub id: OperationId,
    /// The message family.
    pub family: String,
    /// The main status.
    pub status: OpStatus,
    /// The overlay, if any.
    pub overlay: Option<Overlay>,
    /// Terminals addressed (0 for auto-generated work).
    pub terminals: usize,
    /// The current repetition, from 1.
    pub repetition: u32,
    /// PDUs of the current repetition accepted and not yet reported.
    pub outstanding: usize,
    /// PDUs of the current repetition emitted.
    pub emitted: usize,
    /// Why it failed, once it has.
    pub failure: Option<String>,
}

/// One line of the command log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// The operation concerned, if any.
    pub operation: Option<OperationId>,
    /// A stable event name (`encoder-panic`, `scheduler-refused`, ...).
    pub event: &'static str,
    /// Detail text. Never a unit address or serial (ADR-003 §4).
    pub detail: String,
}

struct Record {
    family: String,
    work: Work,
    class: TrafficClass,
    slots: Vec<Option<TerminalRef>>,
    delay: Duration,
    repeat_every: Option<Duration>,
    end_at: Option<MonoTime>,
    state: OpState,
    repetition: u32,
    outstanding: usize,
    emitted: usize,
    rep_started: MonoTime,
    next_repeat_at: Option<MonoTime>,
    failure: Option<String>,
}

#[derive(Default)]
struct Inner {
    next_id: u64,
    ops: BTreeMap<OperationId, Record>,
    pdus: HashMap<PduId, OperationId>,
    log: Vec<LogEntry>,
}

impl Inner {
    fn log(&mut self, operation: Option<OperationId>, event: &'static str, detail: String) {
        self.log.push(LogEntry {
            operation,
            event,
            detail,
        });
    }
}

/// The command bus: the one entry point, and the only holder of the
/// scheduler handle.
///
/// The sink module and the encode guard are private to the command layer,
/// so an adapter cannot reach the scheduler around the bus:
///
/// ```compile_fail,E0603
/// use oob_dac::command::sink::PduSink;
/// ```
///
/// ```compile_fail,E0603
/// use oob_dac::command::exec::encode_guarded;
/// ```
pub struct CommandBus {
    sink: Box<dyn PduSink>,
    policy: Arc<dyn Policy>,
    directory: Arc<dyn TerminalDirectory>,
    encoder: Arc<dyn Encoder>,
    clock: Arc<dyn Clock>,
    handler: Arc<dyn NonPduHandler>,
    cfg: BusConfig,
    lab_mode: AtomicBool,
    link_up: AtomicBool,
    inner: Mutex<Inner>,
}

/// How a start (or restart) of a repetition failed.
enum StartFailure {
    Encode(String),
    Panic(String),
    Refused(SubmitError),
    QueueFull(SubmitError),
}

impl CommandBus {
    /// A bus over a real scheduler. The handle is **moved in**: this is the
    /// only way a `SchedulerHandle` enters oob-dac, so the command layer is
    /// the only path to the scheduler. There is deliberately no default
    /// policy: pass the one your adapter is allowed to ship with.
    pub fn new(
        handle: SchedulerHandle,
        policy: Arc<dyn Policy>,
        directory: Arc<dyn TerminalDirectory>,
        encoder: Arc<dyn Encoder>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self::from_sink(
            Box::new(SchedulerSink(handle)),
            policy,
            directory,
            encoder,
            clock,
        )
    }

    fn from_sink(
        sink: Box<dyn PduSink>,
        policy: Arc<dyn Policy>,
        directory: Arc<dyn TerminalDirectory>,
        encoder: Arc<dyn Encoder>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            sink,
            policy,
            directory,
            encoder,
            clock,
            handler: Arc::new(DefaultHandler),
            cfg: BusConfig::default(),
            lab_mode: AtomicBool::new(false),
            link_up: AtomicBool::new(true),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Replaces the non-PDU handler (the seam #25 plugs into).
    #[must_use]
    pub fn with_non_pdu_handler(mut self, h: Arc<dyn NonPduHandler>) -> Self {
        self.handler = h;
        self
    }

    /// Replaces the tunables.
    #[must_use]
    pub fn with_config(mut self, cfg: BusConfig) -> Self {
        self.cfg = cfg;
        self
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Turns lab mode on or off (C5 injection needs it, #16).
    pub fn set_lab_mode(&self, on: bool) {
        self.lab_mode.store(on, Ordering::Release);
    }

    /// Records whether the PHY seam has a connected puller (from
    /// `SeamStats`, #15). While it has none, non-final operations show
    /// the Device Offline overlay; they return to their status when it
    /// reconnects.
    pub fn set_link(&self, up: bool) {
        self.link_up.store(up, Ordering::Release);
        let mut g = self.lock();
        for rec in g.ops.values_mut() {
            apply_link(&mut rec.state, up);
        }
    }

    /// Submits a command. Every front door calls this and nothing else.
    ///
    /// The outcome of every command, including refusals, is passed to
    /// [`Policy::audit`].
    ///
    /// # Errors
    ///
    /// See [`CommandError`]. An encoder error or panic fails the command
    /// (its operation is left in Request Sent Failed and logged) and
    /// returns `Err`; the process is unaffected.
    pub fn submit(&self, ctx: &CommandContext, cmd: Command) -> Result<Submitted, CommandError> {
        let mut resolved = 0;
        let result = self.dispatch(ctx, &cmd, &mut resolved);
        let outcome = match &result {
            Ok(Submitted::DryRun(_)) => "dry-run",
            Ok(_) => "ok",
            Err(e) => e.reason(),
        };
        self.policy.audit(
            ctx,
            &AuditNote {
                command: cmd.kind_name(),
                terminals: resolved,
                outcome,
            },
        );
        result
    }

    /// Cancels a non-final operation: sugar for submitting
    /// `ControlCommand::Cancel`.
    ///
    /// # Errors
    ///
    /// As [`submit`](Self::submit).
    pub fn cancel(&self, ctx: &CommandContext, id: OperationId) -> Result<(), CommandError> {
        self.submit(ctx, Command::Control(ControlCommand::Cancel(id)))
            .map(|_| ())
    }

    fn resolve(&self, t: &Target, f: &Filters) -> Result<Vec<TerminalRef>, CommandError> {
        self.directory.resolve(t, f)
    }

    fn admit(
        &self,
        ctx: &CommandContext,
        cmd: &Command,
        resolved: &[TerminalRef],
    ) -> Result<(), CommandError> {
        self.policy
            .check(ctx, cmd, resolved)
            .map(|_grant| ())
            .map_err(CommandError::Policy)
    }

    fn dispatch(
        &self,
        ctx: &CommandContext,
        cmd: &Command,
        resolved_count: &mut usize,
    ) -> Result<Submitted, CommandError> {
        match cmd {
            Command::Terminal { op, terminal } => {
                require(op.name(), op.support())?;
                let refs = self.resolve(&Target::Single(*terminal), &Filters::default())?;
                *resolved_count = refs.len();
                self.admit(ctx, cmd, &refs)?;
                self.run(Spec {
                    work: Work::Terminal(*op),
                    class: TrafficClass::C1Addressed,
                    slots: refs.into_iter().map(Some).collect(),
                    delay: MIN_INTER_TERMINAL_DELAY,
                    repeat_every: None,
                    run_for: None,
                })
            }
            Command::Operation(def) => {
                require(def.kind.name(), def.kind.support())?;
                validate_delay(def.delay_between)?;
                let refs = self.resolve(&def.target, &def.filters)?;
                *resolved_count = refs.len();
                validate_repeat(def.repeat_every, def.delay_between, refs.len())?;
                self.admit(ctx, cmd, &refs)?;
                let work = match def.kind {
                    OperationKind::Terminal(op) => Work::Terminal(op),
                    k => Work::Operation(k),
                };
                if def.dry_run {
                    let t0 = self.clock.mono();
                    let times = plan_release_times(t0, refs.len(), def.delay_between);
                    return Ok(Submitted::DryRun(DryRunReport {
                        terminals: refs.len(),
                        first_release: times.first().copied().unwrap_or(t0),
                        last_release: times.last().copied().unwrap_or(t0),
                        class: TrafficClass::C1Addressed,
                        family: work.name(),
                    }));
                }
                self.run(Spec {
                    work,
                    class: TrafficClass::C1Addressed,
                    slots: refs.into_iter().map(Some).collect(),
                    delay: def.delay_between,
                    repeat_every: def.repeat_every,
                    run_for: def.run_for,
                })
            }
            Command::Auto(op) => {
                require(op.name(), op.support())?;
                self.admit(ctx, cmd, &[])?;
                self.run(Spec {
                    work: Work::Auto(*op),
                    class: auto_class(*op),
                    slots: vec![None],
                    delay: MIN_INTER_TERMINAL_DELAY,
                    repeat_every: None,
                    run_for: None,
                })
            }
            Command::Lab(inj) => {
                if !self.lab_mode.load(Ordering::Acquire) {
                    return Err(CommandError::LabModeOff);
                }
                let refs = self.resolve(&inj.target, &inj.filters)?;
                *resolved_count = refs.len();
                self.admit(ctx, cmd, &refs)?;
                self.run(Spec {
                    work: Work::Lab(inj.template.0.clone()),
                    class: TrafficClass::C5Lab,
                    slots: refs.into_iter().map(Some).collect(),
                    delay: MIN_INTER_TERMINAL_DELAY,
                    repeat_every: None,
                    run_for: None,
                })
            }
            Command::Control(ControlCommand::Cancel(id)) => {
                self.admit(ctx, cmd, &[])?;
                self.cancel_op(*id).map(|()| Submitted::Done)
            }
            // The seam: every other command is non-PDU. A new `Command`
            // variant (#25's `Admin`) must be routed above or lands here.
            Command::Control(_) => {
                self.admit(ctx, cmd, &[])?;
                self.handler.handle(ctx, cmd)
            }
        }
    }

    fn cancel_op(&self, id: OperationId) -> Result<(), CommandError> {
        let mut g = self.lock();
        let rec = g
            .ops
            .get_mut(&id)
            .ok_or(CommandError::UnknownOperation(id))?;
        rec.state.transition(OpStatus::Failed)?;
        rec.failure = Some("cancelled".to_owned());
        g.log(Some(id), "cancelled", String::new());
        Ok(())
    }

    /// Creates the record and starts repetition 1.
    fn run(&self, spec: Spec) -> Result<Submitted, CommandError> {
        let now = self.clock.mono();
        let id = {
            let mut g = self.lock();
            let id = OperationId(g.next_id);
            g.next_id += 1;
            let mut state = OpState::new();
            // New -> Request Sent: read and initiated.
            state.transition(OpStatus::RequestSent)?;
            g.ops.insert(
                id,
                Record {
                    family: spec.work.name(),
                    work: spec.work,
                    class: spec.class,
                    slots: spec.slots,
                    delay: spec.delay,
                    repeat_every: spec.repeat_every,
                    end_at: spec.run_for.map(|d| now.saturating_add(d)),
                    state,
                    repetition: 0,
                    outstanding: 0,
                    emitted: 0,
                    rep_started: now,
                    next_repeat_at: None,
                    failure: None,
                },
            );
            id
        };
        self.start_repetition(id, now)?;
        Ok(Submitted::Queued(id))
    }

    /// Plans, encodes and submits one repetition of `id` starting at `t0`.
    fn start_repetition(&self, id: OperationId, t0: MonoTime) -> Result<(), CommandError> {
        let (work, class, slots, delay) = {
            let mut g = self.lock();
            let rec = g
                .ops
                .get_mut(&id)
                .ok_or(CommandError::UnknownOperation(id))?;
            if rec.state.status().is_final() {
                return Ok(());
            }
            rec.repetition += 1;
            rec.rep_started = t0;
            rec.next_repeat_at = None;
            rec.emitted = 0;
            (rec.work.clone(), rec.class, rec.slots.clone(), rec.delay)
        };
        let broadcast = slots.len() == 1 && slots[0].is_none();
        let times = plan_release_times(t0, slots.len(), delay);

        // Encode everything first, outside the lock, each under catch_unwind.
        let mut pdus: Vec<Pdu> = Vec::with_capacity(slots.len());
        for (slot, release) in slots.iter().zip(&times) {
            match encode_guarded(&*self.encoder, &work, slot.as_ref()) {
                Ok(mut pdu) => {
                    pdu.class = class;
                    // Spacing exists only here, as a release time (F6-11).
                    pdu.release_at = (!broadcast).then_some(*release);
                    pdu.deadline =
                        (!broadcast).then(|| release.saturating_add(self.cfg.pdu_deadline));
                    pdus.push(pdu);
                }
                Err(EncodeFailure::Error(r)) => {
                    return self.fail_start(id, &work, StartFailure::Encode(r));
                }
                Err(EncodeFailure::Panicked(m)) => {
                    return self.fail_start(id, &work, StartFailure::Panic(m));
                }
            }
        }

        // Submit under the lock so an outcome cannot overtake the index.
        let mut g = self.lock();
        let mut failure = None;
        for pdu in pdus {
            let mut tries = 0;
            let accepted = loop {
                match self.sink.submit(Submission::Once(pdu.clone())) {
                    Ok(pid) => break Ok(pid),
                    Err(SubmitError::QueueFull { .. }) if tries < self.cfg.queue_full_retries => {
                        tries += 1;
                        std::thread::yield_now();
                    }
                    Err(e @ SubmitError::QueueFull { .. }) => {
                        break Err(StartFailure::QueueFull(e));
                    }
                    Err(e) => break Err(StartFailure::Refused(e)),
                }
            };
            match accepted {
                Ok(pid) => {
                    g.pdus.insert(pid, id);
                    if let Some(rec) = g.ops.get_mut(&id) {
                        rec.outstanding += 1;
                    }
                }
                Err(f) => {
                    failure = Some(f);
                    break;
                }
            }
        }
        if let Some(f) = failure {
            drop(g);
            return self.fail_start(id, &work, f);
        }
        let link_up = self.link_up.load(Ordering::Acquire);
        if let Some(rec) = g.ops.get_mut(&id)
            && !rec.state.status().is_final()
        {
            // Every PDU of the repetition is accepted: Downloaded.
            if rec.state.status() == OpStatus::RequestSent {
                rec.state.transition(OpStatus::Downloaded)?;
            }
            apply_link(&mut rec.state, link_up);
        }
        Ok(())
    }

    /// Fails an operation whose repetition could not start: Request Sent
    /// Failed or Timeout on the first repetition (still Request Sent),
    /// Failed afterwards.
    fn fail_start(
        &self,
        id: OperationId,
        work: &Work,
        why: StartFailure,
    ) -> Result<(), CommandError> {
        let (event, text, to_first, err) = match why {
            StartFailure::Encode(r) => (
                "encode-error",
                r.clone(),
                OpStatus::RequestSentFailed,
                CommandError::Encode {
                    work: work.name(),
                    reason: r,
                },
            ),
            StartFailure::Panic(m) => (
                "encoder-panic",
                m,
                OpStatus::RequestSentFailed,
                CommandError::EncoderPanic { work: work.name() },
            ),
            StartFailure::Refused(e) => (
                "scheduler-refused",
                e.to_string(),
                OpStatus::RequestSentFailed,
                CommandError::Scheduler(e),
            ),
            StartFailure::QueueFull(e) => (
                "scheduler-queue-full",
                e.to_string(),
                OpStatus::Timeout,
                CommandError::Scheduler(e),
            ),
        };
        let mut g = self.lock();
        g.log(Some(id), event, format!("{}: {text}", work.name()));
        if let Some(rec) = g.ops.get_mut(&id) {
            let to = if rec.state.status() == OpStatus::RequestSent {
                to_first
            } else {
                OpStatus::Failed
            };
            if !rec.state.status().is_final() {
                let _ = rec.state.transition(to);
            }
            rec.failure = Some(text);
        }
        Err(err)
    }

    /// Feeds one scheduler outcome into the lifecycle. The daemon (#29)
    /// calls this for every [`OutcomeEvent`] it drains.
    pub fn on_outcome(&self, ev: OutcomeEvent) {
        let mut g = self.lock();
        let Some(id) = g.pdus.remove(&ev.id) else {
            return; // not ours (a carousel) or already forgotten
        };
        let Some(rec) = g.ops.get_mut(&id) else {
            return;
        };
        if rec.state.status().is_final() {
            return; // cancelled, failed or closed out; late outcomes are moot
        }
        rec.outstanding = rec.outstanding.saturating_sub(1);
        match ev.outcome {
            Outcome::Emitted { .. } => {
                rec.emitted += 1;
                if rec.state.status() == OpStatus::Downloaded {
                    let _ = rec.state.transition(OpStatus::Active);
                }
                if rec.outstanding == 0 && rec.state.status() == OpStatus::Active {
                    match rec.repeat_every {
                        Some(p) => {
                            // Between repetitions: Active -> Downloaded.
                            let _ = rec.state.transition(OpStatus::Downloaded);
                            rec.next_repeat_at = Some(rec.rep_started.saturating_add(p));
                        }
                        None => {
                            let _ = rec.state.transition(OpStatus::Completed);
                        }
                    }
                }
            }
            Outcome::Expired => {
                rec.failure = Some("a PDU expired before its first packet".to_owned());
                let _ = rec.state.transition(OpStatus::Failed);
                g.log(Some(id), "pdu-expired", String::new());
            }
            _ => {
                rec.failure = Some("a PDU was dropped".to_owned());
                let _ = rec.state.transition(OpStatus::Failed);
                g.log(Some(id), "pdu-dropped", String::new());
            }
        }
    }

    /// Advances time-driven state: starts due Repeat-Every repetitions and
    /// closes out operations past their end time (Marked Completed). The
    /// daemon calls it periodically; tests call it after moving the clock.
    pub fn tick(&self) {
        let now = self.clock.mono();
        let mut due = Vec::new();
        {
            let mut g = self.lock();
            let ids: Vec<OperationId> = g.ops.keys().copied().collect();
            for id in ids {
                let Some(rec) = g.ops.get_mut(&id) else {
                    continue;
                };
                if rec.state.status().is_final() {
                    continue;
                }
                let between = rec.outstanding == 0 && rec.next_repeat_at.is_some();
                let past_end = rec.end_at.is_some_and(|e| now >= e);
                let active = rec.state.status() == OpStatus::Active;
                if past_end && (active || (between && rec.state.status() == OpStatus::Downloaded)) {
                    // A repeating event between repetitions is still "active"
                    // in the manual's sense: Downloaded -> Active -> Marked
                    // Completed.
                    if !active {
                        let _ = rec.state.transition(OpStatus::Active);
                    }
                    let _ = rec.state.transition(OpStatus::MarkedCompleted);
                    rec.next_repeat_at = None;
                    g.log(Some(id), "marked-completed", String::new());
                } else if between
                    && rec.state.status() == OpStatus::Downloaded
                    && rec.next_repeat_at.is_some_and(|t| t <= now)
                {
                    due.push((id, rec.next_repeat_at.unwrap_or(now)));
                }
            }
        }
        for (id, at) in due {
            // The release times are planned from the slot, not from a late
            // tick, so spacing is unchanged; a failure is logged and recorded.
            let _ = self.start_repetition(id, at.max(now));
        }
    }

    /// The Retrieve Operation Status control: reconciles the ledger with
    /// what the scheduler reported. *Retrieving Status* is entered and left
    /// inside the call, so it is never what a caller reads back; the result
    /// carries the overlay the reconciliation found (Unknown, Device
    /// Offline) or none.
    ///
    /// # Errors
    ///
    /// [`CommandError::UnknownOperation`].
    pub fn retrieve_status(&self, id: OperationId) -> Result<OperationView, CommandError> {
        let link_up = self.link_up.load(Ordering::Acquire);
        let mut g = self.lock();
        let rec = g
            .ops
            .get_mut(&id)
            .ok_or(CommandError::UnknownOperation(id))?;
        if rec.state.set_overlay(Overlay::RetrievingStatus).is_ok() {
            let status = rec.state.status();
            let silent = rec.outstanding == 0
                && rec.emitted == 0
                && rec.next_repeat_at.is_none()
                && matches!(status, OpStatus::Downloaded | OpStatus::Active);
            rec.state.clear_overlay();
            if silent {
                let _ = rec.state.set_overlay(Overlay::Unknown);
            } else {
                apply_link(&mut rec.state, link_up);
            }
        }
        Ok(view(id, rec))
    }

    /// One operation's view.
    ///
    /// # Errors
    ///
    /// [`CommandError::UnknownOperation`].
    pub fn status(&self, id: OperationId) -> Result<OperationView, CommandError> {
        let g = self.lock();
        g.ops
            .get(&id)
            .map(|r| view(id, r))
            .ok_or(CommandError::UnknownOperation(id))
    }

    /// Every operation, by ID.
    pub fn operations(&self) -> Vec<OperationView> {
        let g = self.lock();
        g.ops.iter().map(|(id, r)| view(*id, r)).collect()
    }

    /// The command log (encoder panics, scheduler refusals, expiries).
    pub fn log(&self) -> Vec<LogEntry> {
        self.lock().log.clone()
    }
}

struct Spec {
    work: Work,
    class: TrafficClass,
    slots: Vec<Option<TerminalRef>>,
    delay: Duration,
    repeat_every: Option<Duration>,
    run_for: Option<Duration>,
}

fn require(what: &'static str, s: Support) -> Result<(), CommandError> {
    match s {
        Support::Supported => Ok(()),
        Support::Unsupported { reason } => Err(CommandError::Unsupported { what, reason }),
    }
}

/// Shows or clears Device Offline for a non-final operation past New.
fn apply_link(state: &mut OpState, up: bool) {
    if up {
        if state.overlay() == Some(Overlay::DeviceOffline) {
            state.clear_overlay();
        }
    } else {
        let _ = state.set_overlay(Overlay::DeviceOffline);
    }
}

fn view(id: OperationId, r: &Record) -> OperationView {
    OperationView {
        id,
        family: r.family.clone(),
        status: r.state.status(),
        overlay: r.state.overlay(),
        terminals: r.slots.iter().flatten().count(),
        repetition: r.repetition,
        outstanding: r.outstanding,
        emitted: r.emitted,
        failure: r.failure.clone(),
    }
}
