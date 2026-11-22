<!--
Oob pull request — every declared feature requires completed role reviews
on the head commit, then Marshal merges; his merge is the human approval
(AGENTS.md §6). Fill every section; an incomplete PR is not reviewable.
Delete nothing from this template.
-->

## Summary

<!-- What this PR does, in operator-visible terms. One paragraph. -->

## Gate / Workstream

<!-- e.g., Gate: G1 · Workstream: W3 · Track: E1 · Board item: #NN -->

- Gate:
- Workstream / track / phase:
- Board item link:

## Findings & open items touched

<!-- e.g., "implements F4-6", "refs O13", "partially unblocks G1 entry (E1)".
Write "none" if none. Undispositioned findings must be named if the PR
implements their delta. -->

## Evidence

<!-- Box-visible claims: link capture files under lab/captures/ and the
harness commit log. No capture = not evidence = do not claim it.
Write "n/a — no box-visible claims" if none. -->

## Docs (same-commit rule)

- [ ] Documentation updated in this PR (mdBook links valid)
- [ ] `cargo doc` builds with warnings denied for touched crates
- [ ] `openapi.json` regenerated if the admin API surface moved
- [ ] Runbook entry included if this PR closes a gate-relevant chapter

## Test evidence

- [ ] `cargo nextest` green (new/changed code has tests)
- [ ] clippy / fmt / cargo-deny green
- [ ] Schema + lineup-consistency CI green (if `ops/` touched)
- [ ] Canary impact assessed (post-M1): no expected regression

## Safety declarations

- [ ] No secrets, dumps, or key material in plaintext (git-crypt tier respected)
- [ ] No destructive box operation in this PR — **or** typed confirmation
      from Marshal is linked and the run targets the sacrificial box
- [ ] RF plant untouched — **or** dummy-load-first + leakage sweep log attached
- [ ] No live-plant, licensed-service, or third-party content involved

## Review routing (required before merge)

<!-- Mark every role review this change class requires (AGENTS.md §6):
     - crate boundaries / wire formats / PID plan / OpenAPI → oob-architect
     - box-behavior claims / captures / canary / drills → oob-qa
     - flowgraphs / RF chain / SEM → oob-dev-rf
     - Console SPA / journeys → oob-dev-console
     - board/process/gate items → oob-scrum or oob-pm -->

- [ ] oob-architect
- [ ] oob-qa
- [ ] oob-dev-rf
- [ ] oob-dev-console
- [ ] oob-dev-core (cross-review)
- [ ] oob-scrum / oob-pm

## Merge readiness

- [ ] All required role reviews **completed on the head commit** (each names the SHA it reviewed; no open change requests)
- [ ] Ready for Marshal to merge — his merge is the human approval
- [ ] Definition of done (AGENTS.md §12) met

<!-- Agents do not approve, merge, or self-review. Marshal merges. -->
