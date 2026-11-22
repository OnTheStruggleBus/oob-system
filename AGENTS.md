# AGENTS.md — Working Rules for AI Agents on Oob

You are an AI agent working in the **Oob** repository: a faithful software
reproduction of a GI/Motorola cable headend (DAC 6000, OM 1000/2000,
KLS/DLS code download, STARVUE II/STARFONE return paths) that brings real
DCT2000-class cable boxes back to life on a closed lab plant. The operator
is **Marshal**. The guide stack is **VueGuide+**; the operator UI is
**Oob Console**; the operator brand skin is **ArchaiTel Digital Cable**.

This file is your operating contract. It encodes project process as
machine-followable rules. When this file and a `docs/` document disagree on
process, this file wins for *how you work*; the docs win for *what the
system is*.

## 0. Read first

Before your first task in a session, read (in `docs/`):

1. `mini-headend Project Overview — Faithful Lab Reproduction.md` — orientation.
2. `Project Milestones & Stage Gates — mini-headend.md` — the operating
   contract: gates G0–G6, rules 1–9, enabling tracks E1–E4, tool stack §6.
3. The doc(s) for your workstream: `Oob Reproduction Plan — mini-headend.md`
   (W1–W8), `Management Console Specification — Oob Console.md` (C0–C5),
   `Remote Service Delivery Specification — Interconnect (Stretch Goal).md`
   (R0–R5, post-M6 only).

Then find your role file in `.github/agents/` (roster in that directory's
README) and follow it — it defines your scope and handoffs within this
contract.

Open findings (r3: F3-1…F3-11, r4: F4-1…F4-8) await Marshal's disposition.
Treat undispositioned findings as **constraints you must not silently
violate**, not as approved work: you may implement a finding's delta, but
say so explicitly in the commit and PR text.

## 1. Stage gates are the process (non-negotiable)

From `Project Milestones & Stage Gates` §1 — all nine rules bind agent work:

1. **Every milestone ends at a gate** — an evidence pack plus a review.
   Work does not "finish," it *passes*. Never declare a milestone complete;
   prepare the evidence pack and stop.
2. **Gates are owner-dispositioned.** Only Marshal declares PASS /
   PASS-WITH-DELTAS / HOLD. Agents never self-pass a gate.
3. **Entry criteria are enforced.** Do not start work whose gate entry
   criteria are unmet (e.g., no code-download work before D0 flash restore
   is verified — r3 F3-3). If blocked, say so and work an unblocked track.
4. **Review depth scales with the gate.** G1/G3/G6 are major gates with a
   full adversarial review in the r-series format (§6 below).
5. **Kill criteria are re-checked at every gate.** Current: FPK failure
   kills the encrypted tier only if *both* the SEM's ACP fails at the bench
   *and* the O3 extraction routes fail; fallback definition of done is then
   M2 + M4 + M5.
6. **The evidence pack is commits.** Gates tag the repo `gate/gN`.
   Box-visible claims need capture files in `lab/captures/` referenced
   from the tag message. **A claim without a capture is not evidence —
   the box is the referee.**
7. **Tiered exits where defined** (M1 core vs full tier).
8. **Passed gates stay passed** — guarded by the nightly canary; >3
   consecutive red nights reopens the highest affected milestone.
9. **No gate passes with stale docs** (see §3).

Current gate state: G0 PASS (retroactive); G0.5 next, first agenda =
r3/r4 dispositions. Do not schedule work past G1 entry criteria until
G0.5 closes.

## 2. Evidence and the box-as-oracle discipline

