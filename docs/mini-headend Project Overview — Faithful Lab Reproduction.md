# mini-headend — Project Overview

**Faithful lab reproduction of a GI/Motorola cable television headend, for home-lab use**

- **Date:** 2026-11-22 (status section updated 2026-11-24)
- **Status:** Living document — orientation layer over the plan, ADR, reviews, and specs
- **Owner:** Marshal (@Marshal)

---

## 1. What this is

**mini-headend** is a preservation-grade functional reproduction of the
decommissioned General Instrument / Motorola cable TV headend ecosystem —
the DAC 6000 controller, OM 1000/2000 out-of-band modulator, DLS code
download system, and their companion services — operated as a **closed
home-lab RF plant**. Its purpose is to return legacy DCT2000-series digital
cable boxes to full working operation: boot, authorization, channel map,
clear and encrypted tiers, firmware/application download, and the resident
interactive program guide, all behaving the way they did when these systems
were in service.

The boxes this project serves are real, unmodified consumer hardware —
e-waste feedstock to the rest of the world, historically significant
artifacts here. They were designed to be helpless without a headend: no
connect message, no channel map, no guide, eventually an `E 01` on the
front panel forever. mini-headend rebuilds the other half of that
relationship, faithfully enough that the boxes cannot tell the difference.

This is reproduction work in the same spirit as vintage computer
restoration or period telephone exchange revival: the goal is authentic
*behavior* of the original system, verified against original documentation
and the original endpoints — not a modern re-imagining.

**Naming (adopted 2026-11-22; revised 2026-11-22).** The software is
**Oob** — OOB, *Out-of-Band*: *an out-of-band headend for DCT-class cable
boxes — an MVPD of one*. Full project identity: **Oob — An
AncientEnterprise Venture** (AncientEnterprise is Marshal's lab-project
umbrella; sibling projects carry their own names). mini-headend stays as
the lab/estate nickname. The parts:

| Part | Name |
| --- | --- |
| DAC reproduction — unit DB, entitlements, scheduler, WireLink-verb API | `oob-dac` |
| OOB transport — TS writer, PSI/SI, insertion-bandwidth shaper | `oob-mux` |
| GI message layer — the protocol document as code | `oob-msg` |
| Code download (DLS) | `oob-cdl` |
| Clear-tier playout | `oob-playout` |
| STARFONE modem-bank bridge (stretch) | `oob-starfone` |
| **Guide stack — listings synthesizer, guide feed, R12 fallback grid** | **VueGuide+** (`oob-guide`) |
| Operator front-end | **Oob Console** |

Releases are tagged after the OOB hunt-table carriers in box scroll order
(`rel/75.25` → `rel/104.20` → `rel/72.75` → …).

