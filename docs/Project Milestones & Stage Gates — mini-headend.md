# Project Milestones & Stage Gates — mini-headend

*2026-11-22, rev 1. Companion to the Reproduction Plan (workstreams, risk)
and the Project Overview (orientation). This document is the project's
operating contract: what the milestones are, what it takes to pass each one,
and what tools we use to get there. Marshal asked for three things on
2026-11-22 — dev tool stack in the doc scope, project milestones, stage
gates — and this file is all three.*

## 1. How stage gates work here (the operating agreement)

1. **Every milestone ends at a gate.** A gate is not a date; it is an
   evidence pack plus a review. Work does not "finish," it *passes*.
2. **Gates are owner-dispositioned.** Marshal reviews the evidence pack and
   declares one of three outcomes:
   - **PASS** — exit criteria met, milestone closed.
   - **PASS-WITH-DELTAS** — exit criteria met, but the review produced
     findings; they are logged (F-numbered, like the adversarial reviews)
     and tracked to disposition. The milestone stays closed.
   - **HOLD** — exit criteria not met. The gate review states exactly what
     is missing; work continues against that list. A HOLD on a gate whose
     kill criteria are implicated triggers the kill-criteria check (rule 5).
3. **Entry criteria are enforced.** A workstream does not start a milestone
   whose entry criteria are unmet. Example, already on the books: W6 (code
   download) may not start before D0 flash restore is verified (r3 F3-3).
4. **Review depth scales with the gate.** Major gates (G1, G3, G6) get a
   full written adversarial review in the r1–r4 format. Minor gates get a
   checklist review against this document. Any gate can be escalated to a
   full review if the checklist surfaces something structural.
5. **Kill criteria are re-checked at every gate**, not just where they
   first apply. The current kill criteria (post-r4): FPK failure requires
   *both* no working ACP *and* the O3 extraction routes failing.
6. **The evidence pack is commits.** Every gate tags the repo
   (`gate/g1`, `gate/g2`, …). Box-visible evidence (d-screen scrapes,
   camera OCR, counter logs) is attached as files in `lab/captures/`,
   referenced from the tag message. A claim without a capture is not
   evidence — the box is still the referee.
7. **Tiered exits where defined.** M1 has a core tier and a full tier
   (r3 F3-8); passing core unlocks W4 staging, full closes G1.
8. **Passed gates stay passed.** The nightly canary (once it exists)
   guards regressions; canary red for >3 consecutive nights reopens the
   highest milestone the failure touches, and the fix goes through that
   gate again.
