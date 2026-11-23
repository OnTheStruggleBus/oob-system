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
  skips them (`historical` in `ops/docs-facts.toml`) instead of
  allowlisting each quote.
- `facts.yaml` is read with a small, strict YAML-subset loader so the
  check stays stdlib-only; anything outside the subset is a loud error.
  The subset is written in the `facts.yaml` header; the loader goes when
  schema CI brings uv/pydantic in.
- New CI jobs become required checks only after they have reported green
  once — a required check that never reports blocks every merge. For the
  same reason a required job's display name is final from day one:
  `openapi drift` is a stub inside, not in its name.

**Review round (oob-qa QA-1…5, oob-architect ARCH-1…6).** What the first
cut missed:

- A checker that is quietly lenient is worse than none. An allowlist entry
  with no `contains` exempted a whole file, and a typo in a scan glob
  silently stopped scanning. Both now fail: every `[[allow]]` field is
  required and checked, and every glob must match a file.
- "Broken links fail the build" has to cover every file the claim covers.
  The root `README.md`, `AGENTS.md` and the role files are outside the
  book, so `ops/docs-anchors.py` now checks them at the source, using
  GitHub's heading-slug rule for anchors. The first cut stripped every
  underscore and so rejected valid anchors like `#the-oob_msg-crate`;
  only emphasis underscores at word edges are stripped now (QA-6).
- F6-13 was raised over stale "pending disposition" text in crate doc
  comments, so the facts check now scans `crates/**/*.rs` doc comments and
  has a `disposition` rule. Its first run found one: the Hardware List's
  rev-2 note still called r4 F4-1 undispositioned. A dated "since
  accepted" note was added.
- Checker configuration is not a fact. It moved to `ops/docs-facts.toml`
  (stdlib `tomllib`), and the superseded brand became a real fact,
  `naming.superseded_brand`.
- The doc site is kept: the `mdbook (link check)` job uploads
  `target/book/html` as the `doc-site-<sha>` artifact, kept for 90 days.
  Attaching it to a gate tag stays Marshal's manual step at gate review.
