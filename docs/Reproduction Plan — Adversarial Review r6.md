# Reproduction Plan — Adversarial Review r6 (Software Architecture)

- **Date:** 2026-11-22
- **Scope:** the runtime software architecture implied by the full corpus —
  ADR-001 rev 8, the Reproduction Plan, Milestones & Stage Gates rev 2, the
  Console spec, the DAC Operations Reference, the S2/S4/S5 specs — checked
  against the repository as it stands (`main` @ `a8103a1`: Slice A merged
  in PR #5; AGENTS.md §6 "Marshal's merge is the approval" merged in
  PR #7; `main` protected by rulesets requiring a PR and the six CI checks,
  with no bypass).
- **Status:** dispositioned 2026-11-22 by Marshal — all sixteen findings
  accepted; F6-4 accepted **with amendment**, recorded as
  `ADR-003 — Configuration, Extensibility and Distribution Model.md`
  (see §5)
- **History:** first proposed in PR #6, which merged before its role
  reviews and was reverted in PR #8. Re-proposed unchanged in substance;
  only the repository-state references are refreshed.
- **Companions:** `ADR-002 — Oob Runtime Architecture.md` (accepted
  2026-11-22), which carries the decision record for F6-1, F6-2, F6-3,
  F6-9 and F6-12; `ADR-003 — Configuration, Extensibility and
  Distribution Model.md` (accepted 2026-11-22), the amended F6-4.

## 1. Verdict

r3–r5 hardened the *plan*: discovery method, hardware, gates, evidence,
docs. None of them reviewed the *software* as a system. The software is
specified only as a list of crates. The documents never say which crates
are processes, how they talk, who owns the clock, who owns which state, or
how the Console's write path squares with config-as-code. Nothing is built
yet (three stub crates), so every finding below is cheap to fix now and
expensive after Slice C/D land.

The approach survives. The crate split matches the DAC's own module map,
box-as-oracle is still the right method, and API-first is right for the
Console. Here are the deltas.

## 2. Findings

### F6-1 — No runtime topology: crates are named, processes are not (high, architecture)

**Attack.** The documents describe the runtime four different ways:

- Overview §4 draws `oob-dac ── oob-mux ── oob-msg ── oob-cdl ── oob-playout`
  as a linear pipeline.
- ADR-001 §4 says the stream is "muxed by tsduck".
- Plan §8 has a Rust TS writer, with tsduck only as scaffolding.
- Console §2 calls oob-dac "the ONLY backend", fronting
  "msg/oob-mux/cdl/playout engines".

Nothing says which crates are libraries and which are daemons, what crosses
a process boundary, or what each container runs.

**Why it matters.** Slice C (oob-mux, oob-msg) and Slice D (oob-dac) are next
on the board. Without a topology they will each invent an IPC surface, or
assume one does not exist.

**Delta.** Adopt ADR-002:

- One control-plane daemon (`oob-dac`). It owns state, the scheduler and
  the API, and links `oob-msg`, `oob-cdl`, `oob-guide` and `oob-mux` as
  libraries.
- One **data-plane boundary**, at the PHY (F6-2).
- `oob-playout` is a separate supervisor process in the host-network
  container (F4-7).
- The lab harness and the Console are API clients. They are not second
  injectors.
- Redraw the overview §4 diagram to match.

### F6-2 — The mux ↔ PHY timing contract is unspecified (high, oob-mux / W1)