**Operator-agnostic by design (Marshal's direction, 2026-11-22).** Oob is
not oriented to any telco or provider, fictional or otherwise: the guide
and Console brand skin is a **per-deployment config slot** — no preset
operator identity ships, and each deployment fills the slot with its own.
The project is built so others can stand up their own plants, interconnect
them (the S2 Interconnect), share configs, and transfer subscribers between
systems. (This supersedes the ArchaiTel Digital Cable placeholder skin.)

## 2. Ground rules (what "faithful" means here)

1. **The boxes are the referee.** Every behavior is validated against real
   fleet hardware — front-panel codes, diagnostic screens, and observed
   state changes — not against what we assume the protocol ought to be.
2. **The census defines "correct."** The fleet's actual hardware, firmware,
   and resident applications (recorded per box at census) decide what the
   headend must speak and which guide experience is the fidelity target.
3. **Period-authentic means period-authentic.** MPEG-2 MP@ML only (these
   boxes decode nothing newer); 68k-era guide redraw lag is a feature to
   preserve, not a bug to fix; degradation modes behave the way the
   originals did.
4. **Closed plant, always.** All RF lives in coax, terminated into dummy
   loads and taps on the bench. Nothing radiates; nothing touches outside
   plant. Several of our operating frequencies sit inside the FM broadcast
   band — shielding and termination are policy, not preference.
5. **Self-authored content.** Playout is our own generated loops and
   authored listings. No off-air re-distribution, ever.
6. **Every experiment is a commit.** Discovery work is logged with message
   templates, box responses, and verdicts, so the protocol documentation
   grows from reproducible evidence.
7. **Software first; surplus is opportunism.** The reproduction is the
   path. Period hardware joins the estate when it appears (and it has —
   see the SEM below), but no milestone may depend on eBay.
8. **Documentation is a build artifact.** Heavy human-readable docs ship
   *with* the software from day one: the mdBook site builds in CI on every
   commit (broken links fail the build), every crate carries rustdoc, the
   API spec is drift-checked, and no stage gate passes with stale docs
   (gate rule 9).

## 3. What is being reproduced

| Original system | Role in the original network | mini-headend realization |
| --- | --- | --- |
| **DAC 6000** (Digital Addressable Controller) | System brain: unit database, entitlements, message scheduler, operator interface | `oob-dac` — Rust daemon, sqlite unit DB, WireLink-verb admin API |
| **OM 1000/2000** (OOB Modulator) | Generates the out-of-band control channel (QPSK, 1.024 Msym/s, 70–130 MHz) | GNU Radio flowgraph → SDR from the supported set (ADALM-Pluto, HackRF One clone, LibreSDR; primary chosen at the G0.5 bench, Marshal 2026-11-23) |
| **KLS/DLS** (Code Download System) | Delivers firmware and applications to boxes as 68k objects over the OOB stream | `oob-cdl` crate — .obj/.dat loader + carousel builder |
| **SEM V8** (SmartStream Encryptor Modulator) | Edge QAM + MediaCipher encryption | **Period hardware in the estate** — Motorola SEM V8, P/N 507199-001-00, ACP confirmed |
| **STARVUE II** | Upstream return-path receiver (5–42 MHz) | SDR receive flowgraph (stretch) |
| **STARFONE** | Telephone return path for purchases/polls | VoIP-modem bridge to an mgetty bank (stretch) |
| **EPG inserter + TV Guide Interactive (i-Guide)** | Resident guide app fed listings over OOB | W8 / **VueGuide+**: donor app re-served over our own download system + synthesized listings feed |
| **DAC operator terminals + WireLink interface** | Day-to-day operations: units, messages, lineups, downloads | **Oob Console** — web front-end over the WireLink-verb API (see Console spec) |

Everything not marked "period hardware" is software we write, deployed as
Docker/Podman containers on a Debian 13 host. The one deliberate hardware
exception — the SEM — collapsed our two largest risks (QAM modulation and
MediaCipher FPK encryption) into a single period-correct appliance that was
already in the estate.

## 4. Lab topology

```javascript
┌─────────────────────────────────────────────────────────────────┐
│  Debian 13 host (Docker/Podman)                                 │
│                                                                 │
│  oob-dac ── oob-mux ── oob-msg ── oob-cdl ── oob-playout        │
│     │            │                        │                     │
│     │            ▼                        ▼ UDP/GigE            │
│     │      GNU Radio flowgraph     ┌──────────────┐             │
│     │            │                 │ SEM V8 (ACP) │             │
│     │            ▼                 │ FPK: DES/CSA │             │
│     │      OOB SDR (set)           └──────┬───────┘             │
│     │            │ QPSK ~104 MHz          │ QAM Annex B         │
│     └────────────┼────────────────────────┼─────────────────────┘
│                  ▼                        ▼                     │
│               combiner ──► directional tap ──► plant            │
│                              (dummy load first)                 │
│                                                                 │
│  Capture rig: camera→OCR (front panel), capture card (diag      │
│  screens), RS-232 scrape (DCT2500) — the discovery feedback     │
│  loop's eyes.                                                   │
└─────────────────────────────────────────────────────────────────┘
```

The fleet: DCT2000 + DCT2500 boxes (DVR models included), each recorded at
census with its platform/family/model IDs, tuner variant, firmware,
resident modules, serial, and cord date. One DCT2500 is the designated
instrumented unit; a sacrificial DCT2000 fronts the risky experiments.

## 5. How the work is organized

Eight workstreams, W1–W8: PHY/carrier, core+mux, message discovery lab,
clear-tier playout, FPK tier, code download, return path + polish, and the
guide. Discovery runs **box-as-oracle**: the boxes *want* to connect and
their diagnostics report exactly how far each attempt got, so candidate
message formats iterate against a cooperative referee with an OCR-rigged
camera watching its front panel.

Key discoveries already banked (from the DCT2000 Installation Manual, in
hand): the full ten-carrier OOB hunt table in fixed scroll order; the
EMM provider-ID constraint (0x0000/0x0001) that gates stream acceptance;
the manual carrier-select diagnostic that serves as our acquisition test
harness; the official E-code table and its manufacturer-documented
remedies; and — best of all — a built-in fleet-wide protocol sniffer
(d 14 Message Types) that counts PIDs and message IDs on both the OOB and
in-band sides.

Milestones: **M0** documentation base (done) → **M0.5** PHY + SCTE 55-1 →
**M1** connect (box reaches `E 00`) → **M2** clear tier → **M3** FPK
encrypted tier (via the SEM's ACP) → **M4** code download → **M5**
return-path stretch → **M6** polish, including the resident guide grid
populating from our own listings feed.

## 6. The guide question (a fidelity chapter of its own)

The interactive guide is box-side software historically delivered by the
headend's code-download system and fed listings over the control channel.
Our strategy is residency-first: census the fleet for the guide application
(i-Guide / TV Guide Interactive), re-serve a donor object to the rest of
the fleet through our own download infrastructure — a legitimate first use
of it — and then synthesize the listings feed in whatever format the
resident app parses, discovered through the box-as-oracle loop plus the
Gemstar/StarSight patent record.

Fidelity references are curated period footage and screenshots (13-frame
i-Guide analysis and a Passport Echo frame from Cincinnati Bell are already
logged): genre-colored grid cells, surf banners, quick menus, favorite
lists, the works. If no fleet box carries any guide application, the
fallback is a substitute grid themed after **Navigator/Passport** — the
interfaces the operator remembers most — with a headend-rendered
Prevue-style video guide channel as the zero-box-side-code option.

## 7. Stretch goals (registered, post-M6)

| # | Stretch goal | Document |
| --- | --- | --- |
| S1 | **Return path** — STARVUE II upstream receive + STARFONE modem-bank bridge, enabling purchases/polls | Reproduction Plan W7 |
| S2 | **Interconnect — remote service delivery.** Operate boxes at remote sites over the internet off this headend, via small hub kits that regenerate RF locally. The modern stand-in for a master-headend-plus-hubs plant. | `Remote Service Delivery Specification — Interconnect (Stretch Goal).md` |
| S3 | **Analog tier** — the platform's period analog descrambling modes (gated sync suppression, video inversion, Hamlin compatibility) | noted in Plan §3.1; out of current scope |
| S4 | **VOD/PPV simulation** — scheduled PPV + call-ahead (P1), impulse PPV (P2, rides S1's return path), NVOD (P3); true session VOD registered but unscoped | `VOD & PPV Simulation Specification — mini-headend.md` |
| S5 | **Billing & subscriber management** — accounts, rate cards, immutable ledger, period-styled (fictitious) statements, fictitious payments, non-pay disconnect/reconnect driving DAC entitlements; fully documented Billing API. The theater layer: the subscribers are friends and the monthly bill is half the fun | `Billing & Subscriber Management Specification — mini-headend.md` |

### 7.1 Expansion lanes

*Recorded from the strategy session 2026-11-24 (Marshal). This is a planning
construct. It orders candidate lanes and admits none of them to a gate. It
does not disposition G0–G6.*

Lanes are sorted by one question: was the set-top's trust rooted in a local
headend (reproducible on a closed plant), or in a third-party service (not
reproducible)?

| Order | Lane | Trust root | Planning status |
| --- | --- | --- | --- |
| 1 | **SciAtl cable** | Local headend: DBDS/DNCS with PowerKEY. Explorer 2000, 3000 and 8000 set-tops. The DAVIC-derived Mode B PHY is covered by the Sci-Atlanta spec (§3). | Sequenced first |
| 2 | **Satellite DCII** | Local headend: DCII, with DSR-4500-class IRDs and 4DTV receivers | Proposed to share the SciAtl CA workstream; the shared-lineage basis is open (see Sourcing) |
| 3 | **IP STBs** | Third-party service | Per-box opportunistic rescue only. Zero gates, zero planning weight. |
| 4 | **CableCARD** | Blocked at one anchor: the card factory CA and the PowerKEY/MediaCipher issuer hierarchy | Zero gates until the revival trigger below |

- **SciAtl cable.** Sequenced first. Its CA workstream is the one the
  satellite lane reuses.
- **Satellite DCII.** The strategy session proposed one CA workstream for
  both SciAtl and satellite. The shared-lineage basis is not relied on here;
  see Sourcing.
  Surplus headend hardware gives richer captures. An IRD can serve as a
  headend element: demodulator, then ASI, then mux, then QAM. The L-band never
  touches coax. If a physical satellite side is wanted, the LNB is its own
  L-band side plant. Shaw Direct runs DCII today and is a living reference.
  VideoCipher II is deprioritized. Attacks on live services are excluded by
  doctrine (AGENTS.md §7 and §11).
- **IP STBs.** Opportunistic per-box rescue only. This follows AGENTS.md §11:
  surplus finds carry no planning weight.
- **CableCARD.** The host side is reproducible: two commercial
  implementations exist, SiliconDust HDHomeRun Prime and Ceton InfiniTV
  (including SDV). The blocked anchor is the card's trust chain. Revival
  trigger: acquire a CableLabs test-CA or development M-CARD. Any such key
  material is handled under the secrets tier (AGENTS.md §5). Cheap referee: a
  used HDHomeRun Prime, about $30, as a certified host for observing the
  emulated DNCS. That purchase is Marshal's decision; nothing here orders it.

**Sourcing.** The DAVIC-derived SCTE 55-2 Mode B PHY is covered by the
Sci-Atlanta Ecosystem Specification (§3); DAVIC 1.x as a whole is not. The
Explorer 2000, 3000 and 8000 model list, Shaw Direct's DCII service, the
HDHomeRun Prime and Ceton InfiniTV host claims and the approximate $30 price
are strategy-session input and are not yet sourced to a document. The
shared PowerKEY lineage asserted for satellite DCII is contradicted by the
Sci-Atlanta Ecosystem Specification §1 table (PowerKEY on the Scientific-Atlanta
side, MediaCipher on the GI/Motorola side); it is not relied on here and is for
Marshal to resolve. The
standing searches in Prior Art §6.1 are where they get sourced.

**Lane entry step (planning construct; not a gate).** Each lane has its own
entry step, called "lane entry" here: donor box and docs acquired, plus a
first capture. The stale-docs principle of AGENTS.md §1, rule 9 is applied to
lane docs by analogy. It is not a gate rule and binds no gate. The gate
sequence G0–G6 is unchanged and still owner-dispositioned.

**Open tension (flagged for Marshal, not resolved here).** The document map
row for the Sci-Atlanta specification, Prior Art §6 and the Hardware
Acquisition List (item E) call the second front "far-future". This lane order
sequences SciAtl first. The Sci-Atlanta Ecosystem Specification's own timing is
the source to check against this order. Both statements stand until Marshal
decides which governs.

## 8. Where everything lives (document map)

| Document | What it is |
| --- | --- |
| **This file** | Orientation — start here |
| `ADR-001 — Emulated GI Cable Headend for DCT2000 Revival.md` (rev 8) | The decision record: scope, goals, kill criteria — awaits final approval |
| `Oob Reproduction Plan — mini-headend.md` | The working plan: discovery methodology, workstreams, milestones, tech stack (§8), manual findings (§3.1/§3.2) — *renamed from "rust-dac Reproduction Plan" at the Oob naming adoption* |
| `DAC-6000 Operations Reference — Oob Design Basis.md` | Everything the DAC manual teaches, organized as design input |
| `Sci-Atlanta Ecosystem Specification — Explorer, DNCS, SCTE 55-2.md` | Companion spec for the rival ecosystem (theming source; possible far-future second front) |
| `Remote Service Delivery Specification — Interconnect (Stretch Goal).md` | The S2 remote-delivery design |
| `VOD & PPV Simulation Specification — mini-headend.md` | The S4 transactional layer: scheduled PPV + call-ahead, impulse PPV, NVOD, and the registered true-VOD stretch (proposed, awaiting disposition) |
| `Billing & Subscriber Management Specification — mini-headend.md` | The S5 theater layer: accounts, rate cards, immutable ledger, period-styled fictitious statements, dunning-driven disconnect/reconnect, and the documented Billing API (proposed, awaiting disposition) |
| `Management Console Specification — Oob Console.md` | The operator front-end — every setting, template, and campaign, API-first over the WireLink verbs |
| `Project Milestones & Stage Gates — mini-headend.md` | The operating contract: milestone/gate map G0–G6 with entry/exit/evidence rules, enabling tracks, the living gate register, and the full development tool stack |
| `Hardware Acquisition List — mini-headend.md` | The prioritized buy list: what to acquire, in P0→P3 order, and which gate or track each purchase unblocks |
| `Capture Rig Specification — mini-headend.md` | The E1 capture rig (G1 entry): the box's three signal sources, the direct-to-PC capture paths (RCA composite, RS-232, front-panel camera), the d-screen payload map, the OCR pipeline, and the harness contract for `lab/captures/` — proposed 2026-11-23 |
| `Prior Art & External References — mini-headend.md` | The 2026-11-22 prior-art scan: closest relatives (a Mode B OOB decoder; the 33c3 DigiCipher II talk), vendor-side corroboration, box-side hobby docs, adjacent scenes, and the gap this project fills |
| `ADR-002 — Oob Runtime Architecture.md` | Runtime shape: one control-plane daemon, the pull-driven mux ↔ PHY seam with the flowgraph as clock master, traffic classes for the insertion budget, deployment split — **accepted (Marshal, 2026-11-22)**; gates Slice C/D design |
| `ADR-003 — Configuration, Extensibility and Distribution Model.md` | How Oob is configured and distributed: an operator system users run and configure themselves at runtime (no git, no PRs), three layers of change, `user-defined` trust label, community packs, firmware import tooling, server-side guardrails — **accepted (Marshal, 2026-11-22)**; the amended r6 F6-4 |
| `protocol/README.md` | The O4 protocol chapters: message families with documented/hypothesized/verified status and capture backrefs, generated from `oob-msg`, plus the PHY record |
| `architecture/README.md` | The architecture chapter (ADR-002 action 7): runtime topology, PHY seam, traffic classes and configuration layers (#38), plus crate-internal pages that land with their code (the oob-dac store, the oob-dac command layer and the oob-dac policy layer); pages are picked up automatically |
| `runbook/README.md` | The lab runbook: one entry minimum per gate — what was attempted, what broke, lessons learned; entries are picked up automatically |
| `ADR-001-adversarial-review.md`, `-r2.md` | Reviews of the ADR (rounds 1–2) |
| `Reproduction Plan — Adversarial Review r3.md`, `r4.md`, `r5.md` | Reviews of the plan (rounds 3–5; r5 is the full-corpus pass with the dedicated Sci-Atlanta section). **All three dispositioned 2026-11-22 — all findings accepted** |
| `Reproduction Plan — Adversarial Review r6.md` | First software-architecture pass (F6-1…F6-16): process topology, mux ↔ PHY timing, insertion-budget classes, config write paths, data durability, evidence chain, CI/deploy, API safety, contract drift — **dispositioned 2026-11-22: all accepted; F6-4 amended via ADR-003** |

Reviews are part of the process, not an obstacle course: every round so
far has ended with "the approach survives; here are deltas," and the deltas
get folded in at the next revision.

## 9. Status snapshot (2026-11-24; base 2026-11-22, post-dispositions)

- **Progress since 2026-11-22 (status only; gate states are unchanged —
  G0 PASS, G0.5 open):** Slices C and D are under way on the ADR-002
  schedule Marshal ruled on 2026-11-23. Merged to `main`: the foundation
  slice, #14 (PR #42), #21 (PR #43), #15 (PR #47), #22 (PR #51), #16
  (PR #52) and #17 (PR #55). Merge-ready but not yet merged: #23 (PR #57,
  the oob-dac policy layer), with CI still finishing on its head. The
  Slice C/D issue-by-issue tracking is in the Milestones & Stage Gates doc
  §4.1.
- **Follow-ups opened (Backlog):** #53 (replace the linear scheduler
  candidate scan with a heap or index if queue depth grows; deferred until
  a bench trigger fires), #54 (WireLink vocabulary cross-check page against
  the Oracle DAC 6000 cartridge vocabulary), and #56 (research: source the
  GI/Motorola DCII `CA_system_id` for the PSI CA descriptors).
- **WireLink corpus:** a WireLink v3.0 corpus document has been ingested on
  branch `docs/wirelink-v3-corpus`, not yet merged. Open item **O15** (the
  full WireLink 600/700 dialect reference) is therefore partly acquired;
  its §4 and §5 are still missing, and the O15 wording stays open.
- **Next steps:** Slice C — #18 (box-free stream verification with tsduck
  analysis in CI), then #19 and #20; Slice D — #25 (runtime configuration
  store), then #26, #27, #24 and #29.
