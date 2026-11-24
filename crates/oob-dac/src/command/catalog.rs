//! The command catalog: every operation the DAC manual names, with its
//! support status.
//!
//! Three sets, all code (ADR-003 Layer 3; operators cannot add to them at
//! runtime):
//!
//! - [`TerminalOp`]: the DAC §11 per-terminal operations and the Define
//!   Terminal actions (Plan §2, §3.2), including the enrollment messages in
//!   their manual-confirmed order (factory-init → unit-creation → connect,
//!   r4 F4-4);
//! - [`OperationKind`]: the Define Operations *user-defined* types (Plan §2);
//! - [`AutoOp`]: the Define Operations *auto-generated* types, which the DAC
//!   schedules itself.
//!
//! Every entry the manual names is **present**. An entry Oob does not
//! reproduce is **rejected with a named reason** ([`Support::Unsupported`]),
//! so a refusal is always explainable to the operator (and to a REST or
//! WireLink client, through [`CommandError::reason`]). *Supported* means the
//! command layer accepts and routes the entry to a family encoder (`oob-msg`,
//! #19); whether that family's wire format is `documented`, `hypothesized` or
//! `verified` is `oob-msg`'s status, not this catalog's.
//!
//! The *unsupported* set and its reasons are **design decisions** made for
//! the current slice, not manual statements; each reason says why.
//!
//! [`CommandError::reason`]: super::CommandError::reason

/// Whether the command layer accepts an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Accepted and routed to its family encoder.
    Supported,
    /// Present in the manual, rejected here, with the reason why.
    Unsupported {
        /// A short, stable, human-readable reason.
        reason: &'static str,
    },
}

impl Support {
    /// Whether the entry is accepted.
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// A catalog entry: a manual name and a support status.
pub trait CatalogEntry {
    /// The entry's stable name (used in errors, logs and audit rows).
    fn name(&self) -> &'static str;
    /// Whether it is accepted.
    fn support(&self) -> Support;
}

/// DAC §11 per-terminal operations and Define Terminal actions.
///
/// The three enrollment messages come first, in the manual-confirmed order
/// (r4 F4-4). There is no OTA auto-enrollment; the unit DB is enrollment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminalOp {
    /// Factory initialization (enrollment step 1).
    FactoryInit,
    /// Unit creation (enrollment step 2).
    UnitCreation,
    /// Connect (enrollment step 3).
    Connect,
    /// Virtual Channel Map message.
    VirtualChannel,
    /// Control Channel Timeout message.
    ControlChannelTimeout,
    /// Initialize/Refresh Authorizations message.
    InitRefreshAuthorizations,
    /// Inband Processor message.
    InbandProcessor,
    /// Region message.
    Region,
    /// Interactive message.
    Interactive,
    /// General message.
    General,
    /// Multicast-16 message.
    Multicast16,
    /// Phone message.
    Phone,
    /// Authorize (Define Terminal action).
    Authorize,
    /// Configure Transmission (Define Terminal action).
    ConfigureTransmission,
    /// Initialize (Define Terminal action).
    Initialize,
    /// Cold Initialize (Define Terminal action; destructive, F6-10).
    ColdInitialize,
    /// Refresh (Define Terminal action).
    Refresh,
    /// Disconnect (Define Terminal action).
    Disconnect,
    /// Network (DVB) message.
    NetworkDvb,
}

impl TerminalOp {
    /// Every entry, enrollment order first.
    pub const ALL: [Self; 19] = [
        Self::FactoryInit,
        Self::UnitCreation,
        Self::Connect,
        Self::VirtualChannel,
        Self::ControlChannelTimeout,
        Self::InitRefreshAuthorizations,
        Self::InbandProcessor,
        Self::Region,
        Self::Interactive,
        Self::General,
        Self::Multicast16,
        Self::Phone,
        Self::Authorize,
        Self::ConfigureTransmission,
        Self::Initialize,
        Self::ColdInitialize,
        Self::Refresh,
        Self::Disconnect,
        Self::NetworkDvb,
    ];