**Attack.** The corpus says the flowgraph "owns timing" (AGENTS.md §8, plan
§8), the stream is "null-stuffed by the modulator" (ADR-001), and oob-mux
emits a "null-stuffed stream" (PR #5 crate doc). It names no transport
(UDP, ZMQ, TCP, FIFO), no backpressure, and no single owner of null
insertion.

**Why it matters.** If oob-mux *pushes* at its own pace, the host clock and
the HackRF sample clock drift apart. The flowgraph's buffer then either
underruns, leaving carrier gaps that read as d 02 data-activity dropouts and
eventually `E 01` once Control Channel Timeout expires, or overruns, adding
latency and dropping packets. Insert System Time is also stamped at enqueue
rather than emission, so it carries queue latency. A box-visible fault would
then look like a protocol fault, poisoning the oracle loop that G1 depends
on.

**Delta.**

- **The flowgraph is the clock master, and the seam is a pull.** A custom GNU
  Radio source block (or a bounded ZMQ REQ/REP) asks oob-mux for *n*
  packets per `work()` call.
- oob-mux answers from its class queues (F6-3) and fills any shortfall with
  null packets (PID 0x1FFF). Null insertion lives in exactly one place: the
  mux's pacer.
- Time-bearing messages are stamped at pull time.
- The seam's unit is the transport packet. SCTE 55-1 coding, interleaving
  and randomization stay in the flowgraph.
- The O7 outcome fixes the packet rate. Until then the pacer is
  parameterized and the r4 MPEG-TS-framed lead stays labeled
  `hypothesized`.
- G0.5 exit gains one evidence item: a 24 h loopback run with zero pull
  underruns, plus the null ratio logged.

### F6-3 — No traffic-class model for the ~1000 Kbps insertion budget (high, oob-mux / scheduler)

**Attack.** Everything competes for one insertion budget:

- the connect/config carousel every rebooted box needs to reach CONFI
- Insert System Time and Compute DST
- addressed operations (at least 250 ms between terminals)
- EMMs, including download control
- the CDL carousel (large objects)
- the always-on guide carousel (F3-10)
- EAS
- Discovery Lab injection

The corpus names the budget, the "insertion shaper" and the Console's
budget gauge, but no allocation policy.

**Why it matters.** A G4 download campaign at full rate starves the guide
carousel, which then fails the G6 cold-box repopulation metric (N minutes).
A lab A/B run starves connect, and the boxes flap. Starvation like this
shows up as an intermittent box-side failure and gets misread as a protocol
defect.

**Delta.** The shaper becomes a class-based scheduler with **reserved floors
and caps**. Per-class rates live in the plant config, not in code.

| Class | Traffic | Policy |
| --- | --- | --- |
| C0 system | time, connect/config carousel | guaranteed floor, highest priority |
| C1 addressed | lifecycle ops, EMMs, polls | floor; honors per-PDU release times set by the command layer (F6-11) |
| C2 EAS | alert messages | preempts C3–C5 |
| C3 guide | listings carousel | floor sized from measured N (G6) |
| C4 CDL | download carousel | best-effort, capped |
| C5 lab | Discovery Lab injection | capped, isolated, off outside lab mode |

Back it with three tests:

- a criterion bench (already in the toolchain)
- a deterministic simulation test that asserts every floor holds under
  saturation
- per-class rate metrics (F6-14)

### F6-4 — Two write paths into the single source of truth (high, config / process)

> **Accepted with amendment (2026-11-22).** The delta below is the one
> originally proposed and is **superseded** by ADR-003: the running
> system is fully configurable at runtime and opens no PRs; the
> `ops-state` branch is replaced by a local YAML export. Kept here as
> the review record.

**Attack.** Two documents disagree on how `ops/` changes:

- **AGENTS.md §5/§6:** `ops/units.yaml` and `ops/lineup.yaml` change only
  through git, and any change touching `ops/` merges by a PR that Marshal
  merges after role reviews (his merge is the approval).
- **Console spec §2/§4.2/§4.5:** "config edits = commits (author =
  operator)". The enrollment wizard commits to `units.yaml`. The lineup
  editor commits behind a hook the owner can override.

A daemon cannot auto-commit to `main`: the rulesets now require a PR and
allow no bypass. Committing to a local clone instead silently forks the
source of truth.

**Why it matters.** Enrollment *is* the unit DB (plan §3.2), and it is an
everyday operator action. As specified it is either impossible under branch
protection, or it bypasses the review rule.

**Delta.** Classify configuration by change cadence:

- **Design-time** (lineup, PID plan, plant/class budgets, message templates
  promoted to `verified`): PR-gated. The Console's editors *open a PR*
  through the API and never commit to `main`.
- **Operational** (enrollment records, campaigns, entitlement changes):
  runtime store plus an append-only journal. oob-dac periodically exports a
  snapshot to a machine-written, unprotected `ops-state` branch. That branch
  is reviewable and diffable, but does not gate.
- Split `units.yaml` to match: **census identity** (hardware facts, cord
  check, role) stays PR-gated. **Terminal operational record** (Terminal ID,
  plants, activation) lives in the DB and is journaled.

### F6-5 — Durability is per table, not per database (medium-high, data)

> **Accepted 2026-11-22.** Where this delta says "journaled to
> `ops-state` (F6-4)", read the local append-only journal and optional
> local YAML export of ADR-003 §6; no `ops-state` branch exists.

