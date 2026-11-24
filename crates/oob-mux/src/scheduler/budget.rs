//! The class budget: per-class floors and caps as a runtime input, and the
//! pure validation oob-dac's save path runs (r6 F6-3, ADR-003 §5, #16).
//!
//! Nothing here is a default. The numbers arrive from oob-dac's store
//! (#25) and are checked by [`validate`] on save (#26) and again by
//! [`SchedulerHandle::apply_budget`](super::SchedulerHandle::apply_budget).
//!
//! Rates are in kilobits per second of 188-byte transport packets on the
//! wire (the same unit as `docs/facts.yaml` `oob_stream.insertion_budget_kbps`).

use core::ops::{Index, IndexMut};

use crate::pdu::TrafficClass;
use crate::rate::PacketRate;

use super::class::{PREEMPTOR, enabled, requires_cap};

/// One value per traffic class C0–C5, indexed by [`TrafficClass`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassMap<T>([T; 6]);

impl<T> ClassMap<T> {
    /// A map whose value for each class is `f(class)`.
    pub fn from_fn(mut f: impl FnMut(TrafficClass) -> T) -> Self {
        Self(TrafficClass::ALL.map(&mut f))
    }

    /// Iterates `(class, value)` in class order C0 first.
    pub fn iter(&self) -> impl Iterator<Item = (TrafficClass, &T)> {
        TrafficClass::ALL.into_iter().zip(self.0.iter())
    }
}

impl<T> Index<TrafficClass> for ClassMap<T> {
    type Output = T;
    fn index(&self, class: TrafficClass) -> &T {
        &self.0[class.index()]
    }
}

impl<T> IndexMut<TrafficClass> for ClassMap<T> {
    fn index_mut(&mut self, class: TrafficClass) -> &mut T {
        &mut self.0[class.index()]
    }
}

/// One class's share of the insertion budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassBudget {
    /// Reserved rate in kbps; the class is served at least this fast when
    /// it has work (C2 excepted: it preempts up to its cap, see
    /// [`PREEMPTOR`]).
    pub floor_kbps: u32,
    /// Ceiling in kbps; `None` is "up to the insertion budget". C2, C4 and
    /// C5 must carry one.
    pub cap_kbps: Option<u32>,
    /// Bucket depth in packets (at least 1): how far ahead of its rate a
    /// class may run after idling.
    pub burst_packets: u16,
}

/// The class budget oob-dac's store supplies at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetConfig {
    /// The total non-null budget in kbps (~1000, `docs/facts.yaml`).
    pub insertion_kbps: u32,
    /// One entry per class.
    pub classes: ClassMap<ClassBudget>,
    /// Lab mode: C5 is refused and drained while this is `false`.
    pub lab_mode: bool,
}

/// The stream the budget is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamParams {
    /// The aggregate rate the flowgraph pulls at.
    pub packet_rate: PacketRate,
}

/// Why a budget is refused. Each variant has a stable [`reason`](Self::reason)
/// for the admin API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BudgetViolation {
    /// C0 has no floor, so system traffic could be starved.
    C0FloorZero,
    /// C0's floor is below the demand of the registered C0 carousels
    /// (time, connect/config). Checked on apply and on carousel
    /// registration, not by [`validate`], which has no carousel registry.
    C0FloorBelowDemand {
        /// The offered floor.
        floor_kbps: u32,
        /// The registered C0 carousel demand, at kbps resolution.
        demand_kbps: u32,
    },
    /// The floors of the enabled classes sum above the budget.
    FloorsExceedBudget {
        /// The sum of floors.
        sum_kbps: u32,
        /// The insertion budget.
        budget_kbps: u32,
    },
    /// C0 floor + C1 floor + C2 cap exceed the budget: an EAS flood
    /// could reach the guaranteed classes.
    PreemptorCanStarveC0 {
        /// C0's floor.
        c0_floor: u32,
        /// C1's floor.
        c1_floor: u32,
        /// C2's cap.
        c2_cap: u32,
        /// The insertion budget.
        budget: u32,
    },
    /// A cap below its class's floor.
    CapBelowFloor {
        /// The class.
        class: TrafficClass,
    },
    /// The insertion budget exceeds the stream rate.
    BudgetExceedsStream {
        /// The insertion budget.
        budget_kbps: u32,
        /// The stream rate, rounded down to kbps.
        stream_kbps: u32,
    },
    /// C2 has no cap.
    C2Uncapped,
    /// C4 has no cap.
    C4Uncapped,
    /// C5 has no cap.
    C5Uncapped,
    /// A class with `burst_packets` of 0 could never be served.
    ZeroBurst {
        /// The class.
        class: TrafficClass,
    },
}