9. **No gate passes with stale docs.** Documentation is a build artifact
   (Marshal's directive, 2026-11-22 — see §8): every gate's evidence pack
   includes the updated human-readable documentation — mdBook build green,
   protocol doc current, runbook entry written. If the docs don't say how
   it works, the milestone isn't done.

## 2. The milestone & gate map

| Gate | Milestone | One-line definition | Review | Status |
| --- | --- | --- | --- | --- |
| **G0** | M0 — Foundations | Documentation base built; manuals mined; plan + ADR current | checklist (retroactive) | **PASS** (historical) |
| **G0.5** | M0.5 — PHY ready | OOB transport understood and reproducible on the bench | checklist | open |
| **G1** | M1 — First connect | A box enrolls and reaches `E 00` from our stream alone | **major (adversarial)** | open |
| **G2** | M2 — Clear tier | Boxes tune and decode live services from the SEM | checklist | open |
| **G3** | M3 — FPK tier | The encrypted tier decodes fleet-wide via the SEM ACP | **major (adversarial)** | open |
| **G4** | M4 — Code download | A download campaign lands and rolls back safely | checklist + drills | open |
| **G5** | M5 — Return path (S1 stretch) | A box answers a poll end-to-end | checklist | open (stretch) |
| **G6** | M6 — Period experience | The 2001 living-room: guide populated, ops polished | **major (adversarial)** | open |

Enabling tracks (§4) have their own mini-gates and feed the entry criteria
above. Console phases C0–C5 and Interconnect phases R0–R5 inherit these
gate rules (§5).

## 3. Gate-by-gate detail

### G0 — Foundations (M0) — PASSED

- **Exit (met):** `docs/protocol/` seeded from the manuals; Reproduction
  Plan current; ADR-001 lineage maintained; estate survey done (SEM in
  hand, fleet identified).
- **Evidence:** the document set in the Overview's document map.
- **Note:** passed retroactively when this gate system was adopted;
  recorded here so the register is complete.

### G0.5 — PHY ready (M0.5)

- **Entry:** buy list items 1–2 in hand (OOB SDR, F3-7 RF chain).
- **Exit:**
  - O7 resolved: SCTE 55-1 acquired, *or* the fallback PHY-oracle decision
    documented with its consequences (r3 F3-1 timebox).
  - GNU Radio flowgraph passes a loopback BER test at 1.024 Msym/s.
  - Chosen hunt-table carrier (104.2/110.25 class) measured through the
    F3-7 chain into a dummy load; level log committed (r4 F4-1 metering).
  - OOB TX committed: HackRF One primary, Pluto spare (r4 F4-1).
- **Evidence:** flowgraph commit + BER log; level log; O7 record in
  `docs/protocol/phy.md`.

### G1 — First connect (M1) — MAJOR GATE

- **Entry:** E1 capture rig operational (§4); G0.5 passed; W2 skeleton
  emits a valid null-stuffed stream.
- **Exit (tiered, r3 F3-8):**
  - *Core tier:* sacrificial box reaches `E 00`; d 02 shows data activity
    and carrier lock; `PID 1 received`; enrollment sequence executed in
    the documented order — factory-init → unit-creation → connect
    (r4 F4-4, manual-confirmed) — with harness commits for each step.
  - *Full tier (closes G1):* VCM, Region, Inband Processor, and General
    messages accepted; d 14 logs our connect type; Insert System Time
    emitted on schedule; Console C0 live as the experiment workbench.
- **Evidence:** capture-rig scrapes (front-panel OCR + d-screens) per
  experiment; the harness commit log; C0 screenshots.
- **Review:** full adversarial review (r-series format) before PASS.

### G2 — Clear tier (M2)

- **Entry:** G1 full tier; E4 SEM bench complete (§4); lineup.yaml +
  playout skeleton.
- **Exit:**
  - SEM emits ≥2 QAM services (Annex B DCII mode; STD/HRC consistent
    across SEM / lineup.yaml / VCM — r4 F4-8) from tsp/ffmpeg playout
    over GigE.
  - ≥1 box tunes and decodes both services; d 15/d 16 show our PAT/PMT;
    DCII PSI/SI conformance row validated (r4 F4-6).
  - Lineup consistency CI green; Insert System Time + Region verified
    box-side (clock set, region fields in diagnostics).
- **Evidence:** meter photos, d-screen scrapes, CI log, SEM-EM config
  export in the first-contact archive.

### G3 — FPK tier (M3) — MAJOR GATE

- **Entry:** G2 passed; F4-2 archive complete (SEM-EM jar + MIBs in git);
  ACP FPK mode verified license-enabled at the bench.
- **Exit:**
  - SEM ACP scrambling in FPK mode on ≥1 program; tier matrix committed.
  - Encrypted services decode on fleet boxes; d 01 auth reason correct on
    the encrypted tier; d 05 Seed Health observed.
  - DES vs CSA decided and recorded (O11: SEM config + box behavior).
  - V1 bench check done: SEM emits encrypted programs on its GigE/ASI
    output (this is also the Interconnect R3 dependency).
- **Evidence:** SEM-EM screenshots, d-screen scrapes, tier matrix commit.
- **Review:** full adversarial review; kill criteria explicitly re-checked
  (this is the gate where they bite).

### G4 — Code download (M4)

- **Entry:** E2 D0 track complete (§4) — dump + verified restore +
  message-ID enum extracted; platform→object compatibility table from the
  census; G1 passed.
- **Exit:**
  - Download campaign to the sacrificial box succeeds: d 10 shows the new
    module set; box boots and runs post-download.
  - Rollback drill: return the box to its pre-campaign object set.
  - Recovery drill: the D0 flash-recovery runbook is exercised once on
    purpose (bricked-on-purpose box recovered).
- **Evidence:** campaign ledger, d 10 scrapes before/after, drill logs.
- **Review:** checklist, but escalates to a full adversarial review if the
  first campaign fails or any box needs the recovery path.

### G5 — Return path (M5, S1 stretch)

- **Entry:** G2 passed; upstream-RX SDR in hand; W7 work scheduled.
- **Exit:** STARVUE demod decodes a real box burst; STARFONE bridge
  completes a modem call; a poll (Purchase or Diagnostic) is answered
  end-to-end and lands in the ledger; d 12 sign-on states and power
  leveling observed box-side.
- **Evidence:** burst captures, modem logs, ledger excerpt, d 12/d 13
  scrapes.
- **Note:** stretch gate — G6 does not depend on it.

### G6 — Period experience complete (M6) — MAJOR GATE

- **Entry:** G3 passed; G4 passed; W8 guide work done or fallback
  dispositioned per R12.
- **Exit:**
  - Guide grid renders on ≥1 box and populates from our synthesized
    listings feed (color-coded cells, day-skip working) — the **VueGuide+**
    pipeline (`oob-guide`), per the O12 residency census outcome and the
    O13 transport/opacity results.
  - EAS demonstrated end-to-end (r3 F3-11).
  - All six Console operator journeys pass without SSH (Console spec §7).
  - Canary green 7 consecutive nights.
  - Document set current: plan, protocol doc, runbook, this register.
- **Evidence:** captures of the grid, journey walkthrough recordings, CI
  history, the gate tag.
- **Review:** full adversarial review + project retrospective; stretch
  register (S1/S2/S3) re-scored for what comes next.

## 4. Enabling tracks (mini-gates that feed the gates)

| Track | Feeds | Done when |
| --- | --- | --- |
| **E1 — Capture rig** (r3 F3-2) | G1 entry | Camera OCR of the front panel, capture-card d-screen grabs, and DCT2500 RS-232 scrape all producing files into `lab/captures/`; OCR accuracy spot-checked against manual d-screen readings |
| **E2 — D0 flash RE** (r3 F3-3, r4 F4-3) | G4 entry; W8/O13 support | Clean dump via *both* validated routes (TL866 + 68k BDM), verified restore, message-ID enum registry extracted; dumps in git-crypt |
| **E3 — F0 census** (O2/O12) | G2 exit; G6 fidelity call | Every fleet box's d-screen set recorded; platform table built; guide residency known per platform; CPSC 02-227 cord check logged |
| **E4 — SEM bench** (r4 F4-2) | G2 entry; G3 entry (license) | ACP presence + **license state** verified (FPK selectable, not greyed); SEM-EM jar + MIBs archived to git; tsp loop via GigE → RF out confirmed on the meter; W4 fallback re-documented |

## 5. Console & Interconnect gates inherit these rules

- **Console C0–C5** (Console spec §5): each phase closes with its section
  live and its operator journeys demonstrable; C0 is a **G1 exit
  requirement**, not a side project — the Console is born as the
  experiment workbench because G1's evidence pack demands it.
- **Interconnect R0–R5** (Interconnect spec §8): R1 (loopback hub with
  netem) is its own mini-gate with the same evidence rules; R3 requires
  G3's V1 bench check first; R-gates open only after G6.
- **S3 analog tier:** registered stretch, no gate defined until scoped.

## 6. Development tool stack (in doc scope as of this rev)

The §8 runtime stack (Rust workspace, GNU Radio, ffmpeg/tsduck, Podman,
SEM) is what we *ship*; this section is what we *build and discover with*.
Pinned where it matters, FOSS where possible, everything scriptable.

### 6.1 Build toolchain

| Tool | Role | Notes |
| --- | --- | --- |
| rustup + stable Rust | All daemons | Pinned via `rust-toolchain.toml`; cargo, clippy, rustfmt, cargo-deny (advisories/licenses), cargo-nextest, criterion (bit-rate benches for the insertion shaper) |
| Node LTS + pnpm | Console SPA | Vite + TypeScript; ESLint/Prettier; vitest |
| utoipa → `openapi.json` | API contract | Spec drift = CI failure; `openapi-typescript` generates the SPA client types — the API-first rule is enforced by the build, not by habit |
| Python 3.12 + uv | Lab glue | `lab/` scripts; pydantic models for `units.yaml`/`lineup.yaml` — the same schemas CI validates |
| Podman + podman-compose | Deploy/dev parity | Rootless, images pinned by digest; host-net/macvlan for multicast (r4 F4-7) |

### 6.2 Discovery & RF bench

| Tool | Role | Notes |
| --- | --- | --- |
| GNU Radio 3.10 + gr-osmosdr + gr-iio | OOB PHY | Flowgraphs versioned in-repo |
| hackrf host tools | Primary OOB TX / upstream RX | r4 F4-1: HackRF One is primary; Pluto (gr-iio) is spare |
| inspectrum | Capture analysis | Symbol/constellation inspection of recorded baseband |
| tsduck (`tsp`, `tsdump`, tables plugins) | Mux scaffolding + PSI/SI inspection | Also early playout; dvbsnoop as cross-check |
| ffmpeg/ffprobe | Playout + encode lint | MPEG-2 MP@ML only (F3-6) — lint runs in CI and in the Console |
| Wireshark/tshark | GigE + WireLink analysis | SEM control traffic, hub tunnels |

### 6.3 Capture rig (E1)

| Tool | Role | Notes |
| --- | --- | --- |
| OpenCV + Tesseract | Front-panel 7-seg OCR | Whitelist-trained for E-codes/d-screen digits; spot-checked at E1 exit |
| v4l2 + capture card | d-screen OSD grabs | Composite/HDMI card per F3-7 BOM |
| OBS | Session recording | Journey walkthroughs (G6 evidence) |
| picocom + expect scripts | DCT2500 RS-232 console scrape | Fleet's instrumented tier (F3-8) |

### 6.4 Reverse engineering (E2, O13 support)

| Tool | Role | Notes |
| --- | --- | --- |
| Ghidra (m68k) | Firmware RE | DCT2000 = 68k; DCT2500 stays out of scope for object reuse (F4-5) |
| binwalk + capstone + imhex | Triage, disassembly, hex patterns | imhex pattern templates double as the wire-format drafts for O4 |
| minipro CLI (TL866 II+) | Flash dump/restore route 1 | TSOP-48 adapter set |
| 68k BDM (gdb stub) | Flash/CPU route 2 | Two validated routes required at E2 exit (F4-3) |
| sigrok/PulseView + FX2 logic analyzer | Bus probing | When the paper trail runs out |

### 6.5 Process & CI

| Tool | Role | Notes |
| --- | --- | --- |
| git + git-crypt | Versioning + secrets tier | Dumps, keys, and box-identifying material never in plaintext (standing rule) |
| pre-commit | Local gates | fmt, lint, YAML schema validation |
| Containerized CI on the lab host | Continuous verification | cargo test/clippy/fmt, pnpm build, schema validation, lineup-consistency CI, OpenAPI drift check; **nightly hardware canary** post-M1 (reboot box → `E 00` → scrape d 02/d 15) — the regression tripwire behind gate rule 8 |
| ADR + adversarial-review docs | Decision/risk record | The r-series format is the major-gate review format (rule 4) |
| Pandoc + **mdBook (required)** | Doc exports + the doc site | mdBook builds `docs/` into the human-readable doc site on every CI run — broken links fail the build (see §8); `docs/protocol/` stays Markdown-first |

### 6.6 Later-phase additions

WireGuard + `tc`/netem (Interconnect R1 loopback), srt-live-transmit
(R2 video), chrony (host time discipline), GPSDO (only if d 02 ever shows
carrier drift — F4 escalation path, not a default buy).

## 7. Gate register (living log)

| Gate | Date | Outcome | Tag | Findings logged |
| --- | --- | --- | --- | --- |
| G0 | 2026-11-22 (retroactive) | PASS | — | — |
| G0.5 | — | — | — | — |
| G1 | — | — | — | — |
| G2 | — | — | — | — |
| G3 | — | — | — | — |
| G4 | — | — | — | — |
| G5 | — | — | — | — |
| G6 | — | — | — | — |

*Rows are filled at each gate review. Pending dispositions at adoption of
this system: adversarial reviews r3 (11 findings) and r4 (8 findings);
ADR-001 rev 7 approval. The first gate review to convene (G0.5) should
carry those dispositions as its first agenda item.*

## 8. Documentation policy (in scope from build, Marshal's directive 2026-11-22)

Heavy human-readable documentation is built **with** the software from day
one — it is not a writing phase after the coding phase. Concretely:

- **The doc site:** everything under `docs/` builds into an mdBook site on
  every CI run. Broken internal links fail the build. The published
  artifact is attached to every gate tag, so `gate/g3` forever includes
  the docs exactly as they stood when G3 passed.
- **What's in it:** the O4 protocol document (every message family with
  its documented/hypothesized/verified status and evidence backrefs),
  the PHY/stream parameters, the runbook (Marshal-style build log with
  lessons learned — one entry minimum per gate), the SEM-EM operations
  notes from the first-contact archive, the enrollment and addressing
  references (plan §3.1/§3.2 promoted to proper chapters when the repo
  exists), and operator how-tos mirroring the six Console journeys.
- **Generated layers:** `cargo doc` for every crate (warnings denied);
  the utoipa OpenAPI spec published alongside the mdBook (drift check
  already fails CI); `oob-msg`'s message catalog annotations feed the
  protocol chapters so the wire formats documented are the wire formats
  compiled.
- **Console integration:** Oob Console help links into the mdBook,
  versioned with the release — the "museum mode" guest can read the
  whole story of how the headend works without touching anything.
- **Same-commit rule:** a change and its documentation land in the same
  commit. "Docs later" is debt the project does not carry; gate rule 9
  is the enforcement.