    /// Whether the operation is destructive: policy (#23) requires the owner
    /// role and a confirmation token for it (r6 F6-10, AGENTS.md §7).
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::ColdInitialize | Self::FactoryInit)
    }
}

impl CatalogEntry for TerminalOp {
    fn name(&self) -> &'static str {
        match self {
            Self::FactoryInit => "factory-init",
            Self::UnitCreation => "unit-creation",
            Self::Connect => "connect",
            Self::VirtualChannel => "virtual-channel",
            Self::ControlChannelTimeout => "control-channel-timeout",
            Self::InitRefreshAuthorizations => "init-refresh-authorizations",
            Self::InbandProcessor => "inband-processor",
            Self::Region => "region",
            Self::Interactive => "interactive",
            Self::General => "general",
            Self::Multicast16 => "multicast-16",
            Self::Phone => "phone",
            Self::Authorize => "authorize",
            Self::ConfigureTransmission => "configure-transmission",
            Self::Initialize => "initialize",
            Self::ColdInitialize => "cold-initialize",
            Self::Refresh => "refresh",
            Self::Disconnect => "disconnect",
            Self::NetworkDvb => "network-dvb",
        }
    }

    fn support(&self) -> Support {
        match self {
            Self::NetworkDvb => Support::Unsupported {
                reason: "DVB only; Oob reproduces the GI/Motorola headend",
            },
            _ => Support::Supported,
        }
    }
}

/// Define Operations, user-defined types (Plan §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationKind {
    /// Refresh.
    Refresh,
    /// Purchase Poll.
    PurchasePoll,
    /// Diagnostic Poll.
    DiagnosticPoll,
    /// Chronic Non-Responding Poll.
    ChronicNonRespondingPoll,
    /// Category Epoch Transition.
    CategoryEpochTransition,
    /// Flush and Fill.
    FlushAndFill,
    /// Fill Only.
    FillOnly,
    /// Resource OCAM.
    ResourceOcam,
    /// Synchronize CPMS.
    SynchronizeCpms,
    /// Any per-terminal operation, run as a bulk operation.
    Terminal(TerminalOp),
}

impl OperationKind {
    /// Every kind that is not a wrapped [`TerminalOp`].
    pub const OWN: [Self; 9] = [
        Self::Refresh,
        Self::PurchasePoll,
        Self::DiagnosticPoll,
        Self::ChronicNonRespondingPoll,
        Self::CategoryEpochTransition,
        Self::FlushAndFill,
        Self::FillOnly,
        Self::ResourceOcam,
        Self::SynchronizeCpms,
    ];

    /// Every kind: the own kinds, then each wrapped [`TerminalOp`].
    pub fn all() -> Vec<Self> {
        Self::OWN
            .into_iter()
            .chain(TerminalOp::ALL.into_iter().map(Self::Terminal))
            .collect()
    }

    /// Whether the kind is destructive (r6 F6-10): true only for a wrapped
    /// destructive [`TerminalOp`].
    pub fn is_destructive(self) -> bool {
        match self {
            Self::Terminal(op) => op.is_destructive(),
            _ => false,
        }
    }
}

impl CatalogEntry for OperationKind {
    fn name(&self) -> &'static str {
        match self {
            Self::Refresh => "op-refresh",
            Self::PurchasePoll => "purchase-poll",
            Self::DiagnosticPoll => "diagnostic-poll",
            Self::ChronicNonRespondingPoll => "chronic-non-responding-poll",
            Self::CategoryEpochTransition => "category-epoch-transition",
            Self::FlushAndFill => "flush-and-fill",
            Self::FillOnly => "fill-only",
            Self::ResourceOcam => "resource-ocam",
            Self::SynchronizeCpms => "synchronize-cpms",
            Self::Terminal(op) => op.name(),
        }
    }

    fn support(&self) -> Support {
        match self {
            Self::CategoryEpochTransition => Support::Unsupported {
                reason: "Category Epoch Transition is not implemented yet; the Delayed \
                         status exists but is reachable only once it is",
            },
            Self::ResourceOcam => Support::Unsupported {
                reason: "Resource OCAM needs the OCAM component, which Oob does not reproduce",
            },
            Self::SynchronizeCpms => Support::Unsupported {
                reason: "CPMS is a separate billing-side system; Oob has none",
            },
            Self::Terminal(op) => op.support(),
            _ => Support::Supported,
        }
    }
}