**Attack.** F5-12 was dispositioned as "the unit DB is derived state —
rebuildable from ops config plus box re-enrollment, excluded from backup."
Since then, S4 and S5 have put precious state in the same sqlite file:

- the IPPV purchase ledger
- entitlements granted by billing through the admin API
- the program database
- the G4 campaign ledger, which is gate evidence

Uploaded IPPV purchases are cleared box-side once acknowledged.
Re-enrollment cannot rebuild what billing granted.

**Why it matters.** If the "derived, no backup" disposition is applied to the
whole file, the first disk loss after S4/S5 destroys ledgers that nothing
can regenerate.

**Delta.** Amend F5-12 into a classification table:

- **Derived:** terminal status, last response, d-screen cache, carousel
  state. These may be rebuilt.
- **Precious:** the entitlement-change journal, purchase ledger, campaign
  ledger and audit log (bsilog). These must be backed up and/or journaled
  to `ops-state` (F6-4).

**Atomicity rule.** Many operations write both kinds of state in one
logical step: an entitlement change plus terminal status, or a purchase
upload plus its poll result. Splitting them across sqlite files breaks
that, because in WAL mode a transaction spanning `ATTACH`ed databases is
atomic per file but not across files. So:

- **Keep one database file** and classify durability per table. Back up
  the whole file continuously; on restore, rebuild the derived tables.
- Where derived state follows from a precious event, use an **outbox**:
  commit the precious event first, then derive the status from it
  idempotently, so a crash between the two is repaired on replay.

Schema migrations are versioned from the first table.

### F6-6 — "Protocol document as code" conflicts with runtime-editable templates (medium, oob-msg / Console)

**Attack.** `oob-msg` "*is*" the O4 protocol document as Rust code. Console
§4.3/§4.4 has the Discovery Lab editing versioned templates in git and
"one-click commit to the protocol document". AGENTS.md routes status
promotion through a capture and an oob-qa review.

If wire formats are Rust types, every hypothesis needs a recompile and a
redeploy. If they are data, "as code" needs a definition.

**Why it matters.** The W3 loop's throughput is the critical path (F3-2).
A rebuild per hypothesis throttles it. A web UI committing straight to the
protocol document bypasses review.

**Delta.** The **message catalog is data that compiles into code**:

- Per-family layout files (e.g., `crates/oob-msg/catalog/*.toml`) hold the
  field layout, address mode, status and evidence references. imhex
  patterns are kept as companions.
- The lab loads `hypothesized` templates at runtime.
- `documented` and `verified` families are compiled through `build.rs` into
  typed encoders.
- mdBook protocol chapters are generated from the same files.
- Lab "commit" means a branch plus a PR. Promotion to `verified` happens
  only in a PR carrying oob-qa review and a capture reference (F6-7).

### F6-7 — Encrypted captures make the evidence chain uncheckable in CI (medium, evidence)

**Attack.** Captures are born git-crypted (F5-11), and `verified` requires a
capture reference. But GitHub-hosted CI and the mdBook build hold no key,
and the fresh-clone test proves they can't read captures. So no automated
check can confirm that a `verified` claim points at real evidence.

**Why it matters.** "A claim without a capture is not evidence" (gate rule 6)
becomes an honor system at exactly the gates (G1, G3) that need it most.

**Delta.** Split each capture in two:

- **A plaintext manifest:** capture ID, an **HMAC-SHA256 of the
  decrypted payload, keyed with a secret held in the git-crypt tier**,
  timestamp, experiment ID, template hash, verdict, and a box-role
  pseudonym. Never the UA or serial. A plain hash would leak: many
  captures are low-entropy (a front-panel `E 00`, a single d-screen
  field), and an unkeyed hash of a short, known-format payload can be
  confirmed or brute-forced offline.
- **The encrypted payload.**

GitHub CI, which holds no key, checks that every `verified` catalog entry
resolves to a manifest entry with a matching template hash. The
key-holding lab runner (F6-8) recomputes the HMACs nightly. mdBook renders
manifests, never payloads.

### F6-8 — The CI topology doesn't match the hardware canary (medium, CI / safety)

**Attack.** Gates §6.5 specifies "containerized CI on the lab host", including
the nightly hardware canary. ADR-001 §2/§9 names Gitea. The actual CI is
GitHub Actions on hosted Ubuntu. The canary needs:

- the box
- the SDR
- the capture rig
- the git-crypt key

That means a self-hosted runner. On a public repo, a self-hosted runner that
takes `pull_request` jobs is arbitrary code execution on a host wired to an
RF transmitter.

