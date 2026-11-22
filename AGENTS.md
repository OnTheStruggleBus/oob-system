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

Review rounds r3 (F3-1…F3-11), r4 (F4-1…F4-8) and r5 (F5-1…F5-14) were
all accepted on 2026-11-22. Open findings: r6 (F6-1…F6-16), awaiting
Marshal's disposition.
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
r6 dispositions and ADR-002. Do not schedule work past G1 entry criteria until
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
  agent-guard.py # command guard for agent shells (§14 Credentials)
  agent-guard.kimi.toml # Kimi Code hook entry for ~/.kimi-code/config.toml
  install-agent-guards.py # installs the Kimi entry (user-level only)
  check-agent-hooks.py # CI: every tool's hook is wired, enabled, and blocks
docs/            # mdBook source; protocol/ and runbook/ live here
.github/agents/  # role files: one contract slice per role (source of truth)
.claude/agents/  # Claude Code subagent launchers, one per role (§14)
.claude/settings.json, .cursor/, .gemini/, .codex/, .github/hooks/
                 # per-tool hooks running ops/agent-guard.py (§14)
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
`lab/`, `.github/`, or `.claude/` — merges **only** through a pull request that
satisfies all of the following. **Every change to `main` goes through a
pull request** — branch protection allows no direct pushes (rule 6).
Docs-only typo fixes are the light path: a PR with green checks and
Marshal's merge, no review comments required.

1. **Marshal's merge is the human approval.** Pull requests are opened
   from the owner's account, and GitHub does not let an author approve
   their own PR, so the approval is not recorded as a review: **Marshal
   merging the PR is the approval.** Marshal merges personally (GitHub UI
   or his own CLI). An agent never runs the merge, never approves, and
   never reviews its own work. `CODEOWNERS` still routes every path to
   the owner for notification.
2. **Two reviewers, two lenses — and only they comment on PRs.** Every
   PR outside the light path gets **the required reviews**: normally one
   from each role below; one combined review when a reviewer authored
   the PR (see below). Everywhere this contract says "both reviews", it
   means the required reviews:
   - **oob-qa — bugs and fixes.** Correctness, tests, evidence rule,
     safety declarations, regressions. Each finding states the defect,
     how it fails, and a concrete fix.
   - **oob-architect — drift.** Divergence from the docs, AGENTS.md,
     ADRs, crate boundaries, wire-format status, dispositioned findings,
     and naming. Each finding states what drifted from what, and whether
     the code or the doc should move.
   **No other role comments on a PR.** PO, PM, scrum, expert, and the dev
   roles contribute *upstream* — issues, acceptance criteria, board
   state, ADRs, research notes — and the authoring role answers review
   findings by pushing commits or by disputing them in the PR
   description, never by PR comment. Both reviews post as
   one PR comment each, headed `Role review: oob-qa` /
   `Role review: oob-architect`, because all agents act through the
   owner's GitHub account.
   **Independence:** a review counts only if the reviewing role authored
   none of the PR's commits. Every agent commit ends with an
   `Oob-Role: <role>` trailer (`orchestrator` for the main session), and
   the reviewer checks the trailers (`git log --no-merges`) before
   reviewing. Only merge commits are exempt. Every other commit without
   a trailer — including any Marshal made himself, since all commits
   carry his identity — is a finding until the PR description lists it
   as `<sha> → <role or Marshal>`. Roles run as
   separate subagents (§14), so the dev that built a change is never the
   role that reviews it.
   **When a reviewer authored the PR** (an ADR from oob-architect, a
   harness PR from oob-qa), the other reviewer covers both lenses in its
   one comment. A PR with commits from **both** reviewer roles is split;
   if it can't be, Marshal reviews it himself before merging.
   **Disputes:** the authoring role may contest a finding in a
   *Disputed findings* section of the PR description (finding ID +
   reason) — never as a PR comment. Editing that section triggers a
   re-review by the finding's reviewer even when no commit was pushed;
   the reviewer withdraws or holds each disputed finding. Only Marshal
   can waive a held finding, by writing `waived by Marshal` against it in
   the same section **before** merging; a waived finding counts as
   cleared for rule 3 and §12. **Agents never write, edit, move, or remove
   a waiver line** — except the orchestrator recording a waiver Marshal
   gave in its session, quoted verbatim with the date. A reviewer that
   sees a waiver line new since its last review and not in that quoted
   form adds a **note for Marshal to confirm** — not a finding, so it
   never blocks the merge. Marshal confirms by merging.
   **Scope:** these rules govern PRs written by agents. Machine-opened
   PRs from the running system (e.g. r6 F6-4's Console-opened config
   PRs) need their own rule, to be set when F6-4 is dispositioned.
