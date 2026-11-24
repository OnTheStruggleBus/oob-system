//! Token buckets in packet slots (#16 design note §3).
//!
//! All accounting is in slots, so the budget follows the flowgraph's clock
//! (ADR-002, F6-2). A class at `r` bits/s on a stream of `R` bits/s gets
//! `r / R` packets per slot. To stay in integers a bucket holds
//! `bits/s × slots`: each slot adds `r`, and one packet costs `R`.
//!
//! A bucket may go into debt by the tail of a multi-packet PDU: a PDU is
//! eligible while the bucket holds one packet's worth, and is charged its
//! whole length, so the long-run rate stays exact. The bound on any
//! window is therefore `rate × window ± (burst + the longest PDU)`. A
//! bucket is `burst` packets plus one slot of refill deep, and a floor
//! bucket one packet deeper still.

use crate::pdu::TrafficClass;

use super::budget::{BudgetConfig, ClassBudget};
use super::class::enabled;

/// One bucket. `rate` is added per slot; `depth` caps the fill.
#[derive(Debug, Clone, Copy)]
pub(super) struct Bucket {
    tokens: i64,
    depth: i64,
    rate: i64,
}

impl Bucket {
    /// A full bucket of `rate_bps` and `burst` packets on a stream of
    /// `stream_bps`. A zero rate has no depth: it never serves.
    fn new(rate_bps: u64, burst: u16, stream_bps: u64) -> Self {
        let rate = i64::try_from(rate_bps).unwrap_or(i64::MAX);
        let depth = if rate == 0 {
            0
        } else {
            // The burst, plus one slot of refill: a bucket exactly one
            // packet deep would clamp the fraction accrued past the first
            // token and run slow at any rate above half a packet a slot.
            i64::from(burst)
                .saturating_mul(i64::try_from(stream_bps).unwrap_or(i64::MAX))
                .saturating_add(rate)
        };
        Self {
            tokens: depth,
            depth,
            rate,
        }
    }

    fn refill(&mut self, slots: u64) {
        let add = i128::from(self.rate) * i128::from(slots);
        let t = (i128::from(self.tokens) + add).min(i128::from(self.depth));
        self.tokens = i64::try_from(t).unwrap_or(self.depth);
    }

    pub(super) fn has_packet(&self, packet: i64) -> bool {
        self.tokens >= packet
    }

    fn charge(&mut self, packets: u64, packet: i64) {
        let c = i128::from(packets) * i128::from(packet);
        self.tokens = i64::try_from(i128::from(self.tokens) - c).unwrap_or(i64::MIN);
    }

    fn charge_saturating(&mut self, packets: u64, packet: i64) {
        let before = self.tokens;
        self.charge(packets, packet);
        if before >= 0 && self.tokens < 0 {
            self.tokens = 0;
        }
    }

    /// Slots until one packet's worth is held; `u64::MAX` if never.
    fn wait(&self, packet: i64) -> u64 {
        if self.tokens >= packet {
            return 0;
        }
        if self.rate == 0 || self.depth < packet {
            return u64::MAX;
        }
        let need = i128::from(packet) - i128::from(self.tokens);
        let r = i128::from(self.rate);
        u64::try_from((need + r - 1) / r).unwrap_or(u64::MAX)
    }

    /// Re-expresses the fill for a new packet cost (a stream rate change),
    /// clamped to the new depth.
    fn rescale(&mut self, old: i64, new: i64, burst: u16, rate_bps: u64) {
        let t = i128::from(self.tokens) * i128::from(new) / i128::from(old.max(1));
        let fresh = Self::new(rate_bps, burst, u64::try_from(new).unwrap_or(0));
        self.depth = fresh.depth;
        self.rate = fresh.rate;
        self.tokens = i64::try_from(t).unwrap_or(self.depth).min(self.depth);
    }
}

/// A floor bucket is one packet deeper than its burst: while a floor class
/// waits for a budget or cap token, its floor keeps accruing, and a bucket
/// that clamped that accrual would run the floor slow (found by the
/// validator-soundness test at `burst_packets` 1).
fn floor_burst(burst: u16) -> u16 {
    burst.saturating_add(1)
}

/// How a class was served; decides which buckets are charged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    /// Within its floor.
    Floor,
    /// C2 ahead of the C3–C5 floors, within its cap.
    Preempt,
    /// Spare budget, within the cap.
    Excess,
}

/// The floor and cap buckets of every class plus the budget bucket.
#[derive(Debug, Clone)]
pub(super) struct Buckets {
    cfg: BudgetConfig,
    floor: [Bucket; 6],
    /// `None` is uncapped.
    cap: [Option<Bucket>; 6],
    budget: Bucket,
    /// Packet cost in bucket units: the stream rate in bits/s.
    packet: i64,
    last_slot: u64,
}

impl Buckets {
    /// Full buckets for `cfg` on a stream of `stream_bps`.
    pub(super) fn new(cfg: BudgetConfig, stream_bps: u64, slot: u64) -> Self {
        let mut s = Self {
            floor: [Bucket::new(0, 1, 1); 6],
            cap: [None; 6],
            budget: Bucket::new(0, 1, 1),
            packet: i64::try_from(stream_bps).unwrap_or(i64::MAX),
            last_slot: slot,
            cfg,
        };
        s.build(stream_bps, None);
        s
    }

