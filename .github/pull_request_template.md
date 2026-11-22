<!--
Oob pull request — every declared feature requires the oob-qa (bugs) and
oob-architect (drift) reviews, current per AGENTS.md §6 rule 3, then
Marshal merges; his merge is the human approval (AGENTS.md §6). Fill
every section; an incomplete PR is not reviewable. Delete nothing from
this template.
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

<!-- AGENTS.md §6 rule 2: only oob-qa (bugs + fixes) and oob-architect
     (drift) review and comment on PRs. Neither may review a PR containing
     commits it authored. Name the authoring role and the issue. -->

- Authoring role (subagent): <!-- oob-dev-core / oob-dev-rf / oob-dev-console / oob-architect / … -->
- Issue: <!-- #NN -->
- [ ] oob-qa — bug review at head SHA, no findings standing (or the architect's combined review, if oob-qa authored this PR)
- [ ] oob-architect — drift review at head SHA, no findings standing (or QA's combined review, if oob-architect authored this PR)
- Commits without an `Oob-Role:` trailer, excluding merge commits (`<sha> → <role or Marshal>`): <!-- "none" -->

## Disputed findings

<!-- AGENTS.md §6 rule 2: the authoring role contests a review finding
here (finding ID + reason), never as a PR comment. The reviewer withdraws
or holds it at its next review (editing this section triggers that
re-review). Only Marshal can waive a held finding, by writing
"waived by Marshal" against it here before merging. Agents never write or
edit a waiver line, except the orchestrator recording Marshal's waiver
quoted verbatim with the date (AGENTS.md §6 rule 2, §11). Write "none" if
none. -->

## Merge readiness

- [ ] Required reviews (AGENTS.md §6 rule 2) **completed and current** (rule 3: each names the head SHA, or the pre-merge head when only main-only merges followed; no findings standing, or waived by Marshal)
- Main-only merges since the last review (orchestrator, §6 rule 3): <!-- "none", or "<sha>… — diff against main unchanged" -->
- [ ] oob-po acceptance recorded on the linked issue (AGENTS.md §14 step 8) — or n/a for Marshal's direct request with no issue (his merge is the acceptance, §12)
- [ ] Ready for Marshal to merge — his merge is the human approval
- [ ] Definition of done (AGENTS.md §12) met

<!-- Agents do not approve, merge, or self-review. Marshal merges. -->
