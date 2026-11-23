# ADR-002 — Oob Runtime Architecture: Process Topology and the Mux ↔ PHY Seam

- **Status:** Accepted — Marshal, 2026-11-22 ("Accept as recommended",
  r6 disposition record §5)
- **Hardware-option delta (Marshal's direction, 2026-11-23):** the OOB TX
  is a **supported set** — ADALM-Pluto (range-hacked at the hunt
  carriers), HackRF One clone, and LibreSDR — and the **primary is chosen
  at the G0.5 bench** by the r4 F4-1 level log, not pinned in advance.
  This ADR stays accepted. Its decisions are unchanged; the delta changes
  only the SDR wording in Context and in Decision 6 and action 6 below.
  The r4 F4-1 record itself is unchanged by this note.
- **Date:** 2026-11-22
- **Deciders:** Marshal (approval); oob-architect (author role);
  oob-dev-core and oob-dev-rf (consulted, per AGENTS.md §14 step 3 —
  PR review is oob-qa and oob-architect only)
- **Addresses:** r6 F6-1, F6-2, F6-3, F6-9, F6-12
- **Builds on:** ADR-001 rev 8 (what to build); this ADR decides *how the
  running system is shaped*.
- **History:** first proposed in PR #6 (merged before review, reverted in
  PR #8); re-proposed for review; accepted at the r6 disposition.
- **Related:** ADR-003 (configuration model) — the per-class budgets in
  decision 4 are runtime operator settings, not repository config.

## Context

ADR-001 and the Reproduction Plan fix the crate set (`oob-dac`, `oob-mux`,
`oob-msg`, `oob-cdl`, `oob-playout`, `oob-guide`, `oob-starfone`), the PHY
(GNU Radio → an OOB SDR from the supported set: HackRF One, ADALM-Pluto or
LibreSDR, primary chosen at the G0.5 bench), and the deployment style
(Podman on Debian 13).
They do not decide:

- which crates run as processes
- where the process boundaries are
- who owns the OOB stream's clock
- how the ~1000 Kbps insertion budget is shared

Slice A (PR #5, merged) created the workspace. Slices C and D are next,
so this decision is due before they start.

Forces at play:

- **Timing.** The OOB stream must stay continuous. Gaps read box-side as
  loss of data activity, and eventually as `E 01`.
- **One authority for DAC semantics.** Targeting, 250 ms spacing, the
  status lifecycle and the safety policy must not fork across entry points.
- **Discovery throughput.** The W3 loop is the critical path (F3-2).
- **A one-operator lab.** Operational simplicity beats scale-out. The
  fleet is tens of boxes, not millions.
- **The S2 future.** Interconnect moves 55-1 framing to remote hubs
  (`oob-regen`). The seam should exist before it is needed.
- **The OOB SDR is USB or network-attached.** HackRF One and ADALM-Pluto
  attach by USB, which makes containerizing the PHY the hard case (F6-9).
  LibreSDR attaches by Ethernet and needs no USB passthrough.

## Decision

1. **One control-plane daemon, `oob-dac`.** It owns all state (unit DB,
   entitlements, ledgers), the scheduler, the policy layer and the API
   adapters (REST/OpenAPI, WireLink wire, billing). It links `oob-msg`,
   `oob-cdl`, `oob-guide` and `oob-mux` as libraries. Every operation
   enters through one internal command layer (F6-11).
2. **One data-plane boundary, at the PHY.** The GNU Radio flowgraph is the
   **clock master** and **pulls** transport packets from oob-mux's pacer
   over a bounded local channel (a ZMQ REQ/REP, or a custom GR source block
   over a Unix socket). The pacer serves from class queues and fills any
   shortfall with null packets. Null insertion lives here and nowhere else.
   Time-bearing messages are stamped at pull.
3. **oob-mux has three internal layers:** scheduler (PDUs with class,
   priority and deadline) → packetizer → pacer. Local deployments colocate
   them in-process. S2 moves the pacer to the hub.
4. **Class-based insertion scheduling** with reserved floors and caps
   (C0 system, C1 addressed, C2 EAS, C3 guide, C4 CDL, C5 lab — see r6
   F6-3). Budgets are **runtime operator settings** in oob-dac's store,
   editable through the API and Console (ADR-003); the server refuses
   any configuration that starves C0 system.
5. **`oob-playout` is a separate supervisor process** (ffmpeg/tsp jobs) in
   the host-network container (F4-7). It is controlled by oob-dac through
   the command layer.
6. **Deployment split.** The flowgraph runs as a host `systemd` service.
   The OOB SDR is the one chosen at the G0.5 bench (HackRF One,
   ADALM-Pluto or LibreSDR; Marshal's direction 2026-11-23). A USB-attached
   radio gets a udev rule; a network-attached LibreSDR gets a reachability
   check in place of one. The Rust daemons and the Console SPA run in
   rootless Podman containers.
7. **Lab tooling and the Console are API clients only.** Discovery Lab
   injection is class C5 traffic through the same command layer and pacer.
   It is not a second stream source.

## Options Considered

### Option A — Control-plane daemon + pull-driven PHY seam (SELECTED)

| Dimension | Assessment |
| --- | --- |
| Complexity | Low–medium: one Rust binary, one IPC seam |
| Cost | No new infrastructure |
| Scalability | Ample for a lab fleet; the S2 seam is pre-cut |
| Team familiarity | Rust + GNU Radio, already the stack |

**Pros:** one clock owner and one null-inserter, so timing faults can't
masquerade as protocol faults. One command path. In-process calls between
the scheduler and the encoders keep the W3 loop fast. The S2 refactor
reduces to moving the pacer.

**Cons:** the daemon is a larger blast radius (a crash stops the API and the
stream). A custom GR source block or ZMQ glue has to be written and tested.

### Option B — Service per crate (oob-dac, oob-mux, oob-cdl, oob-guide daemons over gRPC/NATS)

| Dimension | Assessment |
| --- | --- |
| Complexity | High: N services, a schema per interface, distributed state |
| Cost | Broker plus per-service ops |
| Scalability | Unneeded at lab scale |
| Team familiarity | One operator maintaining a distributed system |

**Pros:** fault isolation, and crates deploy independently.

**Cons:** the insertion budget becomes a distributed coordination problem,
and the scheduler needs a global view, which reintroduces a coordinator.
DAC semantics can fork across services. Many more moving parts for one
operator, and no requirement that needs them.

### Option C — tsduck-centred stream (Rust emits sections → `tsp` paces and null-stuffs → flowgraph)

| Dimension | Assessment |
| --- | --- |
| Complexity | Low to start |
| Cost | Free; tsduck is already on the bench |
| Scalability | Fine |
| Team familiarity | High for bench work |

**Pros:** fastest path to a G0.5 bench stream, and `tsp`'s analysis plugins
come for free.

**Cons:** `tsp` paces against the host clock, not the SDR's. That is the
push-drift problem (F6-2) moved, not solved. The class scheduler would live
outside Rust, and so outside the tests and benches. It does not produce the
S2 PDU seam.

**Role under this ADR:** bench scaffolding for G0.5 and the `tsp -P analyze`
verification step in CI (F6-15). It is not the production path.

## Trade-off Analysis

The decisive force is **who owns the clock**. Only the SDR's sample clock
defines when a symbol leaves. Any push design (B or C) needs drift
compensation against a clock it can't see. A pull design gets correct pacing
by construction, and confines null insertion to one testable component.

The second force is **semantic singularity**. The DAC's Define Operations
model is one abstraction. A single daemon with one command layer keeps it
single. Option B distributes it.

Option A's cost is a single failure domain. At lab scale that is the right
trade. Add a systemd restart policy, have the flowgraph emit pure nulls when
the daemon disappears (boxes keep carrier lock and tolerate a long Control
Channel Timeout, which the plan already sets during development), and alert
on restarts.

## Consequences

**Easier:**

- Box-side faults can be attributed: pacer underrun metrics separate PHY
  timing from protocol errors.
- Slices C/D have a defined internal API: command layer → scheduler →
  pacer.
- The S2 hub becomes a relocation, not a rewrite.

**Harder:**

- Custom GR block or ZMQ glue, plus a 24 h zero-underrun loopback test, as
  G0.5 evidence.
- The daemon must be robust. Panics in encoders are caught per command,
  never per process.

**To revisit:**

- the transport packet rate and framing once O7 resolves (the pacer is
  parameterized until then)
- whether `oob-playout` folds into oob-dac once its job control is simple
  enough
- the `oob-regen` naming with Marshal before any S2 work (AGENTS.md §4
  naming freeze)

## Action Items

1. [x] Marshal: approve, amend, or reject this ADR (gates Slice C/D
   design) — accepted 2026-11-22.
2. [ ] oob-architect: redraw Overview §4 and Plan §5 diagrams to the
   topology above; reconcile ADR-001's "muxed by tsduck" and "Gitea"
   phrasing.
3. [ ] oob-dev-rf: prototype the pull source block (or ZMQ REQ/REP) against
   a stub pacer; 24 h loopback, zero underruns, null ratio logged (G0.5
   evidence).
4. [ ] oob-dev-core: the oob-mux scheduler/packetizer/pacer module
   skeleton in Slice C, with the class-floor simulation test and criterion
   bench.
5. [ ] oob-dev-core: the oob-dac command layer and policy layer as the
   first Slice D module, before any API adapter.
6. [ ] ops: host systemd unit for the flowgraph, plus a udev rule for the
   chosen SDR when it is USB-attached (HackRF One or ADALM-Pluto) or a
   reachability check when it is LibreSDR (Ethernet). The container-restart
   canary checks enumeration or reachability for the chosen SDR. The choice
   is made at the G0.5 bench (Marshal's direction, 2026-11-23).
7. [ ] Docs: the same-commit rule applies. This ADR, the redrawn diagrams,
   and the mdBook architecture chapter land together.
