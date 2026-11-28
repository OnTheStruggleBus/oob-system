//! Targeting: All / Terminal-ID range / Single, narrowed by the Plan §2
//! filters, resolved against a [`TerminalDirectory`].
//!
//! The directory is a trait so the command layer is tested against
//! [`InMemoryDirectory`] and runs against [`StoreDirectory`], which reads
//! the #21 store through a read-only connection. Resolution is the same
//! pure function for both ([`resolve_from`]): sorted by terminal ID,
//! deduplicated, and an empty result is an error ([`CommandError::EmptyTarget`]),
//! never a silent no-op.

use std::ops::RangeInclusive;
use std::sync::Mutex;

use crate::store::{Reader, Terminal, TerminalId, TerminalType};

use super::CommandError;

/// Which terminals a Define Operations entry addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Every terminal the filters admit.
    All,
    /// Terminal IDs in the inclusive range.
    IdRange(RangeInclusive<TerminalId>),
    /// One terminal. An unknown ID is [`CommandError::UnknownTerminal`].
    Single(TerminalId),
}

/// The Plan §2 filters. Every `Some` must match (logical AND).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// RADD assignment.
    pub radd: Option<String>,
    /// Downstream plant.
    pub downstream_plant: Option<String>,
    /// Upstream plant.
    pub upstream_plant: Option<String>,
    /// Terminal type.
    pub terminal_type: Option<TerminalType>,
    /// RPD channel.
    pub rpd_channel: Option<String>,
    /// Business System (the terminal's BSI name).
    pub business_system: Option<String>,
}

/// What the command layer needs to know about one terminal. Deliberately
/// *not* the full record: no unit address, no serial (ADR-003 §4), so
/// nothing box-identifying passes through targeting or the logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRef {
    /// The terminal ID.
    pub id: TerminalId,
    /// Terminal type.
    pub terminal_type: TerminalType,
    /// Downstream plant.
    pub downstream_plant: Option<String>,
    /// Upstream plant.
    pub upstream_plant: Option<String>,
    /// Business System (BSI name).
    pub business_system: Option<String>,
    /// RADD assignment, where the directory knows it.
    pub radd: Option<String>,
    /// RPD channel, where the directory knows it.
    pub rpd_channel: Option<String>,
    /// The sacrificial unit (AGENTS.md §7); policy (#23) reads it.
    pub sacrificial: bool,
}

impl TerminalRef {
    /// A reference with only the required fields set.
    pub fn new(id: TerminalId, terminal_type: TerminalType) -> Self {
        Self {
            id,
            terminal_type,
            downstream_plant: None,
            upstream_plant: None,
            business_system: None,
            radd: None,
            rpd_channel: None,
            sacrificial: false,
        }
    }

    fn matches(&self, f: &Filters) -> bool {
        fn eq(want: &Option<String>, have: &Option<String>) -> bool {
            want.is_none() || want == have
        }
        f.terminal_type.is_none_or(|t| t == self.terminal_type)
            && eq(&f.radd, &self.radd)
            && eq(&f.downstream_plant, &self.downstream_plant)
            && eq(&f.upstream_plant, &self.upstream_plant)
            && eq(&f.rpd_channel, &self.rpd_channel)
            && eq(&f.business_system, &self.business_system)
    }
}

/// Resolves a target and filters to terminals. Implemented over the store
/// and by [`InMemoryDirectory`].
pub trait TerminalDirectory: Send + Sync {
    /// The terminals `target` and `filters` select, by ascending ID.
    ///
    /// # Errors
    ///
    /// [`CommandError::UnknownTerminal`] for a `Single` ID the directory
    /// does not hold, [`CommandError::EmptyTarget`] for an empty selection,
    /// [`CommandError::Unsupported`] for a filter the directory cannot
    /// evaluate, [`CommandError::Store`] for a read failure.
    fn resolve(&self, target: &Target, filters: &Filters)
    -> Result<Vec<TerminalRef>, CommandError>;
}

/// The shared resolution rule over a full listing.
pub fn resolve_from(
    mut all: Vec<TerminalRef>,
    target: &Target,
    filters: &Filters,
) -> Result<Vec<TerminalRef>, CommandError> {
    all.sort_by_key(|t| t.id);
    all.dedup_by_key(|t| t.id);
    if let Target::Single(id) = target
        && !all.iter().any(|t| t.id == *id)
    {
        return Err(CommandError::UnknownTerminal(*id));
    }
    let selected: Vec<TerminalRef> = all
        .into_iter()
        .filter(|t| match target {
            Target::All => true,
            Target::IdRange(r) => r.contains(&t.id),
            Target::Single(id) => t.id == *id,
        })
        .filter(|t| t.matches(filters))
        .collect();
    if selected.is_empty() {
        return Err(CommandError::EmptyTarget);
    }
    Ok(selected)
}

/// A directory over a fixed list; the trait-backed fake of the #22 tests,
/// also usable by adapters' tests.
#[derive(Debug, Clone, Default)]
pub struct InMemoryDirectory {
    terminals: Vec<TerminalRef>,
}

impl InMemoryDirectory {
    /// A directory holding `terminals`.
    pub fn new(terminals: Vec<TerminalRef>) -> Self {
        Self { terminals }
    }
}

impl TerminalDirectory for InMemoryDirectory {
    fn resolve(
        &self,
        target: &Target,
        filters: &Filters,
    ) -> Result<Vec<TerminalRef>, CommandError> {
        resolve_from(self.terminals.clone(), target, filters)
    }
}

/// A directory over the #21 store, through a read-only connection.
///
/// Retired units (#25) are excluded. The store's Define Terminal record has
/// no RADD assignment and no RPD
/// channel yet, so a RADD or RPD filter is **refused by name** rather than
/// silently matching nothing: [`CommandError::Unsupported`] with a reason.
pub struct StoreDirectory {
    reader: Mutex<Reader>,
}

impl StoreDirectory {
    /// A directory over `reader` (from `Store::reader`).
    pub fn new(reader: Reader) -> Self {
        Self {
            reader: Mutex::new(reader),
        }
    }
}

fn to_ref(t: Terminal) -> TerminalRef {
    let r = t.record;
    TerminalRef {
        id: t.id,
        terminal_type: r.terminal_type,
        downstream_plant: r.downstream_plant,
        upstream_plant: r.upstream_plant,
        business_system: r.bsi_name,
        radd: None,
        rpd_channel: None,
        sacrificial: r.sacrificial,
    }
}

impl TerminalDirectory for StoreDirectory {
    fn resolve(
        &self,
        target: &Target,
        filters: &Filters,
    ) -> Result<Vec<TerminalRef>, CommandError> {
        if filters.radd.is_some() {
            return Err(CommandError::Unsupported {
                what: "filter:radd",
                reason: "the store's Define Terminal record carries no RADD assignment yet",
            });
        }
        if filters.rpd_channel.is_some() {
            return Err(CommandError::Unsupported {
                what: "filter:rpd-channel",
                reason: "the store's Define Terminal record carries no RPD channel yet",
            });
        }
        let all = self
            .reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .terminals()
            .map_err(CommandError::Store)?;
        // A retired unit (#25) is never a target: not in All or a range,
        // and a Single ID of one is an unknown terminal.
        let live = all.into_iter().filter(|t| t.retired_utc.is_none());
        resolve_from(live.map(to_ref).collect(), target, filters)
    }
}
