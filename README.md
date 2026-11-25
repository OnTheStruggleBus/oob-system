# Oob — an out-of-band headend for DCT-class cable boxes

*An MVPD of one.* — **Oob: An AncientEnterprise Venture**

**Oob** is a faithful software reproduction of a decommissioned
GI/Motorola cable television headend — the DAC 6000 controller, OM
1000/2000 out-of-band modulator, KLS/DLS code download system, STARVUE II /
STARFONE return paths, and the guide data feed — operated as a closed
home-lab RF plant so that real DCT2000/2500-series cable boxes boot,
authorize, tune, download code, and show their resident program guide
again.

The boxes are the referee: every behavior is validated against fleet
hardware diagnostics, never assumptions. **mini-headend** is the lab/estate
nickname; the guide stack is **VueGuide+**; the operator UI is **Oob
Console**. Oob is **operator-agnostic by design** — the guide and Console
brand skin is a per-deployment config slot, no preset operator identity
ships, and the project is built so others can stand up their own plants,
interconnect them, share configs, and transfer subscribers.

## What it reproduces

| Original system | Role in the original network | Oob realization |
| --- | --- | --- |
| **DAC 6000** (Digital Addressable Controller) | System brain: unit database, entitlements, message scheduler, operator interface | `oob-dac` — Rust daemon, sqlite unit DB, WireLink-verb admin API |
| **OM 1000/2000** (OOB Modulator) | Out-of-band control channel (QPSK, 1.024 Msym/s, 70–130 MHz) | GNU Radio flowgraph → SDR from the supported set (HackRF One clone, ADALM-Pluto range-hacked, LibreSDR; primary chosen at the G0.5 bench by the F4-1 level log) |
| **KLS/DLS** (Code Download System) | Firmware/application delivery to boxes as 68k objects over the OOB stream | `oob-cdl` — .obj/.dat loader + carousel builder |
| **SEM V8** (SmartStream Encryptor Modulator) | Edge QAM + MediaCipher encryption | **Period hardware in the estate** — Motorola SEM V8, P/N 507199-001-00, ACP encryption module confirmed |
| **STARVUE II** | Upstream return-path receiver (5–42 MHz) | SDR receive flowgraph (stretch S1) |
| **STARFONE** | Telephone return path for purchases/polls | `oob-starfone` — modem-bank bridge to an mgetty NAS (stretch S1) |
| **EPG inserter + TV Guide Interactive (i-Guide)** | Resident guide app fed listings over the OOB stream | **VueGuide+** (`oob-guide`) — donor app re-served over our own download system + synthesized listings feed |
| **DAC operator terminals + WireLink BSI** | Day-to-day operations: units, messages, lineups, downloads, purchases | **Oob Console** — web front-end over the WireLink-verb API |

Everything not marked "period hardware" is software we write, deployed as
rootless Podman containers on a Debian 13 host.

## Features