3. **Reviews are completed and current.** A QA or architect review counts
   only when it is submitted, is **current**, and leaves no finding
   standing. A review is current when the SHA it names is the PR's head,
   or is the head from before main-only merges that meet the exception
   below. Everywhere this contract, the PR template, the board README,
   or a role file says a review is completed "on the head commit" or
   "at the head SHA", it means current in this sense. Reviews post as PR comments, so
   GitHub does not dismiss them automatically: after any new commit,
   the required reviews (rule 2) are re-run on the new head before
   Marshal merges. **Exception — main-only merges:** a merge commit that
   only brings `main` into the branch (as "require branches up to date"
   forces) needs no re-review if the PR's diff against `main` is
   byte-identical to the diff that was reviewed. The orchestrator checks
   this and records it in the PR description's *Merge readiness* section
   (`main-only merges since review: <sha>… — diff unchanged`); any
   difference means a full re-review.
4. **The template is the price of entry.** PRs use
   `.github/pull_request_template.md` in full: gate/workstream served,
   findings and open items touched, evidence links, docs checklist, test
   evidence, safety declarations, review routing. An incomplete template
   is not reviewable.
5. **Agents prepare, Marshal merges.** Agents prepare branches, PRs,
   evidence, and the QA/architect reviews; the merge (and with it the
   approval) is Marshal's alone. Gate tags remain manual and
   Marshal-only.
