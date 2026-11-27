# Contributing to Oob

Oob is a faithful software reproduction of a GI/Motorola cable headend, built to bring real DCT2000-class set-top boxes back to life on a closed lab plant.

This file is for people contributing development work, usually with their own AI agent (Claude Code, Codex, Cursor, Gemini CLI, Kimi Code, or similar). It is the quick-start. [`AGENTS.md`](AGENTS.md) is the full operating contract, and where this file and `AGENTS.md` disagree, `AGENTS.md` wins for how work is done.

## 1. Read first

- [`AGENTS.md`](AGENTS.md): the process. Stage gates (§1), evidence and box-as-oracle (§2), same-commit docs (§3), reviews and PRs (§6), the role roster and workflow (§13, §14).
- The role file for the work you are doing, under [`.github/agents/`](.github/agents/README.md). The roster and the project board rules are in [`.github/agents/README.md`](.github/agents/README.md).
- The three documents that AGENTS.md §0 requires before your first task, all in [`docs/`](docs/):
  1. the [project overview](docs/mini-headend%20Project%20Overview%20—%20Faithful%20Lab%20Reproduction.md), for orientation;
  2. the [stage gates](docs/Project%20Milestones%20&%20Stage%20Gates%20—%20mini-headend.md), the operating contract;
  3. the workstream document(s) for your work: the [reproduction plan](docs/Oob%20Reproduction%20Plan%20—%20mini-headend.md) (W1–W8), the [Console spec](docs/Management%20Console%20Specification%20—%20Oob%20Console.md) (C0–C5), or the [interconnect spec](docs/Remote%20Service%20Delivery%20Specification%20—%20Interconnect%20(Stretch%20Goal).md) (R0–R5, post-M6 only).

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

### Other platforms (wire the guard yourself)

These platforms are not yet in the committed hook set (AGENTS.md §14), so they are not enforced by CI's `agent-guard` job. Wire the guard locally anyway. It is still the baseline. A future AGENTS.md update may add them.

For any tool with a pre-command or pre-shell hook, the call is the same, and it is stable:

```sh
python3 ops/agent-guard.py --check "<command>"   # exit 0 allows; exit 2 blocks, reason on stderr
```

The entry point differs by tool. The paths below are where to look, not verified file paths. Consult each tool's hook or config documentation for the exact file. Getting a path wrong is worse than being under-specific.

- **Grok CLI / Grok Code (xAI).** Add the call to the tool's pre-command hook configuration. A project-level or user-level hooks file is the usual place. Check Grok's documentation for the exact file.
- **Qwen Code (Alibaba).** Add the call to the user-level configuration, the same model as Kimi Code above. Check Qwen Code's documentation for the file and the hook key.
- **Windsurf (Codeium).** Add the call to a workspace or settings-level hook for shell commands, if Windsurf exposes one for your version. Check its documentation.
- **Aider.** Aider has no pre-shell hook that this file can confirm. If your setup routes Aider's shell commands through a wrapper, call the guard from that wrapper. Otherwise, consult Aider's documentation (`aider.conf.yml` and its command options) before assuming any enforcement.
- **Continue (continue.dev).** Add the call to the shell-tool wrapper if Continue provides one. A system-prompt instruction alone is not enforcement, so prefer a tool-level hook where the docs describe one.
- **Zed AI / Zed Agent.** Look for a command or tool hook in Zed's agent settings and documentation. Do not assume a command-palette entry is a hook.

Where a platform offers no hook at all, the guard does not run for that tool. You are then relying on the written rules in AGENTS.md §11 and §14 alone, and you should say so in your PR description.

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

The role is always an agent-role (AGENTS.md §13). When you contribute, your agent acts as one role for the branch (architect, PO, PM, dev-core, dev-rf, dev-console, QA, scrum, expert, or orchestrator), and the `Oob-Role:` trailer records which one. Humans contribute through agents; the role stays the agent's.

- Add the `Oob-Role:` trailer to every commit. It is required (AGENTS.md §6, rule 2). Use the role that matches the step: `oob-dev-core`, `oob-dev-rf`, `oob-dev-console`, and so on. Use `orchestrator` only for a main session that routes work between roles.
- Reviewers check the trailers, with `git log --no-merges`. A review counts only if the reviewing role authored none of the PR's commits.
- A commit without an `Oob-Role:` trailer is a finding until the PR description lists it in the form `<sha> → <role or Marshal>` in the review-routing section. Only merge commits are exempt (AGENTS.md §6, rule 2).
- If you launch subagents, use the launcher that matches the step: `.claude/agents/<role>.md`, or the parallel `.codex/agents/<role>.toml`. AGENTS.md §1 and §14 currently name only `.claude/agents/`; the `.codex/agents/` set was added later and is wired by the same launcher-hardening lint. One role per subagent run. Do not switch roles mid-task.