/// Define Operations, auto-generated types: operations the DAC schedules
/// itself (their cadences are runtime settings, #25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutoOp {
    /// Global Authorization Refresh.
    GlobalAuthRefresh,
    /// Channel Map.
    ChannelMap,
    /// Multiplex.
    Multiplex,
    /// Insert System Time.
    InsertSystemTime,
    /// Code Download.
    CodeDownload,
    /// Analog Rekey.
    AnalogRekey,
    /// Analog Clear Service.
    AnalogClearService,
    /// PPV Window Authorization.
    PpvWindowAuth,
    /// CEC EMM Generation.
    CecEmmGeneration,
    /// Macrovision.
    Macrovision,
    /// Physical Channel Map.
    PhysicalChannelMap,
    /// Periodic ERS EMM Generation.
    PeriodicErsEmmGeneration,
    /// Compute DST Interval.
    ComputeDstInterval,
    /// CableCARD SNT.
    CableCardSnt,
    /// CP Phone Number.
    CpPhoneNumber,
}

impl AutoOp {
    /// Every entry.
    pub const ALL: [Self; 15] = [
        Self::GlobalAuthRefresh,
        Self::ChannelMap,
        Self::Multiplex,
        Self::InsertSystemTime,
        Self::CodeDownload,
        Self::AnalogRekey,
        Self::AnalogClearService,
        Self::PpvWindowAuth,
        Self::CecEmmGeneration,
        Self::Macrovision,
        Self::PhysicalChannelMap,
        Self::PeriodicErsEmmGeneration,
        Self::ComputeDstInterval,
        Self::CableCardSnt,
        Self::CpPhoneNumber,
    ];
}

impl CatalogEntry for AutoOp {
    fn name(&self) -> &'static str {
        match self {
            Self::GlobalAuthRefresh => "global-auth-refresh",
            Self::ChannelMap => "channel-map",
            Self::Multiplex => "multiplex",
            Self::InsertSystemTime => "insert-system-time",
            Self::CodeDownload => "code-download",
            Self::AnalogRekey => "analog-rekey",
            Self::AnalogClearService => "analog-clear-service",
            Self::PpvWindowAuth => "ppv-window-auth",
            Self::CecEmmGeneration => "cec-emm-generation",
            Self::Macrovision => "macrovision",
            Self::PhysicalChannelMap => "physical-channel-map",
            Self::PeriodicErsEmmGeneration => "periodic-ers-emm-generation",
            Self::ComputeDstInterval => "compute-dst-interval",
            Self::CableCardSnt => "cablecard-snt",
            Self::CpPhoneNumber => "cp-phone-number",
        }
    }

    fn support(&self) -> Support {
        match self {
            Self::AnalogRekey | Self::AnalogClearService | Self::Macrovision => {
                Support::Unsupported {
                    reason: "analog tier (S3) is registered but unscoped; AGENTS.md §9",
                }
            }
            Self::CableCardSnt => Support::Unsupported {
                reason: "CableCARD is outside the DCT2000-class fleet Oob serves",
            },
            _ => Support::Supported,
        }
    }
}

/// The traffic class an auto-generated operation's PDU travels in
/// (r6 F6-3): time and channel maps are system traffic (C0), code download
/// is C4, the rest is addressed (C1). A design mapping, not a manual fact.
pub fn auto_class(op: AutoOp) -> oob_mux::TrafficClass {
    use oob_mux::TrafficClass as C;
    match op {
        AutoOp::InsertSystemTime
        | AutoOp::ChannelMap
        | AutoOp::Multiplex
        | AutoOp::PhysicalChannelMap
        | AutoOp::ComputeDstInterval
        | AutoOp::CpPhoneNumber => C::C0System,
        AutoOp::CodeDownload => C::C4Cdl,
        _ => C::C1Addressed,
    }
}