    fn rate_of(kbps: u32) -> u64 {
        u64::from(kbps) * 1000
    }

    /// (Re)builds every bucket from the config. With `keep`, each fill is
    /// carried over, clamped to the new depth: a swap grants no burst.
    fn build(&mut self, stream_bps: u64, keep: Option<&Self>) {
        for class in TrafficClass::ALL {
            let i = class.index();
            let cb: ClassBudget = self.cfg.classes[class];
            let mut f = Bucket::new(
                Self::rate_of(cb.floor_kbps),
                floor_burst(cb.burst_packets),
                stream_bps,
            );
            let mut c = cb
                .cap_kbps
                .map(|cap| Bucket::new(Self::rate_of(cap), cb.burst_packets, stream_bps));
            if let Some(old) = keep {
                f.tokens = f.tokens.min(old.floor[i].tokens);
                if let (Some(c), Some(Some(oc))) = (c.as_mut(), old.cap.get(i)) {
                    c.tokens = c.tokens.min(oc.tokens);
                }
            }
            self.floor[i] = f;
            self.cap[i] = c;
        }
        let burst = self
            .cfg
            .classes
            .iter()
            .map(|(_, c)| c.burst_packets)
            .max()
            .unwrap_or(1);
        let mut b = Bucket::new(Self::rate_of(self.cfg.insertion_kbps), burst, stream_bps);
        if let Some(old) = keep {
            b.tokens = b.tokens.min(old.budget.tokens);
        }
        self.budget = b;
    }

    pub(super) fn config(&self) -> &BudgetConfig {
        &self.cfg
    }

    pub(super) fn enabled(&self, class: TrafficClass) -> bool {
        enabled(class, self.cfg.lab_mode)
    }

    /// Swaps in a validated budget at the current slot.
    pub(super) fn apply(&mut self, cfg: BudgetConfig) {
        let stream = u64::try_from(self.packet).unwrap_or(0);
        let old = self.clone();
        self.cfg = cfg;
        self.build(stream, Some(&old));
    }

    /// Brings every bucket up to `slot`, and re-expresses them if the
    /// stream rate changed.
    pub(super) fn advance(&mut self, slot: u64, stream_bps: u64) {
        let new = i64::try_from(stream_bps).unwrap_or(i64::MAX);
        if new != self.packet {
            let old = self.packet;
            for class in TrafficClass::ALL {
                let i = class.index();
                let cb = self.cfg.classes[class];
                self.floor[i].rescale(
                    old,
                    new,
                    floor_burst(cb.burst_packets),
                    Self::rate_of(cb.floor_kbps),
                );
                if let (Some(b), Some(cap)) = (self.cap[i].as_mut(), cb.cap_kbps) {
                    b.rescale(old, new, cb.burst_packets, Self::rate_of(cap));
                }
            }
            let burst = self
                .cfg
                .classes
                .iter()
                .map(|(_, c)| c.burst_packets)
                .max()
                .unwrap_or(1);
            self.budget
                .rescale(old, new, burst, Self::rate_of(self.cfg.insertion_kbps));
            self.packet = new;
        }
        let d = slot.saturating_sub(self.last_slot);
        if d > 0 {
            for b in &mut self.floor {
                b.refill(d);
            }
            for b in self.cap.iter_mut().flatten() {
                b.refill(d);
            }
            self.budget.refill(d);
            self.last_slot = slot;
        }
    }

    fn budget_ok(&self) -> bool {
        self.budget.has_packet(self.packet)
    }

    /// Whether `class` may be served in `phase` right now.
    pub(super) fn allows(&self, class: TrafficClass, phase: Phase) -> bool {
        if !self.budget_ok() {
            return false;
        }
        let i = class.index();
        match phase {
            Phase::Floor => {
                self.floor[i].has_packet(self.packet)
                    && self.cap[i].is_none_or(|c| c.has_packet(self.packet))
            }
            Phase::Preempt | Phase::Excess => self.cap[i].is_none_or(|c| c.has_packet(self.packet)),
        }
    }

    /// Charges a served PDU of `packets` slots to the buckets its phase
    /// uses.
    pub(super) fn charge(&mut self, class: TrafficClass, phase: Phase, packets: u64) {
        let i = class.index();
        self.budget.charge(packets, self.packet);
        if let Some(c) = self.cap[i].as_mut() {
            c.charge(packets, self.packet);
        }
        match phase {
            Phase::Floor => self.floor[i].charge(packets, self.packet),
            Phase::Preempt => self.floor[i].charge_saturating(packets, self.packet),
            Phase::Excess => {}
        }
    }

    /// Slots from now until `class` could next be served in some phase,
    /// ignoring whether it has work: the idle hint's refill term. The
    /// excess phase needs only cap room and budget, so floors never make
    /// a class wait longer than this.
    pub(super) fn wait_slots(&self, class: TrafficClass) -> u64 {
        let cap = self.cap[class.index()].map_or(0, |c| c.wait(self.packet));
        cap.max(self.budget.wait(self.packet))
    }
}