**Why it matters.** It undermines the closed-plant doctrine and secrets
hygiene in one move, and gate rule 8 has no runner until it is settled.

**Delta.** Hybrid CI:

- GitHub-hosted runners for all PR checks.
- A lab runner that takes only `schedule` and `workflow_dispatch` jobs from
  `main`, never `pull_request`. It sits on the isolated lab VLAN with a TX
  safety interlock: a level cap in config, and the dummy-load path verified
  before keying.
- Repository setting: require approval for workflows from outside
  collaborators.
- Reconcile ADR-001's "Gitea" with the GitHub reality in one sentence.

### F6-9 — The container rationale predates the HackRF-primary decision (medium, deployment)

**Attack.** Plan §8 justifies containers partly because "the Pluto presents
iiod over its USB-network gadget — containers talk TCP, no USB passthrough".
F4-1 then made the **HackRF One (USB/libusb) primary**. AGENTS.md §10 pins
*rootless* Podman.

**Why it matters.** Rootless Podman, USB passthrough, near-realtime GNU
Radio scheduling and host-net multicast is the hardest combination in the
stack, and it sits on the G0.5 critical path.

**Delta.**

- Run the PHY flowgraph as a host `systemd` service with a udev rule for
  the HackRF.
- Containerize the Rust daemons and the Console.
- The F4-7 restart canary checks HackRF enumeration and resets the USB
  device on failure.
- Record this in ADR-002's deployment section.

### F6-10 — Destructive-operation safety is enforced in the UI, not the server (medium, safety / API)

**Attack.** Typed confirmation (`FACTORY INIT <serial>`), sacrificial-box
defaults and dry-run-first are specified in Console §4.2/§4.8. AGENTS.md §7
makes typed confirmation a hard rule. But the Console is only one client.
The lab harness, the billing engine, a WireLink endpoint, scripts and agents
all reach the same API.

**Why it matters.** One curl call against the API can cold-init a fleet box,
or run a CDL campaign before G4.

**Delta.** A server-side **policy layer** in oob-dac's command path:

- Destructive operation classes need a short-lived confirmation token,
  bound to (operation, target serial), from a separate call.
- Owner role only.
- A **sacrificial-only mode** for CDL campaigns until G4 passes, as a
  config flag Marshal flips.
- Dry-run is the default for bulk operations.
- Every command is written to the audit log.

### F6-11 — One command model, several front doors (medium, oob-dac)

**Attack.** Operations can enter oob-dac through:

- the REST "WireLink-verb" API (Console)
- a period WireLink 600/700 TCP server endpoint (S4/S5 specs)
- the Billing API through the admin API
- lab harness scripts
- scheduler-generated auto operations (Insert System Time, Channel Map, …)

**Why it matters.** Each door that reimplements targeting, the 250 ms
spacing, the status lifecycle or the F6-10 policy creates a fork in DAC
semantics. The Define Operations model is the manual's single abstraction,
and the code should keep it single.

**Delta.**

- One internal **command layer**: an enum of the §11 catalog plus the
  Define Operations targeting and lifecycle. It is the only path into the
  scheduler.
- **Campaign pacing lives here and nowhere else.** The ≥250 ms
  inter-terminal delay and Repeat-Every belong to a *bulk operation*, so
  only the command layer knows which PDUs belong to which campaign. It
  enforces the spacing by setting each PDU's release time. The F6-3 shaper
  only honors those release times; it never infers campaign spacing.
- REST, WireLink-wire, billing and lab entry points are adapters over it.
- OpenAPI is generated from the REST adapter.
- The WireLink-wire endpoint has no authentication, true to the period, so
  it binds to localhost or the lab VLAN on its own port.

### F6-12 — The S2 seam is free to cut now, expensive later (medium, oob-mux / Interconnect)

**Attack.** Interconnect §4 has the master oob-mux emitting **message PDUs**
while a hub service, `oob-regen`, does 55-1 framing and null-stuffing. That
is a different decomposition from a monolithic null-stuffed TS writer. Also,
`oob-regen` is not in AGENTS.md §4's frozen name set.

**Why it matters.** If Slice C builds a monolith, S2 starts with a refactor
of the most timing-sensitive crate.

**Delta.** Cut oob-mux into three layers inside one crate, at zero schedule
cost:

1. **Scheduler:** PDUs carrying class, priority and deadline.
2. **Packetizer:** PDUs to sections and packets.
3. **Pacer:** pull-driven null-stuffing at the clock master (F6-2).

