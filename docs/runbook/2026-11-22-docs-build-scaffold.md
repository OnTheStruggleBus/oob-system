# 2026-11-22 — Docs build scaffold (G0.5, r5 F5-10)

**Attempted.** Turn `docs/` into an mdBook that CI builds on every commit,
with broken internal links failing the build, a table of contents that is
generated from the Overview's document map rather than hand-kept, a
prose-vs-`docs/facts.yaml` check (r6 F6-13), and a wired-but-trivial
OpenAPI drift job for Slice D. Tools pinned: mdBook 0.4.52,
mdbook-linkcheck 0.7.7 (via the already-pinned `taiki-e/install-action`,
which sha256-verifies the binaries).

**What broke.**

- *Percent-encoded SUMMARY links.* Encoding the filenames fully
  (`%E2%80%94` for the em-dash) made mdBook fail with "chapter file not
  found": mdBook 0.4 decodes `%20` and nothing else. The generator now
  escapes spaces only; em-dashes, `&`, `,` and balanced `()` stay literal.
- *Linkcheck heuristics.* With warnings promoted to errors the build died
  on six "potential incomplete link" hits — all prose, none links:
  `1. [x]` decision checkboxes (ADR-002, ADR-003 ×2), `[P3-5 C]` (Hardware
  List), `[VHS]` in a video title (Reproduction Plan), `control[s]` in a
  quotation (VOD & PPV spec). Warnings now stay warnings; missing link
  targets are still hard errors. No prose was changed for them.
- *Anchors are not checked by mdbook-linkcheck 0.7.7.* A probe link to
  `#no-such-heading` passed. `ops/docs-anchors.py` now checks every
  relative `href` and `#fragment` against the ids in the built HTML, so it
  uses mdBook's own slugs rather than re-implementing them.
- *README chapters 404 from prose.* mdBook's default `index` preprocessor
  renames `runbook/README.md` to `index.html` but leaves prose links to
  `README.md` pointing at `README.html`; linkcheck could not see it, the
  anchor check did. Default preprocessors are off (the `links`
  preprocessor is re-enabled), so `README.md` links behave the same on
  GitHub and in the book.

**Genuinely broken internal links found in `docs/`:** none — the corpus
had no internal Markdown links before this change.

**Lessons.**

- Probe the checker before trusting it: plant one broken file link, one
  broken anchor and one good anchor, and watch which fail.
- The review records quote the prose they found wrong, so the facts check
  skips them (`facts_check.historical`) instead of allowlisting each quote.
- `facts.yaml` is read with a small, strict YAML-subset loader so the
  check stays stdlib-only; anything outside the subset is a loud error.
- New CI jobs become required checks only after they have reported green
  once — a required check that never reports blocks every merge.