impl BudgetViolation {
    /// A stable machine-readable name for the API.
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::C0FloorZero => "c0_floor_zero",
            Self::C0FloorBelowDemand { .. } => "c0_floor_below_demand",
            Self::FloorsExceedBudget { .. } => "floors_exceed_budget",
            Self::PreemptorCanStarveC0 { .. } => "preemptor_can_starve_c0",
            Self::CapBelowFloor { .. } => "cap_below_floor",
            Self::BudgetExceedsStream { .. } => "budget_exceeds_stream",
            Self::C2Uncapped => "c2_uncapped",
            Self::C4Uncapped => "c4_uncapped",
            Self::C5Uncapped => "c5_uncapped",
            Self::ZeroBurst { .. } => "zero_burst",
        }
    }
}

impl core::fmt::Display for BudgetViolation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: ", self.reason())?;
        match self {
            Self::C0FloorZero => f.write_str("C0 needs a non-zero floor"),
            Self::C0FloorBelowDemand {
                floor_kbps,
                demand_kbps,
            } => write!(
                f,
                "C0 floor {floor_kbps} kbps is below the registered C0 carousel demand {demand_kbps} kbps"
            ),
            Self::FloorsExceedBudget {
                sum_kbps,
                budget_kbps,
            } => write!(
                f,
                "floors sum to {sum_kbps} kbps, budget is {budget_kbps} kbps"
            ),
            Self::PreemptorCanStarveC0 {
                c0_floor,
                c1_floor,
                c2_cap,
                budget,
            } => write!(
                f,
                "C0 floor {c0_floor} + C1 floor {c1_floor} + C2 cap {c2_cap} exceeds budget {budget} kbps"
            ),
            Self::CapBelowFloor { class } => write!(f, "{class:?} cap is below its floor"),
            Self::BudgetExceedsStream {
                budget_kbps,
                stream_kbps,
            } => write!(
                f,
                "budget {budget_kbps} kbps exceeds the stream's {stream_kbps} kbps"
            ),
            Self::C2Uncapped => f.write_str("C2 must carry a cap"),
            Self::C4Uncapped => f.write_str("C4 must carry a cap"),
            Self::C5Uncapped => f.write_str("C5 must carry a cap"),
            Self::ZeroBurst { class } => write!(f, "{class:?} burst_packets must be at least 1"),
        }
    }
}

impl std::error::Error for BudgetViolation {}

/// A budget that passed [`validate`]. Its rates are what the scheduler's
/// token buckets run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedBudget(BudgetConfig);

impl ValidatedBudget {
    /// The validated settings.
    pub fn config(&self) -> &BudgetConfig {
        &self.0
    }
}

/// Checks `cfg` against `stream`. Pure and total: no I/O, no state.
///
/// Every violated rule is reported, in a fixed order. oob-dac's save path
/// (#26) calls this, then
/// [`SchedulerHandle::apply_budget`](super::SchedulerHandle::apply_budget),
/// which adds the check against the live C0 carousel demand.
///
/// ```
/// use oob_mux::scheduler::{BudgetConfig, BudgetViolation, ClassBudget, ClassMap, StreamParams, validate};
/// use oob_mux::{PacketRate, TrafficClass};
///
/// // Illustrative numbers for the doctest only.
/// let cfg = BudgetConfig {
///     insertion_kbps: 1000,
///     classes: ClassMap::from_fn(|c| ClassBudget {
///         floor_kbps: if c == TrafficClass::C0System { 100 } else { 0 },
///         cap_kbps: Some(200),
///         burst_packets: 4,
///     }),
///     lab_mode: false,
/// };
/// let stream = StreamParams {
///     packet_rate: PacketRate::from_bits_per_second(2_048_000).unwrap_or_else(|| unreachable!()),
/// };
/// assert!(validate(&cfg, stream).is_ok());
///
/// let mut starved = cfg.clone();
/// starved.classes[TrafficClass::C0System].floor_kbps = 0;
/// let errs = validate(&starved, stream).unwrap_err();
/// assert_eq!(errs[0], BudgetViolation::C0FloorZero);
/// assert_eq!(errs[0].reason(), "c0_floor_zero");
/// ```
///
/// # Errors
///
/// The list of [`BudgetViolation`]s, never empty.
pub fn validate(
    cfg: &BudgetConfig,
    stream: StreamParams,
) -> Result<ValidatedBudget, Vec<BudgetViolation>> {
    validate_inner(cfg, Some(stream))
}

