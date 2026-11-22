# Oob Agent Roster & Project Board

Nine role agents serve this repo. Each `.agent.md` file in this directory
defines one role's scope, rules, and handoffs; `AGENTS.md` at the repo
root is the shared operating contract (stage gates, evidence rules,
closed-plant doctrine) that every role inherits.

## Roster

| Agent | Role | Owns |
| --- | --- | --- |
| `oob-architect` | Architect | Crate boundaries, O4 protocol doc as code, design truth vs manuals, gate packs (technical) |
| `oob-pm` | Project manager | Board, gate calendar, dependency map, risk register, buy list, disposition ledger |
| `oob-po` | Product owner | Backlog, stories, acceptance criteria, fidelity targets (census-defined), open items O1–O13 |
| `oob-dev-core` | Core developer (Rust) | `oob-dac`, `oob-mux`, `oob-msg`, `oob-cdl`, `oob-playout`, `oob-starfone`, `oob-guide` |
| `oob-dev-rf` | RF/bench developer | Flowgraphs, SDR roles, F3-7 RF chain, SEM integration, closed-plant discipline |
| `oob-dev-console` | Console developer | Oob Console SPA (Vite/TS/pnpm), API-first, journeys C0–C5 |
| `oob-qa` | QA / verification | Capture rig, canary, evidence rule, verified-status promotion, drills |
| `oob-scrum` | Scrum master | Ceremonies, board flow, blockers, rule protection, gate-review logistics |
| `oob-expert` | Research desk | Manuals, specs, patents, community art, part decodes, surplus watch — cited notes only |

Routing rule: pick the role whose "owns" column matches the task. A task
spanning roles gets handed off explicitly per each file's Handoffs
section — never silently absorbed.

**Roles build, and run as subagents.** Each role has a Claude Code
subagent in `.claude/agents/<role>.md` that loads `AGENTS.md` and this
role file. The orchestrating session hands each step of the AGENTS.md §14
workflow to its role: PO shapes issues, PM builds the board, architect
designs before `Ready`, a dev subagent builds one issue per run, QA and
the architect review, and PO accepts on the issue. Marshal grants the
`project` scope once; after that, oob-pm creates and maintains the
board. Only **oob-qa** (bugs and fixes) and
**oob-architect** (drift) comment on PRs.

## The Oob project board

All work flows through the GitHub Projects board **"Oob"** (create it
under the owner's account, linked to this repo). Board setup:

**Custom fields**

| Field | Type | Values |
| --- | --- | --- |
| Gate | single select | `G0.5`, `G1`, `G2`, `G3`, `G4`, `G5`, `G6`, `post-G6` |
| Workstream | single select | `W1`–`W8`, `E1`–`E4`, `C0`–`C5`, `R0`–`R5`, `ops` |
| Type | single select | `experiment`, `feature`, `docs`, `finding`, `research`, `chore` |
| Evidence | single select | `n/a`, `needed`, `captured`, `verified` |

**Columns**

1. **Backlog** — refined, acceptance criteria present (PO's bar).
2. **Blocked (entry criteria)** — first-class state; the blocker is named
   in the issue (e.g., "needs D0 verified restore", "needs O7").
3. **Ready** — gate entry criteria met; devs pull only from here.
4. **In progress** — small WIP; one active slice per workstream.
5. **In review** — PR + evidence pack assembled; oob-qa bug review and
   oob-architect drift review at the head SHA.
6. **Gate review** — queued for Marshal's PASS / PASS-WITH-DELTAS / HOLD.
7. **Done** — gate-passed only. Only Marshal moves cards here.

**Labels (repo)**

- `gate:g0.5` … `gate:g6` — the gate an item serves.
- `ws:w1` … `ws:w8`, `track:e1` … `track:e4` — workstream/track.
- `finding:F3-1` … `finding:F4-8` — implements or is blocked by a review
  finding.
- `open-item:O1` … `open-item:O13` — tracks an ADR-001 open item.
- `evidence-needed`, `canary-red`, `blocked-entry`, `stretch`.

**Standing board rules**

1. Every issue names its gate; the gate plan (Milestones & Stage Gates
   §3) is the only sequencing authority.
2. A finding-implementing PR cites the finding ID in commit and PR text
   (e.g., `implements F4-6`).
3. Dispositions (r3 ×11, r4 ×8, ADR-001 rev 7) live as board items in
   **Gate review** until Marshal rules.
4. Canary red >3 nights: scrum master files the reopen item against the
   highest affected milestone the same day.
5. Stretch columns (`post-G6`) hold Interconnect/S3 items with no
   schedule weight; they never enter `Ready` early.
6. **Every declared feature merges only by pull request, merged by
   Marshal after the oob-qa and oob-architect reviews are completed on
   the head commit** — Marshal's merge is the human approval (AGENTS.md
   §6; template at `.github/pull_request_template.md`). Cards sit in
   **In review** until both reviews are clean and Marshal merges; then
   oob-scrum moves the card to **Gate review**. No agent moves a card to
   **Done**.
