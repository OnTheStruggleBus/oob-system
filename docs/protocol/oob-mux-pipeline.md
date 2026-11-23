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
| Scheduler | `oob_mux::scheduler` | Accepts PDUs from producers (`SchedulerHandle::submit`): one-shot or cyclic (carousel), each with a traffic class C0–C5 (F6-3), a within-class priority, an optional release time (C1 spacing set by the command layer, F6-11) and an optional deadline. Hands out one PDU at a time, only when a pull needs one. | Per-class queues, carousel registry (in memory; oob-dac rebuilds them on start) |
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
  slot. Rate accounting (class budgets, #16) is done in slots, so the
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
documented number comes from #16's criterion bench. A pull asking for more
than `max_pull` is refused with `PullError::Oversized`, the caller's buffer
untouched and the stream not advanced, and counted: a garbled request from
the seam cannot make the mux allocate without bound. Zero values for
`max_pull`, `pull_budget`, `queue_depth` or `outcome_capacity` are refused
when the configuration is applied.

Carousels: replacing a carousel with new content (a new PSI version) makes
it due at once; re-submitting the same content keeps its schedule, so an
idempotent re-submit from oob-dac never raises its rate. A one-shot PDU is
still eligible at exactly its deadline, and expired one-shots are reaped
in every class on each dequeue.

## Counters

| Counter | Meaning |
| --- | --- |
| `nulls_emitted` | Null fill on shortfall. Normal operation, not an error. |
| `packets_served` | Non-null packets released (also per class). |
| `late_pulls` | A pull took longer than `pull_budget`. |
| `puller_underruns` | **Seam counter (#15), not a pacer counter.** The flowgraph timed out and inserted nulls itself; it reports the cumulative value in every seam request, and the seam's stats keep the last reported value. The G0.5 evidence counter. |
| `expired`, `stamper_panics`, `outcomes_lost`, `oversized_pulls` | Deadline drops, contained stamper panics, outcome-channel overflow, refused pulls above `max_pull`. |

All but `puller_underruns` live in `PacerStats`. Exposure as `/metrics` is
#29.

## Not here yet

The seam transport (#15: AF_UNIX stream socket to a custom GR block),
class floors and caps (#16; today classes are served in strict order C0
first, then priority, then arrival), and the PSI assemblers (#17) arrive as
sibling modules or replace the placeholders named above. Nothing in this
crate is hub-specific (AGENTS.md §9).