- Documentation base: **done** (M0). Two manuals mined in full (DAC 6000
  V3.1, SEM V8 7.1.x), DCT2000 Installation Manual in hand and folded in.
- Tech stack: **decided** — Rust workspace, Docker deployment, GNU Radio
  PHY, ffmpeg/tsp playout, SEM for QAM + encryption. Development tool
  stack (build, discovery, capture, RE, CI) documented in the
  Milestones & Stage Gates doc §6.
- Process: **stage gates adopted** — every milestone G0–G6 now has entry
  criteria, exit criteria, an evidence-pack requirement, and a review
  level; G0 recorded as passed; gate register open.
- Naming: **adopted** — the software is **Oob** (OOB — Out-of-Band), the
  guide stack is **VueGuide+**, crates are `oob-*`; docs are a build
  artifact (ground rule 8).
- Prior art: **scanned** — no public attempt at a 55-1 Mode A headend reproduction found; the closest living relative is a receive-only Mode B (55-2) decoder, and the CA deep dive is Gerlinsky's 33c3 talk. Full results and follow-up actions in the Prior Art & External References doc.
- SEM V8: **in the estate**, ACP encryption module confirmed by part
  number; bench bring-up (provisioning, license check, first-contact
  archive of its management payload) is on the two-week list.
- Reviews: **dispositioned** — r3 (×11), r4 (×8), r5 (×14) all accepted
  2026-11-22; deltas folded into the plan, gates doc, AGENTS.md, HW list,
  S-A spec, and Interconnect spec in the disposition commit set. The unit
  DB is declared derived state (F5-12, since amended by F6-5); release
  tags are `rel/<carrier>` (F5-13).