- The DCT fleet is the conformance instrument. Behavior claims ("the box
  accepts this message") are made only with captures: front-panel OCR,
  d-screen grabs, RS-232 scrapes, or upstream sniffer recordings, committed
  under `lab/captures/`.
- **Every experiment is a commit**: message template hash → box response
  (OSD state + scraped diagnostics) → verdict. The protocol document grows
  from reproducible evidence, never from notes or memory.
- In `oob-msg`, every message family carries a status —
  `documented` (manual-sourced), `hypothesized` (constructed, unverified),
  `verified` (box-accepted, capture-referenced). Do not promote a status
  without the capture. The crate *is* the O4 protocol document; its
  annotations feed the mdBook protocol chapters.
- Discovery order for wire formats (ADR-001 §4): SCTE 55-1 spec →
  structure-guided construction from the DAC §11 semantic payloads →
  patents → firmware-assisted parsing → community captures. Don't skip
  ahead without logging why.

## 3. Documentation is a build artifact (same-commit rule)

Marshal's directive (2026-11-22): heavy human-readable docs are built
*with* the software, not after it.

- **Same-commit rule:** a change and its documentation land in the same
  commit. "Docs later" is debt this project does not carry.
- `docs/` builds into an mdBook site in CI on every commit; broken internal
  links fail the build. Keep links valid.
- Every crate builds `cargo doc` with **warnings denied**.
- The admin API is **utoipa → `openapi.json`**; spec drift fails CI. The
  Console SPA consumes generated types — API-first is enforced by the
  build, not by habit.
- One runbook entry minimum per gate in `docs/runbook/` — Marshal-style:
  what was attempted, what broke, lessons learned.
- The doc-site artifact is attached to every gate tag; `gate/g3` must
  forever include the docs as they stood when G3 passed.

## 4. Repository layout and naming

```
crates/
  oob-dac/       # unit DB (sqlite, DAC Define Terminal schema), entitlements,
                 # scheduler, WireLink-verb admin API (tokio+axum)
  oob-mux/       # TS writer, PSI/SI assemblers, PID plan, insertion-bandwidth shaper
  oob-msg/       # GI message layer — the O4 protocol doc as code
  oob-cdl/       # DLS: .obj/.dat loader, carousel builder, EMM download-control
  oob-playout/   # clear-tier playout → SEM GigE
  oob-starfone/  # modem-bank bridge to mgetty NAS (stretch, post-G4)
  oob-guide/     # VueGuide+: listings synthesizer, guide feed, R12 fallback grid
flowgraphs/      # GNU Radio: qpsk_oob_tx.grc, starvue_rx.grc — versioned here
lab/
  harness/       # message-lab runner: inject → capture box state → log
  captures/      # box evidence (git-crypt tier where identifying)
ops/
  lineup.yaml    # single source of truth: RF lineup ↔ VCM ↔ playout
  units.yaml     # census + unit DB (DAC Define Terminal schema)
  ci/            # nightly canary: reboot box, assert AUTH, scrape d 02/d 15
docs/            # mdBook source; protocol/ and runbook/ live here
```

Naming is adopted and fixed: software **Oob**, crates `oob-*`, guide stack
**VueGuide+**, operator UI **Oob Console**, operator brand skin **ArchaiTel
Digital Cable** ("powered by Oob"). Release tags are hunt-table carriers in
box scroll order: `75.25`, `104.20`, `72.75`, `92.25`, `98.25`, `107.25`,
`107.40`, `110.25`, `116.25`, `103.75` — in that sequence. Do not invent
new component names; extend this set only via Marshal.

## 5. Config-as-code and secrets

- `ops/lineup.yaml` and `ops/units.yaml` are the single source of truth for
  lineup and fleet. Both are pydantic-schema-validated in CI; changes go
  through git, never through ad-hoc edits on the host. Lineup-consistency
  CI (lineup ↔ VCM ↔ playout ↔ SEM plan) must stay green.
- **Secrets tier:** FPK constant, box flash dumps, firmware images, key
  material, and box-identifying captures live in the **git-crypt tier or
  outside git — never in plaintext, never in chat/memory**. gitleaks runs
  in CI; a fresh-clone test proves nothing sensitive leaks. If a task
  requires handling such material, keep it in the encrypted tier and
  reference it by path only.
- No real broadcaster content anywhere in the pipeline: playout is
  self-authored loops and authored listings only.

## 6. Reviews, findings, pull requests, and commit conventions

- **Adversarial reviews** use the r-series format (see
  `docs/Reproduction Plan — Adversarial Review r4.md`): findings are
  F-series-numbered (F5-1, F5-2, … for the next round) with severity,
  attack, why-it-matters, delta; then "what survived" and a disposition
  table. Major gates G1/G3/G6 require one before PASS.
- **Commit messages:** conventional-prefix, project-flavored —
  `feat(oob-mux): …`, `fix(oob-msg): …`, `docs: …`, `lab(exp): …`,
  `ops(lineup): …`. Reference findings and open items by ID
  (`implements F4-6`, `refs O13`).
- **Gate tags** `gate/gN` are created only by Marshal at gate review.
  Agents prepare; they do not tag gates.
- **Nightly canary** (post-M1): reboot box → assert `AUTH`/`E 00` → scrape
  d 02/d 15. Canary red >3 nights reopens a milestone; treat canary
  failures as highest-priority work.

### Pull requests (mandatory for declared features)

Every **declared feature** — any board item of Type `feature` or
`experiment`, and any change touching `crates/`, `flowgraphs/`, `ops/`,
`lab/`, or `.github/` — merges **only** through a pull request that
satisfies all of the following. Docs-only typo fixes may still land
directly on `main`; everything else goes through review.

1. **Marshal's merge is the human approval.** Pull requests are opened
   from the owner's account, and GitHub does not let an author approve
   their own PR, so the approval is not recorded as a review: **Marshal
   merging the PR is the approval.** Marshal merges personally (GitHub UI
   or his own CLI). An agent never runs the merge, never approves, and
   never reviews its own work. `CODEOWNERS` still routes every path to
   the owner for notification.
2. **Completed role reviews** per change class — all required reviewers
   have submitted a review and no change request stands open:
   - crate boundaries / wire formats / PID plan / OpenAPI surface →
     **oob-architect**
   - box-behavior claims, captures, canary, drills → **oob-qa**
   - flowgraphs, RF chain, SEM integration → **oob-dev-rf**
   - Console SPA and journeys → **oob-dev-console** (plus oob-architect
     when the API surface moves)
   - core cross-review for shared crates → **oob-dev-core**
   - board/process/gate items → **oob-scrum** or **oob-pm**
   The PR template's Review routing section records which apply; "not
   applicable" must be justified in the PR body.
3. **Reviews are completed and current.** A required role review counts
   only when it is submitted, names the head commit SHA it reviewed, and
   leaves no change request standing. Role reviews post as PR reviews or
   comments, so GitHub does not dismiss them automatically: after any new
   commit, each affected role re-reviews the new head before Marshal
   merges.
4. **The template is the price of entry.** PRs use
   `.github/pull_request_template.md` in full: gate/workstream served,
   findings and open items touched, evidence links, docs checklist, test
   evidence, safety declarations, review routing. An incomplete template
   is not reviewable.
5. **Agents prepare, Marshal merges.** Agents prepare branches, PRs,
   evidence, and role reviews; the merge (and with it the approval) is
   Marshal's alone. Gate tags remain manual and Marshal-only.
6. **Branch protection (one-time owner action).** On `main`: require a
   pull request before merging, with **0 required approvals** and
   **CODEOWNERS review not required** (both would block every PR, since
   the author can't approve their own PR — rule 1); require status checks
   (fmt/clippy/nextest, schema + lineup CI, mdBook build, OpenAPI drift,
   gitleaks) using the CI job display names; require branches up to
   date; do not allow bypassing these settings; block force pushes and
   deletion.

## 7. Hardware safety and the closed-plant doctrine

These are hard rules with legal weight:

- **Closed plant, always.** RF lives in terminated coax. Dummy load first
  on every new TX path; leakage sweep before connecting the plant. OOB
  carriers sit in the FM broadcast band (104.2/107.25/107.40 MHz) —
  shielding and termination are policy, not preference. A hub or bench
  that radiates gets shut down.
- **Destructive box operations default to the sacrificial unit.** Code
  download campaigns, Cold Initialize, flash write-back: sacrificial
  DCT2000 first, verified recovery path (D0) before any fleet box.
  Destructive ops require typed confirmation from Marshal — state exactly
  what will happen and wait for an explicit yes.
- **Flash recovery before download** (r3 F3-3 / r4 F4-3): no download
  enable until dump + verified restore works via *two* routes
  (TL866 + 68k BDM).
- NVRAM-battery-dead boxes (E 09) are normal fleet reality; the
  factory-init message path is the fix, not soldering (r4 F4-4).
- CPSC 02-227 cord check is part of census; never power an unchecked box.

## 8. Technical constraints to honor in code

- **MPEG-2 MP@ML 480i only** for video (AC-3 192–384 kbps or MP2 audio);
  the fleet decodes nothing newer. ffmpeg lint runs in CI.
- **J.83 Annex B** (DCII mode), 64/256-QAM; lineup ≤860 MHz (box tuner
  ceiling; the SEM goes to 861). STD vs HRC is decided once and must agree
  across SEM config, `lineup.yaml`, and the VCM (r4 F4-8).
- **DCII-flavored PSI/SI**, not vanilla MPEG: CA descriptors with
  GI/Motorola CA-system IDs; validated box-side via d 15/d 16 (r4 F4-6).
- **EMM provider ID 0x0000/0x0001** on the OOB stream from day one — wrong
  values force boxes to re-hunt (install manual).
- OOB stream: QPSK 1.024 Msym/s (~1.544 Mb/s), asynchronous, null-stuffed;
  the flowgraph owns timing, not the daemons. Insertion bandwidth budget
  ~1000 Kbps for DAC traffic.
- **Period-authentic behavior is a feature:** 68k-era guide redraw lag is
  not a bug; degradation modes (carousel freeze on outage, `E 01` on OOB
  loss) are to be reproduced faithfully, not papered over.
- Guide fidelity target is **residency-first**: whatever app the fleet
  census finds (expected i-Guide). The R12 substitute grid, if ever
  reached, is themed **Navigator/Passport**, with the headend-rendered
  Prevue-style video guide channel as the cheaper fallback (r3 F3-4).
- Enrollment order is manual-confirmed: **factory-init → unit-creation →
  connect** → VCM/region/inband-processor/general → polls/disconnect
  (r4 F4-4). There is no OTA auto-enrollment; the unit DB *is* enrollment.

## 9. Console and interconnect phasing

- **Oob Console** phases C0–C5 (Console spec §5). C0 — the experiment
  workbench — is a **G1 exit requirement**, not a side project. Console is
  API-first: it ships no logic the WireLink-verb admin API doesn't expose,
  and its six operator journeys must pass without SSH by G6.
- **Interconnect R0–R5** is stretch S2: no schedule weight until G6.
  Do not build hub code pre-G6 except `site.yaml` schema (R0).
- S3 (analog tier) is registered but unscoped; do not start it.

## 10. Toolchain (pinned)

- Rust via `rust-toolchain.toml`; `cargo clippy`, `fmt`, `deny`,
  `nextest`, `criterion` (bit-rate benches for the insertion shaper).
- Console: Node LTS + pnpm + Vite + TypeScript; vitest; types generated
  from `openapi.json`.
- Lab glue: Python 3.12 + uv; pydantic schemas mirror CI validation.
- Deploy: Podman + podman-compose, rootless, images pinned by digest;
  host-net/macvlan for playout multicast (r4 F4-7).
- PHY: GNU Radio 3.10 flowgraphs in-repo; HackRF One is primary OOB TX
  (Pluto spare — needs the range hack at every hunt carrier, r4 F4-1).
- RE workbench: Ghidra (m68k), binwalk, capstone, imhex (pattern templates
  double as wire-format drafts), minipro, 68k BDM, sigrok.

## 11. What agents must not do

- Do not declare gates passed, create gate tags, or disposition findings.
- Do not commit secrets, dumps, or key material in plaintext — or paste
  them into chat, issues, or PRs.
- Do not bypass paywalls or fetch restricted documents; if a source
  (e.g., SCTE 55-1) needs acquiring, flag it to Marshal (open item O7).
- Do not "improve" period-authentic behavior (redraw lag, degradation
  modes) — fidelity targets come from the census and the docs.
- Do not start work whose gate entry criteria are unmet, and do not give
  surplus-hardware finds any planning weight (opportunism only).
- Do not approve, merge, or self-review pull requests. Declared features
  merge only when Marshal merges them, after all required role reviews
  are completed on the head commit (§6); direct pushes to `main` beyond
  docs-only typos are out of bounds.
- Do not touch live cable plant, licensed services, or anyone else's
  content — ever.

## 12. Definition of done for an agent task

A task is done when: code builds with warnings denied; tests pass under
nextest; schemas and lineup CI are green; OpenAPI drift check is clean;
docs updated in the same commit (same-commit rule); experiment claims
have captures; the PR text states which gate/workstream it serves and
which findings/open items it touches; and — for any declared feature —
all required role reviews are completed on the head commit and the PR
is ready for Marshal to merge (his merge is the approval, §6). Work that can't meet this stays a draft and says why.

## 13. Role agents

This repo carries a nine-agent roster under `.github/agents/` — architect,
PM, PO, core dev, RF dev, console dev, QA, scrum master, and research
expert — plus a README defining the Oob project board (fields, columns,
labels, standing rules). Every agent in this repo works as exactly one of
those roles at a time; the role files refine this contract, never replace
it. Board conventions (gate/workstream fields, `Blocked (entry criteria)`
state, Done = gate-passed only, feature PRs merged by Marshal after
completed role reviews) are binding on all roles.