The feature set below is the design target, documented in `docs/`. Nothing
here claims working code until the gate evidence says so — see
[Progress](#progress-living-status) for what is actually built, and
[How we work](#how-we-work) for why.

### Control plane — `oob-dac`

- **Unit database** (sqlite) implementing the DAC's Define Terminal schema:
  terminal type (One-Way / Fone-Way / Two-Way / Ethernet / DOCSIS), 40-bit
  unit address, 24-bit DAC terminal ID, serial, account, plant assignment,
  poll state, chronic non-responder tracking.
- **Enrollment done the period way:** the operator adds the terminal, then
  the headend sends the sequence — factory-init → unit-creation → connect
  (manual-confirmed order). No over-the-air auto-enrollment, exactly like
  the original; the unit DB *is* the enrollment mechanism.
- **Entitlements:** service/program/package authorization, the single
  shared IPPV impulse tier, per-terminal impulse credit and purchase
  limits, Clear PIN (Event / Parental Control / All).
- **Scheduler:** the DAC's Define Operations model — bulk operations
  (All Terminals / ID range / single × RADD, plant, type filters),
  ≥250 ms inter-terminal delay, repeat-every scheduling, auto-generated
  operations (Global Auth Refresh, Channel Map, **Insert System Time**,
  Compute DST Interval, Code Download, **PPV Window Auth**), and the full
  New → Request Sent → … → Completed/Failed lifecycle.
- **WireLink-verb admin API** (tokio + axum, utoipa → OpenAPI with CI
  drift checks) — the only interface the Console ever talks to.
- **Business-system endpoint** (S4, proposed): a WireLink 600/700-series
  BSI server so the Console can play business system — PPV program
  downloads with BSG handles, call-ahead authorizations, impulse purchase
  uploads, orphan-purchase handling.

### Transport — `oob-mux`

- MPEG-TS writer (188-byte packets, asynchronous, null-stuffed) feeding
  the OOB QPSK stream at **1.024 Msym/s**; the flowgraph owns timing.
- **DCII-flavored PSI/SI** (PAT/PMT/CAT/NIT + CA descriptors with
  GI/Motorola CA-system IDs) — not vanilla DVB tables; validated box-side
  via the d 15/d 16 diagnostic dumps.
- PID plan and a ~1000 Kbps **insertion-bandwidth shaper**
  (criterion-benched).
- **Ten-carrier hunt table** in the box's fixed scroll order
  (75.25 → 104.20 → 72.75 → … MHz) with the acceptance-critical
  **EMM provider IDs 0x0000/0x0001** from day one.

### Protocol — `oob-msg`

- The GI out-of-band message layer as code — the crate *is* the O4
  protocol document.
- Every message family carries a status — `documented` (manual-sourced),
  `hypothesized` (constructed, unverified), `verified` (box-accepted,
  capture-referenced) — promoted only on capture evidence.
- Catalog seeded from the DAC §11 operations catalog and the install
  manual's E-code remedies: connect, VCM, region, inband-processor,
  general, multicast-16, phone, the poll family, disconnect, code-download
  control, system time.

### Code download — `oob-cdl`

- DLS `.obj` / `.dat` loader and carousel builder; download control riding
  the EMM stream.
- **Platform → object compatibility** keyed on census Platform/Family/
  Model IDs — DCT2000 objects are 68k, DCT2500 are Broadcom/MIPS, and
  donors never cross platforms.
- Donor-object re-serving (the guide app is the first customer) and the
  **modified-object round trip**: patch a string resource, download,
  capture the changed behavior — mutation as proof of understanding.
- Gated on D0: two validated flash recovery routes (TL866 + 68k BDM)
  before any download enable.

### Playout — `oob-playout`

- Clear-tier **MPEG-2 MP@ML 480i** playout (ffmpeg / tsduck) over UDP/GigE
  into the SEM V8's Annex B QAMs; CBR per J.83 Annex B with tsp policing.
- Audio: MPEG-1 Layer II (Musicam) primary; AC-3 only where the census
  confirms decoder support. Never anything newer — a box that can't decode
  masquerades as a tuning failure.
- Self-authored content only, ever; barker/EPG channels and the PPV
  ordering-instruction slate as first-class services.
- NVOD stagger-set generator (S4, proposed): N offset instances of one
  feature, each a scheduled PPV service with its own epochs.

### Guide — `oob-guide` (VueGuide+)

- **Residency-first:** census the fleet for the resident guide app
  (expected i-Guide), re-serve a donor object to the fleet over `oob-cdl`,
  then synthesize the listings feed in whatever format the resident app
  parses — discovered box-as-oracle, with the d 14 message-type counters
  as the transport oracle.
- Listings synthesizer: schedule DB (W4 amendment; seeded from `lineup.yaml`) → feed; per-program
  attributes evidenced by the period frame corpus (genre colors,
  ratings, cast synopses, PPV price/`$` badging, preview links).
- The guide feed is an **always-on service** under watchdog; the nightly
  canary asserts grid population, not just authorization.
- Fallbacks in order (risk R12): donor object sourced fleet-externally →
  **headend-rendered Prevue-style video guide channel** → native
  substitute grid (explicitly degraded, optional), themed
  **Navigator/Passport** per the operator's preference.

### Operator UI — Oob Console

- Web front-end, API-first: generated types from `openapi.json`, no logic
  the admin API doesn't expose.
- Fleet registry (the Define Terminal schema), ops views mirroring the
  DAC's operation lifecycle, program schedule editor (over the schedule DB, W4 amendment), purchase ledger and
  orphan-purchase queue, per-terminal credit display, Clear PIN op.
- **Born as the experiment workbench** — Console phase C0 is a G1 exit
  requirement, because the discovery gate's evidence pack demands it.
- Six operator journeys, all passing without SSH by G6; a museum-mode
  reader against the versioned mdBook.
- **Billing & subscriber management** (S5, proposed): accounts, rate
  cards, an immutable ledger, period-styled fictitious statements,
  fictitious payments, and non-pay disconnect/reconnect that drives real
  DAC entitlements — fronted by a fully documented Billing API (OpenAPI
  3.1, CI drift-checked). The theater layer: the subscribers are friends
  and the monthly bill is half the fun.

### Return path (stretch S1)

- `oob-starfone`: STARFONE telephone return via an mgetty modem bank;
  STARVUE II upstream demod on an RX SDR (5–42 MHz).
- The poll family — Purchase Poll, Diagnostic Poll, Chronic Non-Responding
  Poll — closing the loop on impulse purchases and fleet health, with the
  box's d 12 sign-on state machine and d 13 MAC tables as observables.

### Lab harness

- **E1 capture rig:** camera OCR of the 7-segment front panel,
  capture-card diagnostic-screen grabs, DCT2500 RS-232 console scrape —
  the discovery loop's eyes, zero human interaction per iteration.
- Every experiment is a commit: message template hash → box response →
  verdict. Nightly hardware canary post-M1 (reboot → assert `E 00` →
  scrape d 02/d 15) guards passed gates.

## Roadmap

Every milestone ends at a **gate** — an evidence pack plus an
owner-dispositioned review. Work does not "finish," it *passes*.

| Gate | Milestone | Goal | Review |
| --- | --- | --- | --- |
| **G0** | M0 — Foundations | Documentation base built; manuals mined; plan + ADR current | checklist (retroactive) |
| **G0.5** | M0.5 — PHY ready | OOB transport understood and reproducible on the bench | checklist |
| **G1** | M1 — First connect | A box enrolls and reaches `E 00` from our stream alone | **major (adversarial)** |
| **G2** | M2 — Clear tier | Boxes tune and decode live services from the SEM | checklist |
| **G3** | M3 — FPK tier | The encrypted tier decodes fleet-wide via the SEM ACP | **major (adversarial)** |
| **G4** | M4 — Code download | A download campaign lands, mutates, and rolls back safely | checklist + drills |
| **G5** | M5 — Return path (S1) | A box answers a poll end-to-end | checklist (stretch) |
| **G6** | M6 — Period experience | The 2001 living-room: guide populated, ops polished, canary green | **major (adversarial)** |

Enabling tracks feeding the gates: **E1** capture rig → G1 entry; **E2**
D0 flash RE → G4 entry; **E3** fleet census → G2 exit and the G6 fidelity
call; **E4** SEM bench → G2/G3 entry.

Registered stretch goals (no planning weight until scoped): **S1** return
path (above), **S2** Interconnect — remote service delivery to hub sites
over the internet (post-G6), **S3** analog tier (unscoped), **S4** VOD/PPV
simulation — scheduled PPV + call-ahead, impulse PPV, NVOD; true session
VOD registered but unscoped, **S5** billing & subscriber management —
accounts, statements, fictitious payments, and dunning-driven disconnect,
behind a documented Billing API.

Releases are tagged `rel/<carrier>` after the hunt-table carriers in box
scroll order: `rel/75.25` → `rel/104.20` → `rel/72.75` → …

## Progress (living status)

*Updated **2026-11-22**. This section is maintained under the same-commit
rule: progress lands here in the commit that makes it, and gate rule 9
means no gate passes with it stale. The authoritative log is the gate
register in `docs/Project Milestones & Stage Gates — mini-headend.md` §7.*

**Done**

- **G0 passed** (retroactive): the full documentation base — plan, ADR-001
  lineage, operations reference, gates doc, Console spec, Interconnect
  spec, Sci-Atlanta spec, VOD/PPV spec, HW list, prior-art scan, five
  adversarial reviews.
- Three period manuals mined in full: **DAC 6000 V3.1** (648 pp), **SEM V8
  SW 7.1.x** (295 pp), **DCT2000 Installation Manual** (Dec 2001, 86 pp) —
  banked discoveries include the ten-carrier OOB hunt table, the EMM
  provider-ID acceptance constraint, the E-code recovery semantics, the
  fleet-wide d 14 protocol sniffer, the d 05 addressing taxonomy, and the
  DAC's complete enrollment/terminal-database model.
- **Stage-gate system adopted** (G0–G6, rules 1–9, tracks E1–E4);
  documentation declared a build artifact (mdBook in CI, gate rule 9).
- **Adversarial reviews r3 (×11), r4 (×8), r5 (×14) dispositioned — all
  findings accepted**, deltas folded across ten corpus files.
- **Software architecture review r6 (×16) dispositioned — all accepted**;
  ADR-002 (runtime architecture) accepted; ADR-003 (the amended F6-4)
  makes Oob an operator system users configure themselves at runtime,
  with sample content, community packs and firmware import tooling.
- **SEM V8 in the estate**, ACP (MediaCipher FPK, DES or CSA) confirmed by
  part number — the project's two largest risks (QAM, encryption) collapsed
  into one period-correct appliance.
- Prior-art scan complete: no public SCTE 55-1 Mode A headend reproduction
  exists; the gap this project fills is documented.
- **Guide reference corpus built:** 11 curated period references,
  including a 13-frame i-Guide analysis, Passport Echo grids from two
  operators, the PTCI guide-era PPV purchase flow end-to-end, and six
  frames from the 1999 TV Guide Interactive instructional tape (three
  guide palettes documented; three period PPV discovery surfaces).
- **VOD/PPV simulation specified** (S4): scheduled PPV, impulse PPV with
  orphan semantics, NVOD, Console-as-business-system — gate-text
  amendments proposed, awaiting disposition.
- **Billing & subscriber management specified** (S5): accounts, rate
  cards, immutable ledger, period-styled fictitious statements,
  dunning-driven disconnect/reconnect coupled to real DAC entitlements,
  fully documented Billing API — gate-text amendments proposed, awaiting
  disposition.

**In flight — gate G0.5 (PHY ready)**

- Buy list items 1–2 (OOB TX from the supported set, primary chosen at the G0.5 bench; F3-7 RF chain) → flowgraph
  loopback BER test → metered carrier through the chain into a dummy load.
- O7: SCTE 55-1 acquisition (two-week timebox; box-as-oracle PHY fallback
  defined).
- Pre-exit engineering task: mdBook scaffold + CI link-check + OpenAPI
  drift stub (F5-10), plus the prose-vs-`facts.yaml` check (F6-13) —
  built (`book.toml`, generated `docs/SUMMARY.md`, `ops/docs-*.py`, three
  CI jobs); the G0.5 evidence is their green doc-site CI run.

**Next after G0.5**

- **E3 fleet census** (box variants, firmware, NVRAM battery state,
  platform/module IDs, guide residency; CPSC 02-227 cord check).
- **E4 SEM bench bring-up** (first-contact archive, FPK license check,
  GigE → RF loop), **E1 capture rig**, **E2 D0 flash routes**.
- G1: the first connect-message experiments against the sacrificial box.

**Awaiting Marshal (manual actions, no agent can do these)**

ADR-001 approval (rev 8) · `gh` `project` scope for the board (oob-pm then
builds board + labels) ·
git-crypt init + key escrow (G1 entry requirement) · buy list purchases ·
O7 / O15 / O16 restricted-document acquisitions (upload pattern) · VOD/PPV
§7 and billing §7 gate-proposal dispositions · repository setting
"require approval for workflows from outside collaborators" (r6 F6-8) ·
add the F5-10 CI jobs `mdbook (link check)`, `docs facts` and
`openapi drift` to the `main` ruleset's required checks once they have
reported green (AGENTS.md §6 rule 6).

## How we work

1. **The boxes are the referee** — a claim without a capture is not
   evidence; fleet diagnostics outrank assumptions.
2. **The census defines "correct"** — the fleet's actual hardware and
   resident apps decide what the headend must speak.
3. **Period-authentic means period-authentic** — MPEG-2 MP@ML only;
   68k-era redraw lag is a feature; degradation modes are reproduced, not
   papered over.
4. **Closed plant, always** — coax, taps, dummy loads; nothing radiates,
   nothing touches outside plant.
5. **Self-authored content** — no off-air redistribution, ever.
6. **Every experiment is a commit** — the protocol document grows from
   reproducible evidence.
7. **Software first; surplus is opportunism** — no milestone depends on
   eBay.
8. **Documentation is a build artifact** — change and docs land in the
   same commit; mdBook builds in CI; no gate passes with stale docs.

Process details live in [`AGENTS.md`](AGENTS.md) (agent working rules) and
the Milestones & Stage Gates doc (the operating contract).

## Repository layout

```
crates/      oob-dac, oob-mux, oob-msg, oob-cdl, oob-playout,
             oob-starfone, oob-guide (Rust workspace)
flowgraphs/  GNU Radio OOB TX / STARVUE RX — versioned here
lab/         harness/ discovery runner + captures/ box evidence
             (git-crypt tier)
ops/         lineup.yaml, units.yaml (seed data + box-type catalog,
             schema-validated; ADR-003), CI
             (nightly canary), agent-guard.py (agent command guard),
             check-agent-hooks.py (CI: every agent hook wired and blocking)
docs/        the mdBook source — specs, protocol, runbook, reviews
.github/     agent role files, PR template, workflows
.claude/     Claude Code subagent launchers, one per role, and
             settings.json (runs the agent guard on every shell call)
.cursor/ .gemini/ .codex/ .github/hooks/
             the same agent guard for Cursor, Gemini CLI, Codex, Copilot
             (Kimi Code users: uv run --python 3.12 python3
             ops/install-agent-guards.py)
```

## Documentation

Everything human-readable lives in [`docs/`](docs/) and builds into an
mdBook site in CI (broken links and anchors fail the build; prose is
checked against `docs/facts.yaml`). Build it locally with
`mdbook build` (output in `target/book/html`); the table of contents is
generated from the Overview's document map by `ops/docs-summary.py`, so
add a document there, not in `docs/SUMMARY.md`. Start here:

| Start with | What it is |
| --- | --- |
| [`docs/mini-headend Project Overview — Faithful Lab Reproduction.md`](docs/mini-headend%20Project%20Overview%20—%20Faithful%20Lab%20Reproduction.md) | Orientation — ground rules, topology, document map |
| [`docs/Project Milestones & Stage Gates — mini-headend.md`](docs/Project%20Milestones%20&%20Stage%20Gates%20—%20mini-headend.md) | The operating contract: gates G0–G6, tracks E1–E4, tool stack, gate register |
| [`docs/Oob Reproduction Plan — mini-headend.md`](docs/Oob%20Reproduction%20Plan%20—%20mini-headend.md) | Workstreams W1–W8, discovery methodology, tech stack, the guide frame-analysis corpus |
| [`docs/DAC-6000 Operations Reference — Oob Design Basis.md`](docs/DAC-6000%20Operations%20Reference%20—%20Oob%20Design%20Basis.md) | Everything the DAC manual teaches, as design input |
| [`docs/VOD & PPV Simulation Specification — mini-headend.md`](docs/VOD%20&%20PPV%20Simulation%20Specification%20—%20mini-headend.md) | The transactional layer (S4): PPV, IPPV, NVOD, VOD stretch |
| [`docs/Billing & Subscriber Management Specification — mini-headend.md`](docs/Billing%20&%20Subscriber%20Management%20Specification%20—%20mini-headend.md) | The theater layer (S5): accounts, statements, fictitious payments, dunning, and the Billing API |
| [`docs/Management Console Specification — Oob Console.md`](docs/Management%20Console%20Specification%20—%20Oob%20Console.md) | The operator front-end, API-first, phases C0–C5 |
| [`docs/Remote Service Delivery Specification — Interconnect (Stretch Goal).md`](docs/Remote%20Service%20Delivery%20Specification%20—%20Interconnect%20(Stretch%20Goal).md) | S2: boxes at remote sites off this headend |
| [`docs/Sci-Atlanta Ecosystem Specification — Explorer, DNCS, SCTE 55-2.md`](docs/Sci-Atlanta%20Ecosystem%20Specification%20—%20Explorer,%20DNCS,%20SCTE%2055-2.md) | The rival ecosystem (theming source; far-future second front) |
| [`docs/Hardware Acquisition List — mini-headend.md`](docs/Hardware%20Acquisition%20List%20—%20mini-headend.md) | The prioritized buy list and what each item unblocks |
| [`docs/Prior Art & External References — mini-headend.md`](docs/Prior%20Art%20&%20External%20References%20—%20mini-headend.md) | The prior-art scan and the gap this project fills |
| [`docs/ADR-001 — Emulated GI Cable Headend for DCT2000 Revival.md`](docs/ADR-001%20—%20Emulated%20GI%20Cable%20Headend%20for%20DCT2000%20Revival.md) + adversarial reviews r1–r5 | The decision record and its review lineage |
| [`docs/ADR-002 — Oob Runtime Architecture.md`](docs/ADR-002%20—%20Oob%20Runtime%20Architecture.md) + [adversarial review r6](docs/Reproduction%20Plan%20—%20Adversarial%20Review%20r6.md) | Runtime shape: one control-plane daemon, pull-driven mux ↔ PHY seam, traffic classes (accepted) |
| [`docs/ADR-003 — Configuration, Extensibility and Distribution Model.md`](docs/ADR-003%20—%20Configuration,%20Extensibility%20and%20Distribution%20Model.md) | How Oob is configured and distributed: runtime-configurable by its users, community packs, firmware import tooling (accepted) |
| [`AGENTS.md`](AGENTS.md) | Working rules for AI agents in this repo |

## Safety and intent

Closed plant only — coax, taps, and terminators on a bench; several OOB
carriers sit inside the FM broadcast band, so shielding and termination
are policy, not preference. Self-authored content only. Self-owned
hardware only. Nothing here touches a live cable network, a licensed
service, or anyone else's content — the subscribers are boxes that would
otherwise be landfill, and the output is working history plus a public
protocol document, so the next person who finds a DCT2000 at a thrift
store can bring it back to life too.

License: GPLv3 (see [LICENSE](LICENSE)).
