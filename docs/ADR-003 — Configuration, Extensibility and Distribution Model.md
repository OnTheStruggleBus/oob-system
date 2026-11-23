# ADR-003 — Configuration, Extensibility and Distribution Model

- **Status:** Accepted — Marshal, 2026-11-22
- **Date:** 2026-11-22
- **Deciders:** Marshal (decision); oob-architect (author role, recording
  Marshal's amendment)
- **Addresses:** r6 F6-4 (accepted **with amendment**: this ADR *is* the
  amended delta and replaces the one proposed in r6). Amends the context of
  r6 F6-5, and with it r5 F5-12.
- **Builds on:** ADR-001 rev 8 (what to build); ADR-002 (how the running
  system is shaped). This ADR decides *who configures the running system,
  where that configuration lives, and what the project distributes*.

## Context

r6 F6-4 found two write paths into one "single source of truth".
AGENTS.md §5/§6 said `ops/units.yaml` and `ops/lineup.yaml` change only
through git, by a pull request Marshal merges after role reviews. The
Console spec said every UI edit is a commit, and its enrollment wizard
committed to `units.yaml`. Branch protection makes the second path
impossible, and a local clone would silently fork the source of truth.

The delta r6 proposed kept git in charge of design-time configuration
(the Console would *open PRs*) and exported operational state to a
machine-written `ops-state` branch. Marshal accepted the finding but
amended its delta. In his words (2026-11-22):

> "This needs to be self contained fully user configurable"

> "Shouldn't open PRs to add boxes beyond new box _types_"

> "Think of this like BOSS it should be fully user configurable and meet
> the ability or end users to do most things or fork the project and do
> other things. It is a faithful not infiging recreation of this service
> for hobbiests and archival preservation"

On distribution he chose **ship sample content**, a **community pack
format**, and **firmware import tooling**. He did **not** choose "allow
user content", so the no-broadcaster-content rule stands: playout loops
and guide listings remain self-authored.

Forces at play:

- **Who the user is.** Oob is run by hobbyists and archivists on their own
  benches, not only by the project's maintainer. They will never hold
  write access to this repository and should not need it.
- **Box-as-oracle still governs protocol truth.** The `verified` status
  must keep meaning "box-accepted, capture-referenced, reviewed".
- **Safety and legality.** The closed-plant doctrine, the secrets tier and
  the typed-confirmation rule must hold for every user, not only for
  Marshal's bench.
- **The DB is now the installation.** Once configuration is runtime state,
  losing the database means losing the install (F6-5).

## Decision

### 1. Product framing

Oob is an **operator system** that end users (hobbyists, archivists) run
themselves, like an operations/support system ("like BOSS"). Everything
needed to operate a headend is configurable **in the running system**.
Anything beyond that, users fork.

PRs, gates and role reviews govern **developing Oob**, never **operating
it**. The running system needs no git and no GitHub, and it opens no PRs.

### 2. Three layers of change

| Layer | What it covers | Where it lives | How it changes |
| --- | --- | --- | --- |
| **1. Operating the headend** | boxes (enroll, retire), the unit operational record, lineup, PID plan, traffic-class budgets (F6-3), entitlements, campaigns, guide listings, the brand skin slot | oob-dac's store (one sqlite file, F6-5) | Console or API, at runtime |
| **2. Box types and message templates** | user-definable data profiles: capability profile, hunt list, firmware/CDL handling settings, message templates | oob-dac's store; created or imported in the Console, exportable | Console or API, at runtime; import/export as community packs |
| **3. Oob code** | new protocol behavior, encoders, subsystems | this repository | upstream PR through the gate process, or a fork |

Layer 1 rules:

- **Server-validated on every save.** The schema checks and the
  lineup ↔ VCM ↔ playout ↔ SEM consistency check move from CI into
  oob-dac's save path. CI keeps running the same validators over the seed
  data (§6).
- **Revision history and rollback** for every configuration object, in
  the store.
- **Append-only journal** of every change (who, when, what, before/after),
  the same audit path F6-10 writes commands to.

Layer 2 rule: **adding a box of a known type never needs a PR.** A new
box *type* that the shipped profiles do not cover can be defined by the
user as a `user-defined` profile; upstreaming it is optional (§3).

### 3. Trust labels

- Upstream profiles and templates keep the evidence statuses
  `documented` / `hypothesized` / `verified`. `verified` still requires a
  capture referenced in a **reviewed upstream PR** (F6-6, F6-7).
- Anything a user creates or imports is labelled **`user-defined`**. The
  Console shows the label everywhere the item is used.
- Users can contribute upstream by PR with their evidence; promotion to
  `verified` follows the normal evidence rule.

### 4. Distribution

**Oob ships:**

- the code and the docs;
- **self-authored sample content** so a fresh install works out of the
  box: demo MPEG-2 MP@ML 480i loops, sample listings, a demo lineup, and
  the upstream box-type profiles;
- a **community pack** import/export format for box profiles, message
  templates, lineups, guide themes and brand skins. A pack **never**
  contains firmware, keys or box-identifying data: the exporter strips
  them, and refuses to export when it cannot;
- **firmware import tooling** to dump, verify and import firmware from the
  user's *own* boxes into a local encrypted store, under the existing
  safety rules: sacrificial box first, and a two-route verified restore
  (D0, r3 F3-3 / r4 F4-3) before any write-back.

**Oob never ships:** firmware images, flash dumps, key material
(including the FPK constant), serials or Unit Addresses, or broadcaster
content. Box-identifying data stays local to an installation unless the
user explicitly exports it.

"Community pack" is a **format**, not a component; it adds no name to the
AGENTS.md §4 set.

### 5. Guardrails stay server-side

Full configurability is safe only because oob-dac refuses unsafe
configuration for every client: Console, scripts, forks' front-ends,
agents. On save it rejects at least:

- a lineup above 860 MHz (the box tuner ceiling);
- an STD/HRC mismatch across SEM plan, lineup and VCM (r4 F4-8);
- an EMM provider ID other than 0x0000/0x0001 on the OOB stream;
- traffic-class floors and caps that starve C0 system (F6-3).

Destructive operations need the F6-10 policy layer: owner role, a
short-lived confirmation token bound to (operation, target serial),
dry-run as the default for bulk operations, sacrificial-only CDL until G4
passes, and every command in the audit log.

### 6. Repository consequences

- `ops/lineup.yaml` and `ops/units.yaml` become **seed / example install
  data** plus the **upstream box-type catalog**. **No box instance is
  committed to git.** The install loads the seed into the store on first
  run; after that the store is authoritative.
- **Optional YAML export** of runtime state, for backup and diffing, is a
  local feature. It is never a gate and never pushes anywhere.
- The **`ops-state` branch** idea from r6 F6-4's original delta is
  **replaced** by this local export.
- Because the database is now the installation, F6-5 adds **one-click full
  backup / restore** and **whole-install export** (amending F5-12: the
  unit DB is no longer "derived, excluded from backup").
- **AGENTS.md §6 rule 2 "Scope" note** about Console-opened config PRs is
  resolved: the running system opens no PRs, so no rule is needed.

## Options Considered

### Option A — Runtime-configurable operator system; git for code only (SELECTED)

**Pros:** matches who the users are; enrollment and lineup edits are
everyday actions with no repository access; one source of truth per
installation (the store) with history, journal and rollback; the gate
process stays focused on the code it was built for.

**Cons:** validation and guardrails must be built into oob-dac rather than
borrowed from CI; backup becomes a product feature, not an afterthought;
`user-defined` content needs clear labelling so it is never mistaken for
`verified`.

### Option B — r6's proposed delta: design-time config PR-gated, operational state exported to an `ops-state` branch

**Pros:** every design-time change gets a review; diffs live in git.

**Cons:** every user needs a GitHub fork and an approval loop to change a
lineup; the running system depends on git hosting; "PRs to add boxes"
contradicts Marshal's direction. Superseded by Marshal's amendment.

### Option C — Everything in git (the original AGENTS.md §5)

**Cons:** impossible under branch protection for a daemon, and it forks
the source of truth when run against a local clone (the F6-4 attack).

## Consequences

**Easier:**

- A hobbyist can stand up, enroll and run a plant from the Console alone.
- Forks are a supported path, not a workaround.
- The upstream repository stays free of any one installation's fleet.

**Harder:**

- oob-dac needs a configuration store with revisions, a journal, and the
  validators that used to live only in CI.
- Backup/restore and whole-install export must work and be drilled.
- The pack exporter must reliably strip or refuse identifying data.

**To revisit:**

- the community pack format's schema and versioning (spec to follow);
- how upstream profile updates merge with a user's modified copy.

## Action Items

1. [x] Marshal: decision recorded (2026-11-22).
2. [x] oob-architect: rewrite the Console spec configuration sections to
   this model (landed with this ADR: principle 2, §2 architecture,
   enrollment wizard, Discovery Lab and template editor, §4.5
   validation, §4.10 class budgets, §4.14, the M6 journeys).
3. [ ] oob-architect: follow-up sweep of the Reproduction Plan §5 repo
   tree and ADR-001's "config-as-code" wording.
4. [ ] oob-dev-core (Slice D): oob-dac configuration store with revision
   history, rollback, append-only journal, and save-path validators
   (schema, lineup ↔ VCM ↔ playout ↔ SEM, §5 guardrails); one-click
   backup/restore and whole-install export (F6-5).
5. [ ] oob-po: issues for the community pack format spec, the firmware
   import tooling spec, and the seed/sample-content set.
6. [ ] oob-dev-core: convert `ops/lineup.yaml` / `ops/units.yaml` to seed
   data plus the upstream box-type catalog when they are first created;
   no box instance in git.
