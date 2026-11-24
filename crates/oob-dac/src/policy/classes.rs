//! The destructive-class table (r6 F6-10, #23 design note section 2).
//!
//! One data structure, [`CLASSES`], read by the policy code, by the tests and
//! by the docs drift test (`docs/architecture/oob-dac-policy.md` must carry a
//! row for every entry), so the book cannot disagree with the code. It is
//! code (ADR-003 Layer 3): operators cannot add to it at runtime.

use crate::command::catalog::{AutoOp, CatalogEntry, OperationKind, TerminalOp};
use crate::command::target::TerminalRef;
use crate::command::{Command, DefineOperation};
use crate::store::TerminalId;

use super::token::{TokenTarget, plan_digest};

/// How a class is confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Hard to undo: owner role plus a confirmation token.
    Hard,
    /// Reversible (a Disconnect is undone by reauthorizing), but kept in the
    /// same owner-plus-token class by Marshal's ruling (2026-11-23): billing
    /// and WireLink front doors cannot Disconnect.
    Reversible,
}

impl Tier {
    /// The label shown in the docs table.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hard => "D-hard",
            Self::Reversible => "D-reversible",
        }
    }
}

/// What a confirmation token for an operation may be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// One terminal, by store-allocated terminal ID.
    Terminal,
    /// A resolved bulk target set (its digest).
    Plan,
    /// This installation.
    Install,
}

impl TargetKind {
    /// The label shown in the docs table.
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Plan => "plan",
            Self::Install => "install",
        }
    }
}

/// A destructive operation class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DestructiveOp {
    /// Cold Initialize (Define Terminal action).
    ColdInitialize,
    /// Factory initialization (enrollment step 1).
    FactoryInit,
    /// Initialize (Define Terminal action).
    Initialize,
    /// Unit creation (rewrites box identity; architect addition).
    UnitCreation,
    /// Disconnect.
    Disconnect,
    /// A General message that sets `code_download_enable = true`. Enumerated
    /// and token-capable now; the bus cannot see message parameters until
    /// the encoders land (#19), so only [`StorePolicy::authorize`] reaches
    /// it today.
    ///
    /// [`StorePolicy::authorize`]: super::StorePolicy::authorize
    CodeDownloadEnable,
    /// A CDL campaign: the Code Download auto operation.
    CdlCampaign,
    /// Turning the sacrificial-only CDL lock OFF (QA-7).
    CdlLockOff,
    /// Marking a unit sacrificial (makes it CDL-eligible; architect
    /// addition).
    MarkSacrificial,
    /// A whole-install store restore (#24).
    StoreRestore,
}

/// One row of the class table.
#[derive(Debug, Clone, Copy)]
pub struct ClassRow {
    /// The class.
    pub op: DestructiveOp,
    /// Its stable name. For the terminal operations this is the catalog
    /// name, which is also the token's operation binding.
    pub name: &'static str,
    /// How it is confirmed.
    pub tier: Tier,
    /// The typed phrase (the target label follows it).
    pub phrase: &'static str,
    /// What a token may be bound to.
    pub targets: &'static [TargetKind],
    /// Where it enters today.
    pub entry: &'static str,
}