/// [`validate`] with the stream optional: a scheduler is built before the
/// pacer that knows the rate, so the stream check runs when it is bound.
pub(crate) fn validate_inner(
    cfg: &BudgetConfig,
    stream: Option<StreamParams>,
) -> Result<ValidatedBudget, Vec<BudgetViolation>> {
    use TrafficClass::{C0System, C1Addressed, C2Eas, C4Cdl};
    let mut v = Vec::new();
    let b = |c: TrafficClass| cfg.classes[c];

    if b(C0System).floor_kbps == 0 {
        v.push(BudgetViolation::C0FloorZero);
    }
    let sum: u64 = cfg
        .classes
        .iter()
        .filter(|(c, _)| enabled(*c, cfg.lab_mode))
        .map(|(_, cb)| u64::from(cb.floor_kbps))
        .sum();
    if sum > u64::from(cfg.insertion_kbps) {
        v.push(BudgetViolation::FloorsExceedBudget {
            sum_kbps: u32::try_from(sum).unwrap_or(u32::MAX),
            budget_kbps: cfg.insertion_kbps,
        });
    }
    if let Some(s) = stream {
        let stream_kbps = u32::try_from(s.packet_rate.bits_per_second() / 1000).unwrap_or(u32::MAX);
        if cfg.insertion_kbps > stream_kbps {
            v.push(BudgetViolation::BudgetExceedsStream {
                budget_kbps: cfg.insertion_kbps,
                stream_kbps,
            });
        }
    }
    for (class, cb) in cfg.classes.iter() {
        if cb.cap_kbps.is_some_and(|cap| cap < cb.floor_kbps) {
            v.push(BudgetViolation::CapBelowFloor { class });
        }
        if cb.burst_packets == 0 {
            v.push(BudgetViolation::ZeroBurst { class });
        }
        if requires_cap(class) && cb.cap_kbps.is_none() {
            v.push(match class {
                C2Eas => BudgetViolation::C2Uncapped,
                C4Cdl => BudgetViolation::C4Uncapped,
                _ => BudgetViolation::C5Uncapped,
            });
        }
    }
    if let Some(c2_cap) = b(PREEMPTOR).cap_kbps {
        let used = u64::from(b(C0System).floor_kbps)
            + u64::from(b(C1Addressed).floor_kbps)
            + u64::from(c2_cap);
        if used > u64::from(cfg.insertion_kbps) {
            v.push(BudgetViolation::PreemptorCanStarveC0 {
                c0_floor: b(C0System).floor_kbps,
                c1_floor: b(C1Addressed).floor_kbps,
                c2_cap,
                budget: cfg.insertion_kbps,
            });
        }
    }
    if v.is_empty() {
        Ok(ValidatedBudget(cfg.clone()))
    } else {
        Err(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TrafficClass::{C0System, C1Addressed, C2Eas, C3Guide, C4Cdl, C5Lab};

    // Test fixture values only; real values are runtime configuration.
    fn fixture() -> BudgetConfig {
        let mut classes = ClassMap::from_fn(|_| ClassBudget {
            floor_kbps: 0,
            cap_kbps: None,
            burst_packets: 4,
        });
        classes[C0System].floor_kbps = 200;
        classes[C1Addressed].floor_kbps = 100;
        classes[C2Eas].cap_kbps = Some(300);
        classes[C3Guide].floor_kbps = 250;
        classes[C4Cdl].cap_kbps = Some(200);
        classes[C5Lab].cap_kbps = Some(100);
        BudgetConfig {
            insertion_kbps: 1000,
            classes,
            lab_mode: true,
        }
    }

    fn stream() -> StreamParams {
        StreamParams {
            packet_rate: PacketRate::from_bits_per_second(2_048_000).unwrap(),
        }
    }

    fn violations(cfg: &BudgetConfig) -> Vec<BudgetViolation> {
        validate(cfg, stream()).unwrap_err()
    }

    #[test]
    fn the_fixture_is_valid() {
        let v = validate(&fixture(), stream()).unwrap();
        assert_eq!(v.config(), &fixture());
    }

    #[test]
    fn c0_floor_zero() {
        let mut c = fixture();
        c.classes[C0System].floor_kbps = 0;
        assert_eq!(violations(&c), vec![BudgetViolation::C0FloorZero]);
    }

    #[test]
    fn floors_exceed_budget() {
        let mut c = fixture();
        c.classes[C3Guide].floor_kbps = 800;
        assert!(matches!(
            violations(&c)[..],
            [BudgetViolation::FloorsExceedBudget {
                sum_kbps: 1100,
                budget_kbps: 1000
            }]
        ));
    }

    #[test]
    fn a_disabled_lab_class_floor_is_not_counted() {
        let mut c = fixture();
        c.classes[C5Lab].floor_kbps = 500;
        c.classes[C5Lab].cap_kbps = Some(500);
        assert!(violations(&c)[0].reason() == "floors_exceed_budget");
        c.lab_mode = false;
        assert!(validate(&c, stream()).is_ok());
    }

    #[test]
    fn preemptor_can_starve_c0() {
        let mut c = fixture();
        c.classes[C2Eas].cap_kbps = Some(800);
        assert_eq!(
            violations(&c),
            vec![BudgetViolation::PreemptorCanStarveC0 {
                c0_floor: 200,
                c1_floor: 100,
                c2_cap: 800,
                budget: 1000
            }]
        );
    }

    #[test]
    fn cap_below_floor() {
        let mut c = fixture();
        c.classes[C3Guide].cap_kbps = Some(10);
        assert_eq!(
            violations(&c),
            vec![BudgetViolation::CapBelowFloor { class: C3Guide }]
        );
    }

    #[test]
    fn budget_exceeds_stream() {
        let mut c = fixture();
        c.insertion_kbps = 3000;
        assert!(
            violations(&c).contains(&BudgetViolation::BudgetExceedsStream {
                budget_kbps: 3000,
                stream_kbps: 2048
            })
        );
    }

    #[test]
    fn uncapped_c2_c4_c5_each_refused() {
        for (class, want) in [
            (C2Eas, BudgetViolation::C2Uncapped),
            (C4Cdl, BudgetViolation::C4Uncapped),
            (C5Lab, BudgetViolation::C5Uncapped),
        ] {
            let mut c = fixture();
            c.classes[class].cap_kbps = None;
            assert!(violations(&c).contains(&want), "{class:?}");
        }
    }

    #[test]
    fn zero_burst() {
        let mut c = fixture();
        c.classes[C4Cdl].burst_packets = 0;
        assert_eq!(
            violations(&c),
            vec![BudgetViolation::ZeroBurst { class: C4Cdl }]
        );
    }

    #[test]
    fn every_violation_has_a_distinct_reason() {
        let all = [
            BudgetViolation::C0FloorZero,
            BudgetViolation::C0FloorBelowDemand {
                floor_kbps: 1,
                demand_kbps: 2,
            },
            BudgetViolation::FloorsExceedBudget {
                sum_kbps: 1,
                budget_kbps: 0,
            },
            BudgetViolation::PreemptorCanStarveC0 {
                c0_floor: 0,
                c1_floor: 0,
                c2_cap: 0,
                budget: 0,
            },
            BudgetViolation::CapBelowFloor { class: C3Guide },
            BudgetViolation::BudgetExceedsStream {
                budget_kbps: 1,
                stream_kbps: 0,
            },
            BudgetViolation::C2Uncapped,
            BudgetViolation::C4Uncapped,
            BudgetViolation::C5Uncapped,
            BudgetViolation::ZeroBurst { class: C3Guide },
        ];
        let mut names: Vec<_> = all.iter().map(BudgetViolation::reason).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
        for v in all {
            assert!(v.to_string().starts_with(v.reason()));
        }
    }

    #[test]
    fn several_violations_are_all_reported() {
        let mut c = fixture();
        c.classes[C0System].floor_kbps = 0;
        c.classes[C2Eas].cap_kbps = None;
        assert_eq!(
            violations(&c),
            vec![BudgetViolation::C0FloorZero, BudgetViolation::C2Uncapped]
        );
    }
}