6. **Branch protection (one-time owner action).** On `main`: require a
   pull request before merging, with **0 required approvals** and
   **CODEOWNERS review not required** (both would block every PR, since
   the author can't approve their own PR — rule 1); require status
   checks by CI job display name — today exactly `rustfmt`,
   `clippy (-D warnings)`, `nextest`, `cargo doc (warnings denied)`,
   `cargo-deny`, `gitleaks`, `agent-guard`; require branches up to date; do not allow
   bypassing these settings; block force pushes and deletion.
   **Required checks grow with CI:** a required check that never reports
   blocks every merge, so each new CI job (mdBook link check per F5-10,
   schema + lineup CI, OpenAPI drift) is added to the required list in
   the same PR that introduces it — never before.

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
- Do not approve, merge, or self-review pull requests, and do not
  comment on PRs unless you are oob-qa (bugs) or oob-architect (drift).
  Declared features merge only when Marshal merges them, after the
  required reviews are completed on the head commit (§6); direct pushes
  to `main` are out of bounds — typo fixes included (they take the light
  PR path).
- Do not write, edit, move, or remove a `waived by Marshal` line in a PR
  description, except as the orchestrator quoting Marshal verbatim
  (§6 rule 2).
- Do not touch live cable plant, licensed services, or anyone else's
  content — ever.

## 12. Definition of done for an agent task

A task is done when: code builds with warnings denied; tests pass under
nextest; schemas and lineup CI are green; OpenAPI drift check is clean;
docs updated in the same commit (same-commit rule); experiment claims
have captures; the PR text states which gate/workstream it serves and
which findings/open items it touches; and — for any declared feature —
the required reviews (§6 rule 2) are completed on the head commit with
no findings standing (or waived by Marshal), oob-po has recorded
acceptance on the issue (§14 step 8) — or, for a PR with no issue
because it is Marshal's direct request, his merge is the acceptance —
and the PR is ready for Marshal to merge (his merge is the approval,
§6). Work that can't meet this stays a draft and says why.

## 13. Role agents

This repo carries a nine-agent roster under `.github/agents/` — architect,
PM, PO, core dev, RF dev, console dev, QA, scrum master, and research
expert — plus a README defining the Oob project board (fields, columns,
labels, standing rules). Every agent in this repo works as exactly one of
those roles at a time, with one exception: the **orchestrator** (the
main Claude Code session, §14), which routes work between roles and does
no role's work itself except to bootstrap the roles. The role files
refine this contract, never replace it. `.claude/agents/` holds a Claude Code subagent for each role — a thin
launcher that loads this contract and the role file, so there is one
source of truth per role. Board conventions (gate/workstream fields,
`Blocked (entry criteria)` state, Done = gate-passed only, feature PRs
merged by Marshal after the required reviews (§6 rule 2) and oob-po
acceptance (§14 step 8)) are binding on all roles.

## 14. Agent-driven workflow (roles build, not just review)

Work is done by the roles themselves, each running as its own subagent
task. The main session is the **orchestrator**: it reads the board, hands
each step to the role that owns it, and relays results to Marshal. It does
not do a role's work itself when a role exists for it.

| Step | Role (subagent) | Builds |
| --- | --- | --- |
| 1. Shape | **oob-po** | Issues from the docs and Marshal's intent: one outcome each, acceptance criteria, gate, workstream, findings/open items cited |
| 2. Plan | **oob-pm** | Board placement, fields, sequencing against gate entry criteria, dependency links, `Blocked (entry criteria)` with the blocker named |
| 3. Design | **oob-architect** | Before an issue enters `Ready`: an ADR (new decision) or a design note on the issue (interfaces, crate placement, data ownership). Issues that touch boundaries, wire formats, the PID plan, or the API surface need this sign-off. The architect consults the dev role that will build it; dev roles are consulted on ADRs, not reviewers |
| 4. Research | **oob-expert** | Cited research notes on request from any step; never code |
| 5. Build | **oob-dev-core** / **oob-dev-rf** / **oob-dev-console** | Pulls one `Ready` issue, works on its own branch (in its own worktree when parallel), lands code + tests + docs in the same commits, opens the PR with the full template |
| 6. Review | **oob-qa** then **oob-architect** | The required reviews at the head SHA (§6 rule 2): QA for bugs and fixes, architect for drift; one combined review when a reviewer authored the PR |
| 7. Fix | the authoring role | Pushes commits that resolve each finding, citing it in the commit message, or disputes it in the PR description (§6 rule 2); the required reviews re-run on the new head |
| 8. Accept | **oob-po** | Once the required reviews are clean, records accept or reject against the acceptance criteria **on the issue** (not the PR). Acceptance is a merge criterion (§12). On reject, oob-scrum returns the card to `In progress` and the authoring role fixes the gaps the PO names |
| 9. Flow | **oob-scrum** | Moves cards `Ready` → `In progress` → `In review`; after Marshal merges, moves the card to `Gate review`; enforces WIP and entry criteria, flags blockers |
| 10. Merge | **Marshal** | Merges (the approval); gate tags, dispositions, and `Done` stay his |

Rules for running roles as subagents:

- **One role per subagent run.** A subagent never switches roles mid-task;
  a handoff is a new subagent call for the next role.
- **Dev work is always a subagent task**, one issue per run. Independent
  `Ready` issues may run in parallel subagents, each in its own git
  worktree and branch; issues that touch the same files run one at a
  time.
- **Every subagent returns** what it changed (issues, branches, PRs,
  commits by SHA), what it could not do, and the next role's handoff.
- **Board access** needs the `project` scope on the owner's `gh` token
  (`gh auth refresh -s project` — a one-time owner action). Until then,
  roles maintain issues and labels, and record intended board fields in
  each issue body.
- **Credentials:** until agents have their own identity, subagents act
  through the owner's `gh` login, so GitHub itself cannot stop an agent
  from merging; "never merge" is enforced only by this rule and, best
  effort, by the command guard below (an unnumbered candidate for the
  next review round; r6 itself ends at F6-16). Subagents must never call merge, approve, or
  admin endpoints, and must not be given any other owner-level or
  merge-capable token. Every launcher grants `Bash`, and that shell runs
  with the owner's `gh` login, so the rule alone can't stop a misbehaving
  role. **Marshal chose option (b), a command guard (2026-11-22):**
  - `ops/agent-guard.py` blocks shell commands that merge or approve PRs,
    change repository admin settings (protection, rulesets, secrets,
    repo settings), read, print, or change the `gh` login, call the
    GitHub API through curl/wget, force-push, push to `main`, or edit the
    guard and hook files. Rules match within one command (pipes, chains
    and new lines split commands); a few deliberately look further and
    only ever block more. Heredoc text (commit messages, PR bodies, the
    usual `"$(cat <<'EOF' …)"` form) is ignored unless it is fed to
    `gh api` or an interpreter such as `bash` or `eval`. The CI job
    `agent-guard` runs `python3 ops/agent-guard.py --self-test` and
    `python3 ops/check-agent-hooks.py`. The checker requires each hook's
    exact keys (an unknown key could disable it), event, matcher, and a
    minimum timeout in the tool's own unit; rejects `disableAllHooks`;
    requires one identical launcher; and runs every launcher with its
    tool's payload both in the checkout and in a throwaway repo whose
    checkout lacks the guard, expecting a merge blocked and `git status`
    allowed. It cannot tell whether a tool honours a key it ignores at
    runtime; QA reviews CI-workflow changes (the checker's own step lives
    in `.github/workflows/ci.yml`, which the guard does not lock).
  - **Changing branch doesn't disable it or brick a session** — on any
    branch that carries the hook files (everything cut after this guard
    landed). Every hook file runs the same launcher: the checked-out
    `ops/agent-guard.py` if present, otherwise the copy on `origin/main`,
    otherwise the newest commit on a remote-tracking branch that has it
    (never an agent's local-only branch); outside a git repo it does
    nothing. A branch cut before the guard has no hook files of its own;
    Claude Code keeps the hooks it loaded at session start, but other
    tools reading that checkout run unguarded.
  - **It fails closed on what it can see:** its own errors, and a
    shell-tool payload with no readable command (an unknown payload
    shape), both block with a message. A non-shell tool that reaches the
    hook (a tool ignoring the matcher) is let through.
  - **It is wired for everyone who clones the repo.** This is a public
    repository, so each tool's per-repo hook file is committed and points
    at the same guard; CI (`agent-guard` job) runs the self-test and checks
    every file still calls it:

    | Tool | Hook file (committed unless noted) | If the guard can't start or times out |
    | --- | --- | --- |
    | Claude Code | `.claude/settings.json` (PreToolUse/Bash, plus edit-deny rules) | command runs |
    | Cursor | `.cursor/hooks.json` (beforeShellExecution, `failClosed`) | command blocked |
    | Gemini CLI | `.gemini/settings.json` (BeforeTool/run_shell_command) | command runs |
    | Codex | `.codex/hooks.json` (PreToolUse/Bash, `--format codex`) | command runs |
    | GitHub Copilot CLI / cloud agent | `.github/hooks/agent-guard.json` (preToolUse/bash) | blocked if it can't start; runs on timeout |
    | Kimi Code | user-level only: run `uv run --python 3.12 python3 ops/install-agent-guards.py` (installs or updates the `ops/agent-guard.kimi.toml` entry in `~/.kimi-code/config.toml`) | command runs |

    The column above covers what the guard can't control: a missing
    `python3` or a hook timeout. **Python versions:** the guard itself
    runs on any `python3` (3.9+); the installer and
    `ops/check-agent-hooks.py` need 3.11+ (the §10 pin is 3.12, recorded
    in `.python-version`). `python3` is part of the pinned
    toolchain (§10), so a working setup has it.

    Most tools ask you to trust a folder or review its hooks (`/hooks`)
    before project hooks run; a contributor who declines has only this
    rule. Only Claude Code also blocks *file-tool* edits of the guard and
    hook files; elsewhere only shell edits are blocked.
  - **Other AI tools** call it from their own pre-command hook as
    `python3 ops/agent-guard.py --check "<command>"` (exit 2 = block).
    A tool with no pre-command hook is covered by this rule only.
  - It is best effort: pattern matching can be evaded by deliberate
    obfuscation. **Known, accepted gaps** (recorded at Marshal's merge of
    PR #10, not pursued further): `cp -t <dir> <file>`; acting on a
    guarded file by a generic bare name from inside its directory
    (`cd .claude && rm settings.json`); `git apply` of a patch that edits
    a guarded file; and the CI workflow `.github/workflows/ci.yml`
    itself, which QA reviews rather than the guard locking. A guarded
    file inside any other checkout is protected too, by design. The alternatives Marshal did not choose — (a) a bot
    account or per-role token without merge/admin rights, (c) an
    allowlisted GitHub wrapper — remain the way to make it airtight.
  - Changing the guard or any hook file above is Marshal's job; agents
    are blocked from both, and a PR that weakens them is a drift finding. Running-system credentials (r6
  F6-4/F6-10) are set when those findings are dispositioned.
- §11 still binds every subagent: no merging, approving, gate tags,
  dispositions, or moving cards to `Done`.
