# ADR-004 — Dependency Licensing Tiers and External-Component Packaging

- **Status:** Proposed — oob-architect, 2026-11-27 (for Marshal's
  acceptance; ADRs are owner-accepted, AGENTS.md §14 step 10)
- **Date:** 2026-11-27
- **Deciders:** Marshal (decision); oob-architect (author role)
- **Addresses:** issue #83 (the registered "Oob open-source component
  survey", `/Users/Shared/Oob open-source component survey.md`, dated
  2026-11-26 — referenced by path, not vendored, per AGENTS.md §5 and the
  #83 no-commit rule). Makes the survey's A/B/C tier screen normative and
  decides the packaging/compliance obligations for components Oob ships or
  deploys that are outside GPL-3.0-only.
- **Builds on:** ADR-002 (runtime topology — the process boundaries this
  ADR relies on already exist: `oob-playout` is a separate supervisor
  process, the GNU Radio flowgraph is a host `systemd` service, lab tooling
  and the Console are API clients only); ADR-003 §4 (distribution — what
  Oob ships and never ships). This ADR decides *which licenses may enter
  the compiled Rust binary, which may only be invoked across a process
  boundary, and what obligations attach to each external tool Oob ships or
  deploys*.

## Context

Oob's own outbound license is **GPL-3.0-only**, settled in-tree and
consistent: the root `LICENSE` file carries the full GNU GPL v3 text (674
lines, the FSF canonical text), and `[workspace.package]
license = "GPL-3.0-only"` in the root `Cargo.toml` is inherited by every
crate via `license.workspace = true`; `README.md` points to the `LICENSE`
file. Because Oob's outbound terms are fully materialized in-tree, the
A/B/C tier obligations below are unblocked on that condition.