The local deployment colocates all three. S2 moves the pacer to the hub.
Either register `oob-regen` with Marshal, or define it as "oob-mux in pacer
mode". Pre-G6 work is limited to the seam itself (AGENTS.md §9).

### F6-13 — The operating contract has drifted from the dispositioned corpus (medium, process)

**Attack.** AGENTS.md "wins for how you work", but it is stale against the
2026-11-22 disposition set:

| Topic | AGENTS.md says | Dispositioned corpus says |
| --- | --- | --- |
| Review findings | r3/r4 await disposition; r5 not mentioned | r3, r4 and r5 all accepted (Overview §9, Gates rev 2) |
| Brand skin | ArchaiTel Digital Cable | superseded by a per-deployment config slot |
| Release tags | bare `75.25` | `rel/75.25` (F5-13) |
| OOB stream rate (§8) | ~1.544 Mb/s at 1.024 Msym/s | 2.048 Mb/s at 1.024 Msym/s (ADR-001); 1.544 Mb/s is the 55-2 Mode B rate |
| G0.5 first agenda item | r3/r4 dispositions | already closed |

AGENTS.md is not the only stale document:

- Console §4.3 repeats the 1.544 Mb/s figure.
- The DAC Operations Reference §9 still maps SEM scrambling to a
  "`playout` scrambler stage", which the SEM-ACP decision superseded.

**Why it matters.** Drift has already caused a defect. The PR #5 role review
followed AGENTS.md and reworded the `oob-mux` and `oob-dac` crate docs to
call F5-6 and F5-12 "pending disposition". Both are dispositioned. The
wording was reverted in PR #5 (`c51d810`).

**Delta.**

- One docs sweep commit to bring AGENTS.md, the Console spec and the DAC
  Reference §9 in line.
- Add a small **facts registry** (`docs/facts.yaml`: stream rates, symbol
  rate, tag format, disposition state per review round). The mdBook build
  checks the prose against it, which F5-10's CI makes possible.

### F6-14 — No observability contract (low, ops)

**Attack.** ADR-001 names VictoriaMetrics and Zabbix. The Console wants live
telemetry over WebSocket. No daemon has a defined metrics surface.

**Delta.**

- Every daemon exposes a Prometheus-format `/metrics` endpoint, which
  VictoriaMetrics scrapes. Named series include:
  - per-class insertion rate, null ratio and pull underruns (F6-2, F6-3)
  - carousel cycle time
  - per-unit last-seen
  - command lifecycle counts
- Console WebSocket channels are views over the same series.
- Zabbix alerts on them.

### F6-15 — Box-free verification of the stream is unplanned (low, test strategy)

**Attack.** Every correctness signal for oob-mux and oob-msg is box-side. CI
cannot catch a continuity-counter bug, a bad section CRC or a broken PID
plan before it costs a box iteration.

**Delta.**

- Golden-vector tests for every packetizer output.
- Property tests for CRC32, continuity counters and section boundaries.
- A tsduck `tsp -P analyze` pass over generated streams in CI.
- A loopback receive flowgraph that records the TX output for replay in
  regression tests.

Box iterations are then spent only on questions the box alone can answer.

### F6-16 — Host time discipline is scheduled too late (low, time)

**Attack.** Gates §6.6 lists chrony as a "later-phase addition". But Insert
System Time and Compute DST are hunted from day one (F5-8), and the guide's
time slots depend on them.

**Delta.**

- Move chrony into the base host build.
- oob-mux stamps time at pull (F6-2).
- Add DST-transition test vectors to the time family's catalog entry.

## 3. What survived

- **The crate decomposition.** It maps one-to-one onto the DAC's own module
  table (Operations Reference §9). It needs a topology, not a rework.
- **Box-as-oracle, the tiered G1 exit, and evidence-as-commits.**
  Strengthened by F6-6/F6-7, not challenged.
- **The SEM-for-QAM-and-ACP decision.** No software finding touches it.
- **API-first Console with no Console-side database.** Correct. F6-4 only
  fixes where its writes land.
- **Enrollment as an operator-side unit-DB action** (plan §3.2). F6-4 and
  F6-5 give it a durable home.

## 4. Disposition table

