# oob-mux pipeline: scheduler → packetizer → pacer

How the OOB transport stream is produced on the mux side of the pull seam.
Source of truth: the `oob-mux` crate docs (`cargo doc -p oob-mux`); this
page summarizes them. Built by issue #14 to ADR-002 decisions 2–3 and r6
F6-12, F6-2 and F6-16 (all accepted 2026-11-22).

**Status of the framing: `hypothesized`.** The 188-byte MPEG-2 transport
framing of the OOB stream is the r4 lead, corroborated only by the d 02 /
d 14 diagnostics. The packet header, the null PID 0x1FFF, section-in-packet
framing and the section CRC32 used here are generic MPEG-2 structure and
stay `hypothesized` until a capture in `lab/captures/` verifies them (the
box is the referee, AGENTS.md §2). SCTE 55-1 coding, interleaving and
randomization belong to the flowgraph, not this crate (F6-2).

## The three layers

| Layer | Module | Does | Owns |
| --- | --- | --- | --- |
| Scheduler | `oob_mux::scheduler` | Accepts PDUs from producers (`SchedulerHandle::submit`): one-shot or cyclic (carousel), each with a traffic class C0–C5 (F6-3), a within-class priority, an optional release time (C1 spacing set by the command layer, F6-11) and an optional deadline. Hands out one PDU at a time, only when a pull needs one, chosen by the class budget (see [the class budget](#the-class-budget-f6-3)). | Per-class queues, carousel registry, class token buckets (in memory; oob-dac rebuilds them on start) |
| Packetizer | `oob_mux::packetizer` | Turns the one PDU in flight into 188-byte packets on demand. Each section starts a packet (PUSI set, pointer_field 0); the last packet is 0xFF-stuffed. Stamps time-bearing PDUs as their first packet is released, then recomputes the CRC32. | Continuity counters per PID |
| Pacer | `oob_mux::pacer` | Answers `PullSource::pull(n)` with exactly `n` packets, null-filling any shortfall. Counts slots, served packets, nulls, late pulls. | The null packet; `PacerStats` |

Nothing is packetized ahead of a pull, so no queue of finished packets sits
between the layers, and stamp-at-pull (F6-16) is exact by construction: a
time field carries the UTC read when its first packet left the pacer.

## Null packets exist only in the pacer

`Pid` cannot hold 0x1FFF, `TsPacket` has no public constructor, and the
null packet is a private constant built with a key only the pacer's private
`null` module can make. `compile_fail` doctests in `oob_mux::pacer` show
that code outside the crate cannot reach a null constructor; a unit test
shows `Pid::new(0x1FFF)` is rejected.

## Time

- **Stream time is slots.** Every packet answered, served or null, is one
  slot. Rate accounting (the class budget, #16) is done in slots, so the
  flowgraph's clock governs the budget, as F6-2's pull seam intends.
- **The rate is a parameter.** `PacerConfig::packet_rate` converts slots to
  stream time and carousel periods to slots. It is runtime configuration
  (ADR-003 Layer 1) until O7 fixes the rate, and stays one afterwards.
- **Monotonic host time** is used only for release times, deadlines and
  timing each pull. **UTC** is read only for stamping.

## The pull is bounded

A pull does no I/O, never awaits, allocates nothing when the caller's
buffer has room for `max_pull`, and takes the queue lock only per dequeue.
It times itself: a pull longer than `pull_budget` is reported `late`. The
documented number comes from the criterion bench (see
[the bench](#the-bench)). A pull asking for more
than `max_pull` is refused with `PullError::Oversized`, the caller's buffer
untouched and the stream not advanced, and counted: a garbled request from
the seam cannot make the mux allocate without bound. Zero values for
`max_pull`, `pull_budget`, `queue_depth` or `outcome_capacity` are refused
when the configuration is applied.

Carousels: replacing a carousel with any change (new section bytes such as
a new PSI version, or a new PID, class, priority or period) makes it due at
once, at most once per period; re-submitting it unchanged keeps its schedule, so an idempotent
re-submit from oob-dac never raises its rate. `SchedulerConfig::validate`
and `PacerConfig::validate` let oob-dac check settings before saving
them. A one-shot PDU is
still eligible at exactly its deadline, and expired one-shots are reaped
in every class on each dequeue.

## The class budget (F6-3)

The ~1000 Kbps insertion budget (`docs/facts.yaml`, approximate) is shared
by six traffic classes. The class *semantics* are code (ADR-003 Layer 3,
`scheduler/class.rs`); the *numbers* are runtime configuration
(`BudgetConfig`, supplied by oob-dac's store, #25) and appear nowhere in
the crate outside test fixtures. All rates are kbps of 188-byte packets.

| Class | Carries | Guarantee | Ceiling | Notes |
| --- | --- | --- | --- | --- |
| C0 system | time, connect/config carousel | floor, highest priority | optional | The floor must cover the registered C0 carousels' demand |
| C1 addressed | lifecycle operations, EMMs, polls | floor | optional | A PDU is never released before its `release_at`; the scheduler infers no spacing (F6-11) |
| C2 EAS | emergency alerts | none: a C2 floor is refused | **required** | Preempts the C3–C5 floors, bounded by its cap; never touches C0/C1 floors. A cap of 0 is accepted and means no alerts are served |
| C3 guide | listings carousel | floor | optional | Can dip below its floor only while C2 runs, by at most C2's cap (Marshal, 2026-11-23) |
| C4 CDL | code download | none | **required** | Best effort |
| C5 lab | Discovery Lab injection | none | **required** | Isolated: off outside lab mode |

**Selection, per slot** (a slot is one packet the pacer must fill): the
insertion-budget bucket must hold a token; then phase A serves the C0 then
C1 floors, phase B serves C2 within its cap, phase C serves the C3, C4, C5
floors, and phase D gives any spare budget to the first class, in class
order, with cap headroom. A PDU in flight is never preempted. Each class
has a floor bucket and a cap bucket counted in slots, so the budget
follows the flowgraph's clock.

**Lab mode** is the only class gate. With `lab_mode = false`, `submit`
refuses C5 (`ClassDisabled`) and `apply_budget` drops queued C5 work,
reporting each one-shot `Dropped(LabModeOff)`. Registered C5 **carousels**
are removed too, with no outcome event, and each is counted in
`dropped_by_class`. oob-dac's later `withdraw(id)` of one returns
`UnknownCarousel`, and switching lab mode back on does not restore it:
producers must register their C5 carousels again.

**Validation.** `oob_mux::scheduler::validate(&BudgetConfig, StreamParams)`
is pure and exported for oob-dac's save path (#26). It reports every
violation with a stable name:

| `reason()` | Refused because |
| --- | --- |
| `c0_floor_zero` | C0 has no floor |
| `floors_exceed_budget` | The enabled classes' floors sum above the budget |
| `preemptor_can_starve_c0` | C0 floor + C1 floor + C2 cap exceed the budget |
| `cap_below_floor` | A class's cap is under its floor |
| `budget_exceeds_stream` | The budget is above the stream rate |
| `c2_floor_unsupported` | C2 has a floor; it is served by preemption within its cap, so a floor would reserve budget and guarantee nothing |
| `c2_uncapped`, `c4_uncapped`, `c5_uncapped` | A required cap is missing |
| `zero_burst` | A class's bucket depth is 0 |
| `c0_floor_below_demand` | (apply only) C0's floor is under the registered C0 carousel demand |

`SchedulerHandle::apply_budget` runs `validate` plus the demand check and
swaps at the next pull boundary with no burst granted.
`SchedulerHandle::check_budget` is its non-mutating dry run, so oob-dac's
save path (#26) can refuse a config before it commits the store row, and
`c0_demand_kbps()` reads the demand the floor must cover. The reverse door
is closed too: registering a C0 carousel that would push demand above the
floor fails with `SubmitError::C0DemandExceedsFloor`, and a changed
replacement of a carousel may jump its schedule at most once per period,
so churn cannot raise the demand (#14 amendment 4). Demand is compared at
kbps resolution, rounded up, so a floor a fraction of a kbps under the
true demand is refused.

**Tolerance.** A bucket is `burst_packets` plus one slot of refill deep (a floor
bucket one packet deeper, so it keeps accruing while it waits for a
budget token) and
may go into debt by the tail of a multi-packet PDU, so over any window a
class gets `rate × window ± (burst + longest PDU)` packets.

**Evidence** (box-free; these are class-budget tests, not wire claims):
`tests/sim_floors.rs` runs every class saturated on a virtual slot clock
and asserts floors and caps over sliding windows, C2 preemption, C5
silence with lab mode off, C1 release times and no inferred spacing, and
that every random config `validate` accepts keeps C0 at its floor.

## The bench

`cargo bench -p oob-mux --bench scheduler` (criterion) pulls with all six
classes saturated. On the development machine (Apple silicon, release),
`Pacer::pull(64)` costs about 44 µs with 24 PDUs queued per class and
about 1.8 ms with 1000 queued per class: a pick scans its class's queue,
so the cost grows linearly with queue depth, bounded by the
operator-set `queue_depth` and `max_carousels` (the candidate scan also
filters the carousel registry once per class). Producers wait on the queue
lock for each scan. A heap or index is a tracked follow-up to #16, to be
done if depth or `max_carousels` grows. Both are far inside the
~47 ms a 64-slot pull covers at 2.048 Mb/s; set `pull_budget` from the
deployment's own bench run, not from this one.

## Counters

| Counter | Meaning |
| --- | --- |
| `nulls_emitted` | Null fill on shortfall. Normal operation, not an error. |
| `packets_served` | Non-null packets released (also per class). |
| `late_pulls` | A pull took longer than `pull_budget`. |
| `puller_underruns` | **Seam counter (#15), not a pacer counter.** The flowgraph timed out and inserted nulls itself; it reports the cumulative value in every seam request, and the seam's stats keep the last reported value ([`SeamStats`](seam.md#counters)). The G0.5 evidence counter. |
| `expired`, `stamper_panics`, `outcomes_lost`, `oversized_pulls` | Deadline drops, contained stamper panics, outcome-channel overflow, refused pulls above `max_pull`. |
| `served_by_class`, `served_bytes_by_class` | Non-null packets, and their wire bytes, per class C0–C5. |
| `expired_by_class`, `dropped_by_class` | Per-class deadline drops, and drops for lab mode off. |

All but `puller_underruns` live in `PacerStats`. Queue depth per class is a
gauge, `SchedulerHandle::queue_depths()`. Exposure as `/metrics` is #29.

## Not here yet

The seam transport is built (#15): see [the pull seam](seam.md). Class
floors and caps (#16) are built, see above; the PSI assemblers (#17)
arrive as a sibling module. Nothing in this
crate is hub-specific (AGENTS.md §9).

## Box-free stream verification

Built by issue #18 to r6 F6-15 (accepted 2026-11-22) and ADR-002 Option C.
CI shows the stream is *valid* before a box iteration; it never shows the
box accepts it.

**None of this is box evidence.** Every golden vector header reads
`status: hypothesized — regression lock, not box-verified`: a vector made
by this crate proves only that its bytes did not change. **tsduck proves
generic MPEG-2 conformance only** (section syntax, CRC32, continuity, PID
references). The GI/DCII descriptors and table layouts are private to it:
it reports them as unknown or private, and that is neither a failure nor a
proof. `d 15` / `d 16` on a box remain the referee for the DCII bits (r4
F4-6); only a capture in `lab/captures/` promotes a status (AGENTS.md §2).

| Layer | Where | What it proves |
| --- | --- | --- |
| Golden vectors | `crates/oob-mux/tests/golden/*.hex`, loader `tests/common/golden.rs` | Exact bytes of the null packet, one packet per PID class (PAT, CAT, PMT, EMM, private), a one-packet section, sections ending exactly on a packet boundary and a multi-packet section. The PAT, the minimal PMT and the null packet were also cross-checked by hand against ISO/IEC 13818-1 and tsduck's decoder; the cross-check is in each file's header. `OOB_BLESS=1` rewrites a vector after a deliberate change (the header is kept). |
| Properties | in-crate (`packetizer::section`, `::cc`, `::tests`) and `tests/stream.rs` | CRC32 of a section with its CRC is zero and any single bit flip breaks it; counters run +1 mod 16 per PID across any interleaving, wrap at 16, never disturb another PID, and the null PID is exempt; a section of any length spans `packets_for(len)` packets, reassembles byte-identical and has `pointer_field` 0 on every payload-unit-start packet; `pull(n)` returns exactly `n` for `n <= max_pull`. |
| Whole stream | `cargo run -p oob-mux --example gen_stream` | A pure function of its arguments (`ManualClock`, fixed submissions, fixed xorshift seed): 20 000 packets with the class mix of C0 to C5 through the real scheduler, packetizer and pacer. |
| tsduck | CI job `tsduck analyze`, `ops/ci/tsduck-analyze.py` | The independent oracle over the generated stream: no continuity error, no section tsduck rejects (CRC32 or syntax), no invalid sync or transport error, no PID outside the PAT/PMT/CAT (the null PID is the one allowlisted name), exactly the declared PIDs. The script's self-test corrupts a copy four ways and requires each to be rejected. |

Run it locally (tsduck 3.45; `brew install tsduck` or the release package):

```text
cargo run -p oob-mux --example gen_stream -- --out /tmp/oob-gen.ts
python3 ops/ci/tsduck-analyze.py --stream /tmp/oob-gen.ts   # or no argument: regenerate
python3 ops/ci/tsduck-analyze.py --self-test                # checker rejects bad streams
```

The OOB stream has no PCR, so the rate is given to tsduck with `--bitrate`;
its one home is `docs/facts.yaml` (`oob_stream.stream_rate_mb_s`). The
tsduck package is pinned by version and sha256 in the workflow. Out of
scope: the loopback *receive* flowgraph that records TX output for replay
(F6-15's fourth bullet) belongs to oob-dev-rf.

