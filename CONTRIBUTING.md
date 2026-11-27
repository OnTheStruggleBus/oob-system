# Contributing to Oob

Oob is a faithful software reproduction of a GI/Motorola cable headend, built to bring real DCT2000-class set-top boxes back to life on a closed lab plant.

This file is for people contributing development work, usually with their own AI agent (Claude Code, Codex, Cursor, Gemini CLI, Kimi Code, or similar). It is the quick-start. [`AGENTS.md`](AGENTS.md) is the full operating contract, and where this file and `AGENTS.md` disagree, `AGENTS.md` wins for how work is done.

## 1. Read first

- [`AGENTS.md`](AGENTS.md): the process. Stage gates (§1), evidence and box-as-oracle (§2), same-commit docs (§3), reviews and PRs (§6), the role roster and workflow (§13, §14).
- The role file for the work you are doing, under [`.github/agents/`](.github/agents/README.md). The roster and the project board rules are in [`.github/agents/README.md`](.github/agents/README.md).
- The one or two documents in [`docs/`](docs/) for the workstream you are touching. Start with the [project overview](docs/mini-headend%20Project%20Overview%20—%20Faithful%20Lab%20Reproduction.md), the [stage gates](docs/Project%20Milestones%20&%20Stage%20Gates%20—%20mini-headend.md), and the [reproduction plan](docs/Oob%20Reproduction%20Plan%20—%20mini-headend.md) (workstreams W1–W8). The Console spec and the interconnect spec cover their own areas.

## 2. Set up the agent guard first

Do this before your agent runs a single shell command in this repo.

[`ops/agent-guard.py`](ops/agent-guard.py) is a command guard for agent shells. It blocks the commands an agent must never run: merging or approving PRs, changing repository admin settings, reading the `gh` token, force-pushing, pushing to `main`, and editing the guard itself. Agents act through your own `gh` login, so nothing on GitHub's side tells an agent apart from you. The guard is the agreed baseline (AGENTS.md §14).

The guard is wired per tool, and the hook files are committed:

| Tool | Hook file |
| --- | --- |
| Claude Code | `.claude/settings.json` |
| Cursor | `.cursor/hooks.json` |
| Gemini CLI | `.gemini/settings.json` |
| Codex | `.codex/hooks.json` |
| GitHub Copilot CLI / cloud agent | `.github/hooks/agent-guard.json` |
| Kimi Code | user-level only (see below) |

Most tools ask you to trust the folder or review its hooks (for example with `/hooks`) before project hooks run. Accept that review. If you decline, you have only the written rules.

Kimi Code reads hooks only from its user-level config, so run this once from the repo root:

```sh
uv run --python 3.12 python3 ops/install-agent-guards.py
```

It adds the entry from `ops/agent-guard.kimi.toml` to `~/.kimi-code/config.toml`. Re-running it updates the entry.

Two things to know:

- The guard is best effort, not a sandbox. Pattern matching can be evaded on purpose. It stops mistakes and casual misbehaviour. Do not treat it as a security boundary.
- Do not disable it, edit it, or edit the hook files. CI's `agent-guard` job runs `python3 ops/agent-guard.py --self-test` and `python3 ops/check-agent-hooks.py`, which check that every committed hook is wired, enabled, and blocks a merge. A PR that weakens the guard or its hooks is a drift finding.

If your tool has a pre-command hook but is not in the table, call `python3 ops/agent-guard.py --check "<command>"` from it (exit 2 means blocked). See the header of `ops/agent-guard.py` for details.

## 3. Pick an issue