| Finding | Severity | Proposed sequencing | Disposition (Marshal, 2026-11-22) |
| --- | --- | --- | --- |
| F6-1 | high | ADR-002 approval before Slice C/D start | Accepted as proposed (ADR-002 accepted) |
| F6-2 | high | ADR-002; G0.5 exit evidence (24 h loopback, zero underruns) | Accepted as proposed (ADR-002); G0.5 exit item added to the Gates doc |
| F6-3 | high | Slice C design; test lands with the shaper | Accepted; per-class floors/caps are operator settings editable at runtime (ADR-003); the server refuses a configuration that starves C0 system |
| F6-4 | high | Before C0 enrollment wizard; amends AGENTS.md §5 | **Accepted with amendment** → ADR-003 (supersedes the §2 F6-4 delta) |
| F6-5 | medium-high | Amends F5-12; before the first precious table exists | Accepted as proposed (amends F5-12); adds one-click full backup/restore and whole-install export — the DB is now the installation |
| F6-6 | medium | Slice C (oob-msg) design | Accepted; user-defined templates (incl. imported community packs) load at runtime; upstream `documented`/`verified` compile via `build.rs`; Slice C |
| F6-7 | medium | Before G1 entry (with F5-11) | Accepted; before G1 entry, with F5-11 |
| F6-8 | medium | Before the canary exists (post-M1); repo setting now | Accepted; repo setting "require approval for workflows from outside collaborators" now (owner action); lab runner post-M1 |
| F6-9 | medium | ADR-002 deployment section; G0.5 bench | Accepted as proposed (ADR-002 deployment section) |
| F6-10 | medium | Slice D (oob-dac API) | Accepted; Slice D — the server-side guardrail that makes full configurability safe, for every client incl. forks and scripts |
| F6-11 | medium | Slice D (oob-dac API) | Accepted; Slice D |
| F6-12 | medium | Slice C (seam only; zero pre-G6 hub work) | Accepted as proposed |
| F6-13 | medium | Immediate docs sweep (PR #5 wording already reverted) | Accepted; sweep done in the r6 disposition PR (incl. the ADR-003 follow-up locations), `docs/facts.yaml` added; the mdBook prose-vs-facts check is wired later by F5-10's CI work (separate oob-dev-core PR) |
| F6-14 | low | With each daemon's first binary | Accepted as proposed |
| F6-15 | low | Slice C | Accepted as proposed |
| F6-16 | low | Host build; F5-8 catalog entry | Accepted as proposed |

## 5. Disposition record (2026-11-22)

Marshal dispositioned r6 in the orchestrator session on 2026-11-22,
together with ADR-002 (G0.5 agenda). Recorded verbatim where quoted.

**Everything except F6-4:** "Accept as recommended". ADR-002 is
**Accepted**; F6-1, F6-2, F6-9 and F6-12 are accepted as proposed;
the remaining findings are accepted with the sequencing notes in the
§4 table.

**F6-4 — accepted with amendment.** Marshal's words:

> "This needs to be self contained fully user configurable"

> "Shouldn't open PRs to add boxes beyond new box _types_"

> "Think of this like BOSS it should be fully user configurable and meet
> the ability or end users to do most things or fork the project and do
> other things. It is a faithful not infiging recreation of this service
> for hobbiests and archival preservation"

The amended delta is recorded as
`ADR-003 — Configuration, Extensibility and Distribution Model.md`
(Accepted). It replaces F6-4's proposed delta: PRs and gates govern
developing Oob, never operating it; the running system needs no git and
opens no PRs; the `ops-state` branch becomes a local YAML export.

**Distribution rules** (asked alongside F6-4). Marshal chose **"Ship
sample content"**, **"Community pack format"** and **"Firmware import
tooling"**. He did **not** choose "Allow user content", so the
no-broadcaster-content rule stands: playout loops and listings remain
self-authored (AGENTS.md §5, ADR-003 §4).

**Consequences recorded elsewhere in the same PR:**

- F6-5 amends F5-12: the unit DB is no longer "derived, excluded from
  backup"; durability is per table in one file, and the whole file is
  backed up (ADR-003 §6).
- F6-13: AGENTS.md, the Console spec, the DAC Reference §9, the
  Overview, the Gates doc and the Interconnect spec swept;
  `docs/facts.yaml` added.
- F6-2: the G0.5 exit criteria gain the 24 h loopback item.
- F6-8: the repository setting is an owner action for Marshal; agents
  cannot change repository settings.

*Review closes where every round has: the approach survives, and here are
the deltas. This round's deltas are cheap only for as long as the crates
stay stubs.*