The 2026-11-26 component survey screened the open-source components Oob's
workstreams touch (TS mux, MPEG-2 video/audio, playout, DSM-CC/carousels,
SDR/RF, admin/scheduling, EAS, DOCSIS, CPU32/68K emulation, MoCA) and
sorted each into three tiers. Many of the most useful components —
FFmpeg (LGPL-2.1-or-later), ffplayout and gr-dtv (GPL-3.0-or-later),
OpenCaster, HackRF tooling, `mptsd`, `docsis`, `libdvbcsa`
(GPL-2.0 family) — are **not** licensed GPL-3.0-only. Marshal's direction
(issue #83): "make sure we are doing the proper packaging options for items
that need to be included but are outside GPL 3."

The licensing question is not abstract. Combining GPL and non-GPL code in
one program is governed by whether the result is a single *combined /
derivative* work or a *mere aggregation* of separate works:

- A library linked into the GPL-3.0-only binary — static link, dynamic
  link, C FFI, a Rust `-sys` binding, or loading the library into the Rust
  process — forms **one combined work**, which must then be distributable
  under GPL-3.0-only. An incompatible license (notably **GPL-2.0-only**,
  which has no "or later" clause) makes that combination **undistributable**.
- An independent executable that Oob merely *invokes* across a process,
  CLI, pipe, or socket boundary, with its own license, data, and address
  space, is **mere aggregation**: each program keeps its own license, and
  Oob's GPL-3.0-only terms do not reach into the external tool (nor the
  tool's terms into Oob).

Forces at play:

- **Faithful reproduction needs these tools.** FFmpeg does the MPEG-2
  encode; gr-dtv + HackRF tooling drive the J.83B bench path; OpenCaster
  and TSDuck author and inspect carousels. Excluding every copyleft tool
  would gut the pipeline.
- **Oob must stay cleanly GPL-3.0-only and redistributable.** The
  compiled binary cannot absorb a license it cannot satisfy.
- **A subprocess boundary is necessary but not sufficient.** The survey is
  explicit: aggregation keeps the licenses separate, but it does **not**
  discharge *redistribution* obligations — a GPL/LGPL executable Oob
  bundles or ships still carries notice and corresponding-source duties.
- **ADR-003 already decided what Oob ships.** Oob ships code, docs, and
  self-authored sample content; it never ships firmware, keys, or
  broadcaster content. External tools are the operator's own install. This
  ADR must reconcile the packaging rules with that boundary, not reopen it.
- **cargo-deny already enforces the in-process half.** `deny.toml`'s
  license allow-list admits only permissive licenses plus Oob's own
  GPL-3.0 family, so a copyleft Rust crate entering the dependency graph
  already fails CI. The decision is whether to state that as policy and
  whether to tighten it further.

## Decision

### 1. Three license tiers, made normative

The survey's A/B/C screen is adopted as binding project policy. Every
third-party component Oob depends on, ships, or deploys is classified:

| Tier | Licenses | Integration rule |
| --- | --- | --- |
| **A — permissive** | MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, MPL-2.0, Unicode-3.0, CC0-1.0 | **In-process allowed.** May be a compiled Rust dependency, adapted, or statically linked into the GPL-3.0-only binary. Capture the notice/attribution the license requires. |
| **B — copyleft / LGPL / GPL** | LGPL-2.1/3.0, GPL-2.0, GPL-3.0 (third-party), and their `-or-later` forms | **External process / executable ONLY.** Never static-linked, dynamic-linked, FFI-bound, `-sys`-wrapped, or loaded into the GPL-3.0-only Rust binary. Invoked across a process/CLI/socket boundary as a separate program (mere aggregation). Packaging obligations in §2 apply. |
| **C — reference-only / AVOID** | AGPL (9eye, LibreTime, OBServer, SDR-CMTS); otherwise-permissive-but-unlisted (SoapySDR BSL-1.0); unverified/incompatible (`tstools` MPL-1.1, `dsmcc-mhp-tools`); and the barred ecosystems below | **Excluded from the product.** May be read as reference where the survey allows, never adopted as code or shipped. **Card-sharing, subscriber-authorization-injection, and payware-bypass ecosystems are barred entirely — not even as references** (survey; AGENTS.md §11). |

Tier assignment follows the survey's verdicts. `verified`/`watch`/`adopt`
status in the survey is an adoption-readiness note, not a tier change; a
"watch" Tier-B tool is still Tier-B for integration purposes.

**The aggregation vs derivative line (the rule the tiers encode):** Oob
(GPL-3.0-only) invoking an external tool over a process/CLI/socket boundary
is **mere aggregation** — each program keeps its own license. In-process
linking of a non-GPL-3-compatible library would create a **combined work**
and is **forbidden**. This is why Tier B is "external process only": it is
the boundary that keeps the aggregation clean. ADR-002's topology already
puts every heavyweight media/RF tool on the far side of such a boundary
(`oob-playout` supervisor process; the flowgraph `systemd` service), so
this rule confirms the existing architecture rather than reshaping it.

### 2. Packaging and compliance for shipped/deployed non-GPL-3 components

This is the core of the ADR. For **every** Tier-B external tool that Oob
**ships, bundles, or deploys** (a pinned Podman image, a documented install
step, a container Oob orchestrates), the following obligations are
concrete and mandatory:

1. **Separate executable across a process boundary.** The tool runs as its
   own process/container — FFmpeg behind `ffmpeg`/`ffprobe`; gr-dtv in the
   flowgraph `systemd` service; OpenCaster, `docsis`, dvbsnoop, `mptsd`,
   MAME, etc. as standalone CLIs. **No** in-process linkage of any kind
   (static, dynamic, FFI, `-sys`, library load). This keeps the
   combination a mere aggregation.

2. **Pin the exact build by digest.** Per AGENTS.md §10 (Podman images
   pinned by digest) and ADR-002's deployment split, every shipped/deployed
   external tool is referenced by an immutable digest, not a floating tag.
   The pinned digest identifies precisely which build's obligations are
   being discharged.

3. **Capture license text + NOTICE.** For each shipped/deployed external
   tool, store its license text and any upstream NOTICE/attribution file
   alongside the deployment manifest (a `licenses/` or `third-party/`
   directory in the deploy artifact, referenced from the install docs). The
   survey's per-file caveats are honored: where a project mixes licenses
   (MJPEG Tools' GPLv2 `COPYING` plus the MPEG Software Simulation Group
   notice; Aften's Xiph BSD files; MAME's many BSD-3-Clause files), the
   notice of the **build actually shipped** is captured, not a generic
   top-level label.

4. **Provide the corresponding-source offer where the license requires it.**
   GPL-2.0/3.0 and LGPL carry a corresponding-source obligation to anyone
   who receives the binary. Because Oob **does not ship these binaries**
   (§3 below — the operator installs them from upstream under the tool's
   own license), Oob's duty is to **name the exact upstream source and
   pinned build** (repository URL + version/commit/digest) in the deploy
   documentation, so the corresponding source is unambiguously locatable.
   **If a future deployment artifact ever does bundle a Tier-B binary**
   (e.g. a convenience image), that artifact must carry the written
   corresponding-source offer (or the source itself) for the exact build —
   this is a release-engineering gate on any such artifact, flagged to
   Marshal as an open question (Consequences).

5. **LGPL tools preserve the user's relink right.** For LGPL components
   (FFmpeg's default build, TwoLAME, GStreamer core, MLT), either keep them
   as **external executables** (the default — FFmpeg is invoked as a CLI,
   never bound as `libav*`), or, if a shared library is ever used, keep it
   **dynamically linked and replaceable** so the user can substitute their
   own build (LGPL §6). **Never static-link an LGPL library into the
   GPL-3.0-only binary** in a way that defeats the relink right. In
   practice Oob's rule is the stronger one: LGPL media libraries are
   reached only through their CLI front-ends, so no linkage question
   arises.

6. **GPL-2.0-only is incompatible even across a link — external process is
   the only lawful integration.** `mptsd` (GPL-2.0-only in its inspected
   entry point) and `libdvbcsa` (GPL-2.0-or-later in release source) sit in
   the GPL-2 family. **GPL-2.0-only cannot be combined with Oob's
   GPL-3.0-only by any linking**, because neither license permits
   relicensing to the other. The external-process boundary is therefore not
   a convenience but the **only** lawful way to use them at all:
   - `mptsd` — Tier B, external comparison mux only; never linked.
   - **`libdvbcsa` — Tier C, reference-only: no link, no vendor, no port,
     no copy of code, tables, or fixtures** (survey; AGENTS.md §2 discovery
     order and §5 secrets tier reinforce that DCII CA work is
     capture-and-spec-driven, not borrowed). This bars even the "external
     process" use that other Tier-B tools get. Card-sharing and
     payware-bypass ecosystems around it are barred entirely (§1 Tier C).

### 3. Reconciliation with ADR-003 §4 distribution

ADR-003 §4 stands unchanged; this ADR reads onto it:

- **Oob ships** code, docs, and self-authored sample content (demo MPEG-2
  MP@ML 480i loops, sample listings, a demo lineup, upstream box-type
  profiles), the community-pack import/export format, and firmware import
  tooling for the user's own boxes. **It does not ship third-party
  executables, firmware, or key material.**
- **External Tier-B tools are the operator's own install**, obtained by the
  operator from upstream under each tool's own license. Oob provides the
  **pinned reference** (digest, upstream URL, version) and the deploy
  manifest that wires them in; it does not redistribute their binaries by
  default. This is what keeps Oob's own distribution cleanly GPL-3.0-only
  and free of other projects' redistribution obligations.
- **Community packs** carry only self-authored config/templates/themes
  (message templates, lineups, guide themes, brand skins), per ADR-003 §4.
  **A pack never contains a third-party executable, firmware, key material,
  or box-identifying data** — the exporter already strips/refuses those.
  This ADR adds no exception: packs are content, not tooling.
- The result: the only GPL-3.0-only redistributable Oob ships is **Oob
  itself**; every Tier-B tool crosses to the user as the user's own
  upstream install, across the aggregation boundary.

### 4. cargo-deny alignment (policy stated; implementation deferred)

`deny.toml`'s current `[licenses].allow` set is:
`Apache-2.0, BSD-2-Clause, BSD-3-Clause, CC0-1.0, GPL-3.0-only,
GPL-3.0-or-later, ISC, MIT, MPL-2.0, Unicode-3.0, Zlib`.

- **Already aligned.** The in-process Rust dependency graph that cargo-deny
  governs permits only permissive licenses plus Oob's own GPL-3.0 family.
  LGPL, GPL-2.0-only/or-later, and AGPL are **not** admitted, so Tier B's
  "never linked into the Rust process" is **already enforced for Rust
  crates** — a copyleft crate entering the graph fails the `cargo-deny` CI
  job today.
- **Scope limit.** cargo-deny governs only the compiled Rust dependency
  graph. Tier-B *external executables* (FFmpeg, ffplayout, gr-dtv,
  OpenCaster, HackRF tooling) live outside that graph; their obligations
  are the §2 packaging/distribution concern, not a cargo-deny concern.
- **Decision — keep GPL-3.0 in the crate allow-list, bar third-party
  copyleft (unchanged today; a follow-up PR documents it).** Oob's own
  crates are GPL-3.0-only and must remain admissible, so `GPL-3.0-only`
  (and `GPL-3.0-or-later`, harmless and compatible) stay in the allow-list.
  The allow-list already bars third-party copyleft by omission of LGPL/
  GPL-2/AGPL. A later cargo-deny PR (oob-dev-core / ops) should **(a)**
  record the A/B/C → `deny.toml` mapping as a commented policy block, and
  **(b)** optionally add a `[licenses.exceptions]`-style guard so that only
  Oob's **own** crates may carry the GPL-3.0 licenses while any *external*
  crate is held to permissive-only — closing the theoretical gap where a
  third-party `GPL-3.0-or-later` crate would currently pass. Whether to add
  that tightening is the open question in Consequences; the policy
  (third-party = permissive only; own crates = GPL-3.0-only) is decided
  here regardless.
- **CI-check rule.** If that cargo-deny PR introduces a **new** CI check
  (e.g. a dedicated license-policy job distinct from the existing
  `cargo-deny`), it is added to the AGENTS.md §6 rule-6 required-checks
  list **in the same PR that introduces it** — never before (a required
  check that never reports blocks every merge).

## Options Considered

### Option A — Normative A/B/C tiers + external-process packaging for Tier B (SELECTED)

**Pros:** keeps Oob cleanly GPL-3.0-only and redistributable; lets the
pipeline use the best-in-class tools (FFmpeg, gr-dtv, OpenCaster) lawfully;
rides the process boundaries ADR-002 already drew; the in-process half is
already enforced by cargo-deny. Each obligation (digest pin, notice
capture, source reference) is concrete and auditable.

**Cons:** per-tool notice/source capture is ongoing release-engineering
work; the "operator installs it themselves" model asks more of the user
than a bundled image would; a future convenience image that bundles Tier-B
binaries would reopen the corresponding-source duty.

### Option B — Bundle Tier-B tools as pinned images Oob ships

**Pros:** one-command install; the operator gets a working plant without
sourcing FFmpeg/gr-dtv/OpenCaster themselves.

**Cons:** Oob would then **redistribute** GPL/LGPL binaries and inherit
their full corresponding-source and notice obligations for every shipped
build; it also cuts against ADR-003 §4 ("Oob never ships third-party
executables"). Rejected as the default; may return as an **optional,
fully-compliant** convenience artifact only if Marshal wants it, with the
source offer attached (Consequences open question).

### Option C — Permissive-only; exclude all copyleft tools

**Pros:** no copyleft obligations anywhere; the simplest compliance story.

**Cons:** removes FFmpeg, gr-dtv, ffplayout, OpenCaster, HackRF tooling —
the survey found no permissive replacements of comparable maturity for the
MPEG-2 encode, the J.83B transmit path, or carousel authoring. Guts the
reproduction. Rejected.

## Consequences

**Easier:**

- A single, auditable rule for every dependency: Tier A links, Tier B is a
  separate process, Tier C is reference-only or barred.
- Compliance obligations are explicit and per-tool, not folklore.
- The in-process half is machine-enforced (cargo-deny), so the risk
  concentrates on external-tool packaging, which §2 makes a checklist.

**Harder:**

- Release engineering must maintain the license-text/NOTICE capture and the
  pinned-source references for each shipped/deployed external tool, and keep
  them current with each digest bump.
- The optional cargo-deny tightening (own-crates-only GPL-3.0) needs design
  so it does not break workspace-member resolution.

**To revisit / open questions for Marshal (ADR acceptance is Marshal's):**

1. **cargo-deny tightening.** Keep today's allow-list (GPL-3.0 admitted for
   any crate, third-party copyleft barred by omission), or add the
   own-crates-only guard so external crates are held to permissive-only?
   Recommended: add the guard in the follow-up PR; deciding here is
   optional.
2. **Optional bundled convenience image.** Does Marshal want an opt-in,
   fully-compliant image that bundles Tier-B tools (with the written
   corresponding-source offer attached), or stay with operator-sourced
   installs only? Default stays operator-sourced.

(Oob's own outbound license is already settled in-tree — root `LICENSE`
GPL-3.0 text + workspace `Cargo.toml` — so no license-text gap blocks the
§2 obligations.)

## Action Items

1. [ ] Marshal: accept, amend, or reject this ADR (gates the cargo-deny
   policy PR and the deploy-manifest license-capture work).
2. [ ] oob-dev-core / ops: cargo-deny policy PR — the A/B/C → `deny.toml`
   mapping as a commented policy block, confirm the allow-list matches the
   Tier-A set plus Oob's GPL-3.0 family, and (open question 1) the optional
   own-crates-only GPL-3.0 guard. Any new CI check joins the AGENTS.md §6
   rule-6 required list in the same PR.
3. [ ] ops: for each shipped/deployed Tier-B tool, the deploy manifest
   captures license text + NOTICE and the pinned upstream source reference
   (URL + version/digest), per §2, when that tool is first wired in.
4. [ ] oob-po: adoption stories for the per-tool license-capture checklist
   and the cargo-deny policy block.