Work comes from the project board, **"Oob-CHE"** (GitHub Projects, owner `OnTheStruggleBus`, project #3). The columns and fields are described in [`.github/agents/README.md`](.github/agents/README.md).

- **Ready**: gate entry criteria are met. Pull only from here.
- **Blocked (entry criteria)**: a gate or a hardware step is not done yet. The issue names the blocker. Do not start it. Work an unblocked card instead.
- **Backlog**: refined but not yet planned. Do not start it without a go-ahead from Marshal.
- **In progress**, **In review**, **Gate review**, **Done**: not yours to pull. **Done** is set by Marshal only.

If the issue touches crate boundaries, wire formats, the PID plan, or the API surface, and it has no architect design note yet, do not just start. Ask the architect role on the issue, or wait for Marshal to route it (AGENTS.md §14, step 3).

Check the gate on every issue. The gate plan in [`docs/Project Milestones & Stage Gates — mini-headend.md`](docs/Project%20Milestones%20&%20Stage%20Gates%20—%20mini-headend.md) is the only sequencing authority (AGENTS.md §1, rule 3).

## 4. The role you act as

Every change is credited to one role. Human or agent, you work as exactly one role per branch, and the role is recorded in the commit.

- Add the `Oob-Role:` trailer to every commit. It is required (AGENTS.md §6, rule 2). Use the role that matches the step: `oob-dev-core`, `oob-dev-rf`, `oob-dev-console`, and so on. Use `orchestrator` only for a main session that routes work between roles.
- Reviewers check the trailers, with `git log --no-merges`. A review counts only if the reviewing role authored none of the PR's commits. A commit with no trailer is a finding until the PR description accounts for it.
- If you launch subagents through `.claude/agents/` or `.codex/agents/`, use the launcher that matches the step. One role per subagent run. Do not switch roles mid-task.

## 5. Branch, build, commit

**Branches** are `<type>/<short-slug>`, where the type is one of `feat/`, `fix/`, `docs/`, `test/`, `chore/`, `ops/`, or `lab/`. Branch from current `main`.

**Commit messages** use a conventional prefix with the project flavour:

```text
feat(oob-mux): add PID plan shaper for class C0 traffic

implements F4-6
Oob-Role: oob-dev-core
Co-Authored-By: <your model's name> <its noreply address>
```

Examples of the prefix: `feat(oob-mux): …`, `fix(oob-msg): …`, `docs: …`, `lab(exp): …`, `ops(lineup): …`. Reference findings and open items by ID, for example `implements F4-6` or `refs O13`.

**Same-commit docs.** A change and its documentation land in the same commit (AGENTS.md §3). "Docs later" is not accepted.

**Evidence.** A claim about box behaviour needs a capture under `lab/captures/`, referenced from the commit or PR. A claim without a capture is not evidence (AGENTS.md §2). Each experiment is a commit.

**Before you push**, run the checks locally. CI runs the same ones with warnings denied.

```sh
cargo build
cargo clippy -- -D warnings
cargo test
cargo doc                 # warnings denied
python3 ops/docs-summary.py --check && mdbook build && python3 ops/docs-anchors.py && python3 ops/docs-facts.py
```

`mdbook` must be the pinned version (0.4.52) with `mdbook-linkcheck` 0.7.7 (AGENTS.md §10). `ops/docs-summary.py --check` fails on a stale `docs/SUMMARY.md`. `ops/docs-facts.py` checks prose against [`docs/facts.yaml`](docs/facts.yaml). If it flags a true false positive, allowlist it in `ops/docs-facts.toml` with a reason. Do not silence it by rewording the fact.

## 6. Open the PR

Open the PR against `main` and fill in **all** of [`.github/pull_request_template.md`](.github/pull_request_template.md). Delete nothing from it. An incomplete template is not reviewable (AGENTS.md §6, rule 4). The sections are:

- **Gate / workstream**: which gate and workstream the change serves, and the board item.
- **Findings and open items touched**: `implements F…`, `refs O…`, or `none`.
- **Evidence**: links to captures and the harness log, or `n/a`.
- **Docs checklist**, **test evidence**, and **safety declarations** (see section 10).
- **Review routing**: the authoring role, the issue number, and the two reviews.
- **Disputed findings** and **merge readiness**.

Direct pushes to `main` are blocked. Every change goes through a PR. Docs-only typo fixes take the light path: a PR with green checks and Marshal's merge, no review comments required (AGENTS.md §6).

## 7. Reviews

Two roles review PRs, each with its own lens (AGENTS.md §6, rule 2):

- **oob-qa** reviews for bugs and fixes: correctness, tests, the evidence rule, safety declarations, regressions.
- **oob-architect** reviews for drift: divergence from the docs, `AGENTS.md`, ADRs, crate boundaries, wire-format status, and naming.

Both reviews are posted on the PR head SHA. They must be current. After any new commit, both reviews are run again on the new head (AGENTS.md §6, rule 3). Findings are answered by pushing commits that cite each finding, or by disputing one in the PR description's **Disputed findings** section.

Only oob-qa and oob-architect comment on PRs. Other roles and contributors do not. You never approve your own PR, and you never review your own work.

## 8. Marshal merges, not you

Marshal's merge is the approval. Agents, and contributors working through agents, do not:

- merge a PR, run `gh pr merge`, or call the merge API,
- approve a PR or call the approve endpoints,
- write, edit, move, or remove a `waived by Marshal` line in a PR description,
- create `gate/gN` tags, disposition findings, or move a card to **Done**.

Gate tags, dispositions, and the **Done** column stay with Marshal (AGENTS.md §1, §6, §11).

Acceptance is recorded by the PO role on the linked issue, against its acceptance criteria. For a PR with no issue (Marshal's direct request), his merge is the acceptance (AGENTS.md §12, §14 step 8).

## 9. Hard safety rules

Read these every time. They have legal weight (AGENTS.md §7).

- **Closed plant, always.** RF lives in terminated coax. Use a dummy load first on every new transmit path, and run a leakage sweep before connecting the plant. OOB carriers sit in the FM broadcast band (around 104 MHz), so shielding and termination are policy. A hub or bench that radiates gets shut down.
- **Destructive box operations default to the sacrificial unit.** That covers code download campaigns, cold initialize, and flash write-back. Each needs typed confirmation from Marshal first. State exactly what will happen, then wait for an explicit yes.
- **Flash recovery before download.** No download enable until a dump and a verified restore work by two routes (TL866 and 68k BDM). The recovery path (D0) is verified before any fleet box is touched.
- **Never commit secrets.** No firmware, flash dumps, serials, Unit Addresses, key material, or the FPK in plaintext git, in chat, in issues, or in PRs. Box-identifying captures and key material live in the git-crypt tier or outside git, and are referenced by path only (AGENTS.md §5). gitleaks runs in CI.
- **No real broadcaster content.** Playout uses self-authored loops and authored listings only.
- **CPSC 02-227 cord check on every box.** Never power an unchecked box.

## 10. Not permitted, even if asked

- Declaring a gate passed, or dispositioning a review finding.
- Bypassing paywalls or fetching restricted documents such as SCTE 55-1. Flag the need to Marshal as an open item (AGENTS.md §11, open item O7).
- "Improving" period-authentic behaviour: guide redraw lag, carousel freeze on outage, `E 01` on OOB loss. These are fidelity targets (AGENTS.md §8).
- Starting work whose gate entry criteria are unmet (AGENTS.md §1, rule 3).
- Editing or removing a `waived by Marshal` line.
- Weakening the agent guard or its hooks, or running anything that does (AGENTS.md §14).
- Touching live cable plant, licensed services, or anyone else's content.

## 11. Where to go when stuck

- **Work questions**: comment on the issue and ping Marshal there. Do not start a card that is not in **Ready** to find out.
- **Agent guard or hook trouble**: `ops/agent-guard.py` and `ops/check-agent-hooks.py` are the authoritative source. Read them rather than guessing. The guard's header describes each tool's payload. CI's `agent-guard` job shows exactly what is checked.
- **Process questions**: [`AGENTS.md`](AGENTS.md), then [`.github/agents/README.md`](.github/agents/README.md).
- **Anything in this file that disagrees with `AGENTS.md`**: `AGENTS.md` wins. Open a docs fix PR.