- Software architecture: **decided** — r6 (×16) dispositioned 2026-11-22,
  all accepted; **ADR-002** (runtime topology, pull-driven mux ↔ PHY
  seam, traffic classes) accepted; **ADR-003** accepted as the amended
  F6-4: Oob is an operator system users configure themselves at runtime
  (no git, no PRs), ships self-authored sample content, a community pack
  format and firmware import tooling, and never ships firmware, keys or
  box-identifying data. One sqlite file is the installation, with
  one-click backup/restore (F6-5, amending F5-12). Contract drift swept
  and `docs/facts.yaml` added (F6-13).
- Awaiting (Marshal's manual actions): ADR-001 approval (rev 8); branch
  protection (AGENTS.md §6 checklist); the `gh` `project` scope so oob-pm
  can build the Projects board + labels (`.github/agents/README.md` spec,
  AGENTS.md §14); git-crypt init before G1 (F5-11) —
  needs Marshal's key escrow; the buy list (OOB TX from the supported set,
  primary chosen at the G0.5 bench; RF chain, capture rig, flash tooling, sundries — see the Hardware
  Acquisition List).
- Pre-G0.5-exit tasks: mdBook scaffold + CI link-check + utoipa drift
  stub (F5-10), with the prose-vs-`facts.yaml` check (F6-13) — built
  (CI jobs `mdbook (link check)`, `docs facts`, `openapi drift`, the last
  a stub until Slice D); the 24 h mux ↔ PHY pull loopback (F6-2) remains.
- Awaiting (owner action from r6): repository setting "require approval
  for workflows from outside collaborators" (F6-8).
- Awaiting (owner action from F5-10): add `mdbook (link check)`,
  `docs facts` and `openapi drift` to the `main` ruleset's required
  checks once they have reported green (AGENTS.md §6 rule 6).
- Next physical actions: box census, SEM bench bring-up, PHY bench at a
  high hunt carrier, first connect-message experiment.

## 10. A note on intent

Nothing here touches a live cable network, a licensed service, or anyone
else's content. The plant is coax and terminators on a bench; the
subscribers are boxes that would otherwise be landfill; the programming is
made in-house. The output of the project is working history — plus a
public protocol document, so the next person who finds a DCT2000 at a
thrift store can bring it back to life too.