## 5. Branch, build, commit

**Branches** are `<type>/<short-slug>`, where the type is one of `feat/`, `fix/`, `docs/`, `test/`, `chore/`, `ops/`, or `lab/`. Branch from current `main`.

**Commit messages** use a conventional prefix with the project flavour:

```text
feat(oob-mux): DCII CA descriptor on PMT for GI CA_system_id

implements F4-6
Oob-Role: oob-dev-core
Co-Authored-By: <your model's name> <its noreply address>
```

Examples of the prefix: `feat(oob-mux): …`, `fix(oob-msg): …`, `docs: …`, `lab(exp): …`, `ops(lineup): …`. Reference findings and open items by ID, for example `implements F4-6` or `refs O13`.

**Same-commit docs.** A change and its documentation land in the same commit (AGENTS.md §3). "Docs later" is not accepted.

**Evidence.** A claim about box behaviour needs a capture under `lab/captures/`, referenced from the commit or PR. A claim without a capture is not evidence (AGENTS.md §2). Each experiment is a commit.

**Before you push**, run the checks locally. These match the required CI jobs (AGENTS.md §6, rule 6):

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo nextest run --locked
cargo test --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
python3 ops/docs-summary.py --check && mdbook build && python3 ops/docs-anchors.py && python3 ops/docs-facts.py
```

The `agent-guard`, `gitleaks`, `openapi drift` and `mdbook (link check)` CI jobs run server-side and don't need a pre-push equivalent. Just don't weaken the hook files.

`mdbook` must be the pinned version (0.4.52) with `mdbook-linkcheck` 0.7.7 (AGENTS.md §10). `ops/docs-summary.py --check` fails on a stale `docs/SUMMARY.md`. `ops/docs-facts.py` checks prose against [`docs/facts.yaml`](docs/facts.yaml). If it flags a true false positive, allowlist it in `ops/docs-facts.toml` with a reason. Do not silence it by rewording the fact.

## 6. Open the PR

Open the PR against `main` and fill in **all** of [`.github/pull_request_template.md`](.github/pull_request_template.md). Delete nothing from it. An incomplete template is not reviewable (AGENTS.md §6, rule 4). Its sections, in order:

- **Summary**: what the PR does, in operator-visible terms.
- **Gate / Workstream**: the gate and workstream served, and the board item.
- **Findings & open items touched**: `implements F…`, `refs O…`, or `none`.
- **Evidence**: links to captures and the harness log, or `n/a`.
- **Docs (same-commit rule)**: the docs checklist.
- **Test evidence**: nextest, clippy, fmt, cargo-deny, and CI status.
- **Safety declarations**: see section 9.
- **Review routing (required before merge)**: the authoring role, the issue, the two reviews, and any commits without an `Oob-Role:` trailer.
- **Disputed findings**: finding ID plus reason, or `none`.
- **Merge readiness**: the merge checklist.

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

- **Closed plant, always.** RF lives in terminated coax. Use a dummy load first on every new transmit path, and run a leakage sweep before connecting the plant. OOB carriers sit in the FM broadcast band (the OOB carriers at 104.2, 107.25 and 107.40 MHz), so shielding and termination are policy. A hub or bench that radiates gets shut down.
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
- Pushing directly to `main`. Docs-only typo fixes included; they take the light PR path instead (AGENTS.md §6).
- Giving surplus-hardware finds any planning weight. They are opportunism only, never a schedule assumption (AGENTS.md §11).
- Editing or removing a `waived by Marshal` line.
- Weakening the agent guard or its hooks, or running anything that does (AGENTS.md §14).
- Touching live cable plant, licensed services, or anyone else's content.

## 11. Where to go when stuck

- **Work questions**: comment on the issue and ping Marshal there. Do not start a card that is not in **Ready** to find out.
- **Agent guard or hook trouble**: `ops/agent-guard.py` and `ops/check-agent-hooks.py` are the authoritative source. Read them rather than guessing. The guard's header describes each tool's payload. CI's `agent-guard` job shows exactly what is checked.
- **Process questions**: [`AGENTS.md`](AGENTS.md), then [`.github/agents/README.md`](.github/agents/README.md).
- **Anything in this file that disagrees with `AGENTS.md`**: `AGENTS.md` wins. Open a docs fix PR.