/// Every destructive class.
pub const CLASSES: &[ClassRow] = &[
    ClassRow {
        op: DestructiveOp::ColdInitialize,
        name: "cold-initialize",
        tier: Tier::Hard,
        phrase: "COLD INIT",
        targets: &[TargetKind::Terminal, TargetKind::Plan],
        entry: "Command::Terminal, bulk Command::Operation",
    },
    ClassRow {
        op: DestructiveOp::FactoryInit,
        name: "factory-init",
        tier: Tier::Hard,
        phrase: "FACTORY INIT",
        targets: &[TargetKind::Terminal, TargetKind::Plan],
        entry: "Command::Terminal, bulk Command::Operation",
    },
    ClassRow {
        op: DestructiveOp::Initialize,
        name: "initialize",
        tier: Tier::Hard,
        phrase: "INITIALIZE",
        targets: &[TargetKind::Terminal, TargetKind::Plan],
        entry: "Command::Terminal, bulk Command::Operation",
    },
    ClassRow {
        op: DestructiveOp::UnitCreation,
        name: "unit-creation",
        tier: Tier::Hard,
        phrase: "UNIT CREATE",
        targets: &[TargetKind::Terminal, TargetKind::Plan],
        entry: "Command::Terminal, bulk Command::Operation",
    },
    ClassRow {
        op: DestructiveOp::Disconnect,
        name: "disconnect",
        tier: Tier::Reversible,
        phrase: "DISCONNECT",
        targets: &[TargetKind::Terminal, TargetKind::Plan],
        entry: "Command::Terminal, bulk Command::Operation",
    },
    ClassRow {
        op: DestructiveOp::CodeDownloadEnable,
        name: "code-download-enable",
        tier: Tier::Hard,
        phrase: "CDL ENABLE",
        targets: &[TargetKind::Terminal],
        entry: "StorePolicy::authorize (parameter-level; needs #19)",
    },
    ClassRow {
        op: DestructiveOp::CdlCampaign,
        name: "cdl-campaign",
        tier: Tier::Hard,
        phrase: "CDL CAMPAIGN",
        targets: &[TargetKind::Plan],
        entry: "Command::Auto(CodeDownload)",
    },
    ClassRow {
        op: DestructiveOp::CdlLockOff,
        name: "cdl-lock-off",
        tier: Tier::Hard,
        phrase: "CDL LOCK OFF",
        targets: &[TargetKind::Install],
        entry: "StorePolicy::set_cdl_lock",
    },
    ClassRow {
        op: DestructiveOp::MarkSacrificial,
        name: "mark-sacrificial",
        tier: Tier::Hard,
        phrase: "MARK SACRIFICIAL",
        targets: &[TargetKind::Terminal],
        entry: "StorePolicy::mark_sacrificial",
    },
    ClassRow {
        op: DestructiveOp::StoreRestore,
        name: "store-restore",
        tier: Tier::Hard,
        phrase: "STORE RESTORE",
        targets: &[TargetKind::Install],
        entry: "StorePolicy::authorize (the restore itself is #24)",
    },
];

impl DestructiveOp {
    /// Every class, in table order.
    pub fn all() -> impl Iterator<Item = Self> {
        CLASSES.iter().map(|r| r.op)
    }

    /// The class's table row.
    pub fn row(self) -> &'static ClassRow {
        CLASSES
            .iter()
            .find(|r| r.op == self)
            .expect("every DestructiveOp has a CLASSES row (tested)")
    }

    /// The stable name; also the operation a token is bound to.
    pub fn name(self) -> &'static str {
        self.row().name
    }

    /// How it is confirmed.
    pub fn tier(self) -> Tier {
        self.row().tier
    }

    /// Whether the sacrificial-only CDL lock applies to it.
    pub fn is_cdl(self) -> bool {
        matches!(self, Self::CdlCampaign | Self::CodeDownloadEnable)
    }
}

/// The destructive class of a per-terminal operation, if any.
pub fn classify_terminal_op(op: TerminalOp) -> Option<DestructiveOp> {
    match op {
        TerminalOp::ColdInitialize => Some(DestructiveOp::ColdInitialize),
        TerminalOp::FactoryInit => Some(DestructiveOp::FactoryInit),
        TerminalOp::Initialize => Some(DestructiveOp::Initialize),
        TerminalOp::UnitCreation => Some(DestructiveOp::UnitCreation),
        TerminalOp::Disconnect => Some(DestructiveOp::Disconnect),
        _ => None,
    }
}

/// What a command needs before it may run: the class and what its token
/// must be bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Need {
    pub(super) op: DestructiveOp,
    pub(super) target: TokenTarget,
}

/// The destructive class a command falls in, with its binding. `None` for a
/// non-destructive command and for a dry run (which sends nothing).
pub(super) fn need(cmd: &Command, resolved: &[TerminalRef]) -> Option<Need> {
    match cmd {
        Command::Terminal { op, terminal } => classify_terminal_op(*op).map(|op| Need {
            op,
            target: TokenTarget::Terminal(*terminal),
        }),
        Command::Operation(DefineOperation { dry_run: true, .. }) => None,
        Command::Operation(def) => match def.kind {
            OperationKind::Terminal(t) => classify_terminal_op(t).map(|op| Need {
                op,
                target: TokenTarget::Plan(plan_digest(t.name(), resolved)),
            }),
            _ => None,
        },
        Command::Auto(AutoOp::CodeDownload) => Some(Need {
            op: DestructiveOp::CdlCampaign,
            target: TokenTarget::Plan(plan_digest(AutoOp::CodeDownload.name(), resolved)),
        }),
        _ => None,
    }
}

/// The terminals in `resolved` the lock refuses: every non-sacrificial one.
pub(super) fn non_sacrificial(resolved: &[TerminalRef]) -> Vec<TerminalId> {
    resolved
        .iter()
        .filter(|t| !t.sacrificial)
        .map(|t| t.id)
        .collect()
}
