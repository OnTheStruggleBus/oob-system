# oob-dac configuration store

This page describes how an operator's configuration lives in oob-dac's store
(issue #25; ADR-003 §2 and §6; r6 F6-3, F6-4 as amended, F6-5). It is
crate-internal and tracks `crates/oob-dac/src/config/`. Its kind, reason,
rule and journal-command lists are drift-tested against the code
(`the_docs_page_carries_every_kind_reason_and_structural_rule`).

Everything here is **box-free**: the evidence is deterministic tests, and no
behavior is claimed `verified` against a box.

## What is configuration

Operating a headend is ADR-003 **Layer 1**: runtime configuration in
oob-dac's store, changed through the [command layer](oob-dac-command.md) and,
later, the REST API (#27) and the Console. The running system needs no git
and opens no PRs.

| Kind | Document schema | Key | Head | Applied to (by the daemon, #29) |
| --- | --- | --- | --- | --- |
| `lineup` | 1 | singleton | latest revision | SEM and playout config (later slices) |
| `pid_plan` | 1 | singleton | latest revision | the PSI assemblers (#17) |
| `class_budgets` | 1 | singleton | latest revision | the mux's live budget (hot) |
| `mux_settings` | 1 | singleton | latest revision | pacer fields hot (#49); queue fields restart-required |
| `unit` | 1 | terminal ID | the `terminal` row, with a revision per change | targeting (#22) reads the row |

The drift test reads the first two columns as `` | `kind` | schema | ``.

`mux_settings` is the fifth kind oob-po added on #25: the pacer's
`packet_rate_bps`, `max_pull` and `pull_budget_us`, and the queue sizes
`queue_depth`, `max_carousels` and `outcome_capacity`.

**Not runtime-configurable** (Layer 3, code or upstream data): the list of
kinds itself, the document schemas and their versions, the validator's rule
IDs, the immutable-identity list below, and the **box-type catalog**. There is
no kind, command, API or method that adds a box type at runtime (Marshal's
QA-6 ruling); the catalog is read-only and ships with the install. Process
bootstrap values (the seam socket path and the like, #29) are not
configuration objects either. Entitlements, campaigns, listings, the brand
skin slot and templates arrive with their own issues.

## Documents

Each kind has a serde document of oob-dac's own (`config::doc`): integers,
strings and booleans only, so a document always parses and a bad value is
refused by a **named rule** from the validator rather than by a parse error.

- **No floats.** Frequencies are integer kHz (`freq_khz`, so the tuner
  ceiling is `freq_khz <= 860000`), rates are integer bit/s or kbit/s, and
  durations integer milliseconds or microseconds.
- **Unknown fields are refused**, so nothing an operator sends is dropped
  silently.
- **Canonical JSON**: keys sorted at every level, no whitespace. It is what is
  stored and what `doc_sha256` covers, so the digest is reproducible.
- **Schema versions.** A revision stored at an older document schema is
  upgraded **in memory** when it is read or rolled back to; the stored row is
  never rewritten. A newer one is refused (`doc-schema-too-new`).
- **Size cap**: 1 MiB of canonical JSON by default (an install setting), so a
  client cannot bloat a precious table (`doc-too-large`).

## Tables

Migration `0003_config` adds them (see [the store](oob-dac-store.md)).

- **`config_revision`** (Precious, append-only): one row per revision, keyed
  by `(kind, object_id, revision)`. `object_id` is empty for a singleton and
  the decimal terminal ID for a unit. Each row holds the canonical document,
  its SHA-256, its schema version, its origin (`save`, `rollback` or `seed`),
  the revision a rollback restored, the actor, the time, and `audit_seq`, the
  journal row written with it. Triggers abort every `UPDATE`, `DELETE` and
  overwriting insert (`REPLACE`), as on the other journals.
- **No head table.** An object's head is its highest revision, so there is no
  second copy to disagree with the history. Revision numbers are contiguous
  per object.
- **`terminal.retired_utc`**: a retired unit keeps its row, its ID (never
  reused) and its history.
- **`psi_table_version`** (Precious): the #17 hand-off. The daemon stores
  oob-mux's per-table PSI `version_number` and last-emitted bytes, so a
  restart continues the mod-32 count instead of re-emitting a changed table at
  version 0. **Pruning policy: none.** Rows are upserted and never deleted; a
  table that left the plan keeps its version (`retired`) so a re-add
  continues the count, and the set is bounded by the number of PSI table
  kinds. An explicit owner reset is the only future way to drop one.

## The journal

The journal is the existing precious `audit_log` (#21 built it as one audit
path for commands and configuration): there is no second journal table. Each
revision's row is written in the same transaction as the revision and carries
the actor, role, front door, target `kind[/terminal id]@revision`, outcome,
and the **full before and after documents**, so the journal is
self-contained. It never carries a unit address or serial: identity is not
part of the unit document.

| Journal command | Written by |
| --- | --- |
| `config.save` | a singleton save (and each item of a batch) |
| `config.rollback` | a rollback, of a singleton or a unit |
| `config.seed` | the first-run seed, actor `seed:<first 12 hex of the bundle digest>` |
| `unit.enroll` | a unit enrollment (revision 1) |
| `unit.update` | a change to a unit's operational record |
| `unit.retire` | a retire (before and after are `{"retired":…}`; no revision) |
| `unit.reinstate` | a reinstate (likewise) |

A refusal stores nothing, so its journal row is written in its own
transaction: the same command and target, outcome `refused:<rule,rule…>`, and
the refused proposal as `after`. The bus also writes its usual policy audit
row for every outcome (#23).

## The save sequence

Every change runs in **one** store write transaction (`BEGIN IMMEDIATE`; the
store has one writer, so saves serialize):

1. **Base check.** Every change names the head it edited
   (`BaseRevision::None` or `BaseRevision::At(n)`); there is no "whatever is
   there". A stale base is `conflict` (HTTP 409 and `If-Match` in #27), with
   the current head.
2. **Unchanged?** A document whose digest equals the head's writes nothing and
   returns `Unchanged`; the applier is not called.
3. **Candidate.** The current heads with the change applied, plus every unit
   record.
4. **Validate.** The mandatory validator sees the **whole candidate** and the
   change. A refusal (`config-refused`) stores nothing.
5. **Write**: the journal row, the `config_revision` row (`head + 1`), and the
   outbox event `ConfigSaved`, together. The commit is the outcome. A failure
   anywhere rolls all of it back (tested by fault injection after the
   revision insert).
6. **Apply**, after commit only. The daemon's applier (#29) returns
   `Applied`, `RestartRequired`, `Failed` with a reason, or `NotApplicable`,
   and the result is returned beside the revision ("saved, not applied:
   reason"). **A failed apply never un-saves**: the previous good
   configuration keeps running, and the daemon's head-versus-applied metrics
   show the divergence.

A **batch** saves several singletons validated as one candidate, all or
nothing; a key may appear once.

## Rollback

A rollback is **a save of an older revision's document**. It loads revision
`to`, upgrades it in memory if it is at an older schema, and runs the same
sequence with `origin = 'rollback'` and `rolled_back_to = to`. So:

- it creates a **new** revision equal to the target; history is never
  rewritten (rows 1 to n stay byte-identical, by test);
- a target that a later guardrail now refuses is **refused with named
  violations** and stores nothing: history never bypasses the guardrails;
- a rollback to a document equal to the head is `Unchanged`.

## Validation: the surface #26 implements

`config::check` defines the shapes and #26 implements the rules:
`ConfigSnapshot` (the four singleton documents, absent before seeding, a
read-only unit view and the box-type catalog), `Change` (save, batch,
rollback, unit enroll, update, retire, reinstate, seed), and the
`ConfigValidator` trait returning a `Refusal`: a never-empty list of
violations, each with a stable kebab-case `rule`, the objects it involves
(kind, terminal ID, JSON pointer), a detail text, and `hard` for an ADR-003 §5
guardrail. Both kinds block the save; there is no override flag. Absent
objects skip the rules that need them: incomplete is allowed, wrong is not.

**A validator is mandatory.** `ConfigAdmin::new` takes one and has no
default; no validator that admits everything is compiled outside tests. #25
ships `StructuralValidator`, which checks only the unit records a change
touches:

| Rule | Hard | Refuses |
| --- | --- | --- |
| `unknown-box-type` | yes | a box type that is not in the upstream catalog |
| `terminal-type` | no | a terminal type that is not one of the five Define Terminal types |
| `unit-date` | no | an install date that is not a `YYYY-MM-DD` calendar day |
| `unit-ip-address` | no | an IP address that is not IPv4 or IPv6 |

It checks **no guardrail and no consistency rule** (860 MHz, STD/HRC, EMM
provider ID, C0 floor): that is #26. The configuration store has no front
door until the REST adapter (#27), and #26 lands before it (design note §7),
so the window in which only structural checks run has no exposure.

## Units

A unit is the one deliberate **dual representation**. The `terminal` row is
the authoritative current record (it is precious, and targeting reads it);
every change to its operational record also writes a `unit` revision of the
same document in the same transaction. Only the configuration store writes
those columns (`Tx::create_terminal` is crate-internal; the policy layer's old
direct flag write is gone), and a randomized test of enroll, update, retire,
reinstate and rollback checks after every step that each row equals its
latest revision.

- **Immutable after enrollment:** terminal ID, unit address, check digits,
  serial. They are not in the unit document (an extra field is refused), so no
  update or rollback can touch them. A wrong identity is corrected by retiring
  the unit and enrolling it again, or by a forward migration.
- **The operational record** (the `unit` document): terminal type, box type,
  account number, On-Plant, install date, Activated, Autodiscovered, IP
  address, BSI name, downstream and upstream plant, sacrificial. The
  auto-registration date is system-set and kept across updates.
- **Enroll** (`unit-enroll`, the Define Terminal record, WireLink 660 "Add New
  Set-Top") writes the row, its `TerminalCreated` event, revision 1 and the
  journal row. It is a store record, not a claim that the box has it: the
  on-air Unit Creation is a separate terminal command, and enrollment order
  stays factory-init, unit-creation, connect (r4 F4-4). A unit is enrolled
  **non-sacrificial** (`sacrificial-at-enroll`); a duplicate unit address or
  serial is named (`duplicate-unit-address`, `duplicate-serial`).
- **Retire and reinstate** set or clear `retired_utc`; nothing is deleted. A
  retired unit is out of targeting (not in All or a range; a Single ID of one
  is an unknown terminal) and refuses updates and rollbacks (`unit-retired`)
  until it is reinstated.

## Policy

Configuration enters as `Command::Admin(AdminOp)` through `CommandBus::submit`,
so the [policy layer](oob-dac-policy.md) applies:

| Op | Needs |
| --- | --- |
| `config-save`, `config-save-batch`, `config-rollback`, `unit-enroll`, `unit-update` | the owner role (an operator is refused `owner-required`, a guest `guest-read-only`; the DAC's own timers cannot originate one) |
| `unit-update` or a unit `config-rollback` that turns a unit sacrificial | also the D-hard `mark-sacrificial` token bound to the terminal, through the REST or Lab door |
| `unit-retire`, `unit-reinstate`, unmarking sacrificial | the owner role only: store-only and reversible |

Whether a change marks a unit is decided against the **base revision** the
caller edited, which is immutable history; the save refuses a base that is no
longer the head, so the transition the policy priced is the one that commits.
An unknown base fails closed (a token is required). Lowering budgets or
changing the lineup needs no token: the guardrails protect those, not a typed
phrase.

## The first-run seed

`ConfigAdmin::seed_if_empty` (bootstrap only, the daemon's `--seed`, #29)
writes revision 1 of each singleton, validated as one candidate, **only when
none of the four singleton kinds has a revision**. It never overwrites a head,
and a partly configured store is not seeded over; the revisions are the
"seeded" marker, so no flag table exists. The bundle has **no units field**
and refuses unknown fields, so no box instance can come from git: after a
first-run seed every box-identifying table is empty and there is no unit
revision (tested; this closes #21's carried-over criterion). Until #30 ships
the real seed files, the bundle is the fixture
`crates/oob-dac/tests/fixtures/seed-bundle.json` (self-authored demo values).

## Boot

`ConfigAdmin::boot` hands every singleton head that today's validator accepts
to the applier. A head that a guardrail tightened since its save now refuses
is reported `Invalid` with its rules and **not applied**; the daemon serves
nothing for that subsystem, reports it in health and metrics, and waits for
the owner to save a fix. It never crash-loops. A fresh store reports every
kind `Unconfigured`.

## Reads

`Reader::config_head`, `Reader::config_revision` and `Reader::config_history`
(oldest first, paged after a revision, at most 500 per page) are the read
model for #27. Reads are not commands; the REST adapter decides who may read.

## Error codes

Every refusal has a stable code (`ConfigError::reason`), which is also
`CommandError::reason` for a configuration command:

| Code | Meaning |
| --- | --- |
| `conflict` | the base revision is not the head |
| `config-refused` | the validator refused; the rules are listed |
| `unknown-box-type` | the box type is not in the upstream catalog |
| `unknown-revision` | no such revision of the object |
| `unknown-object` | no such unit |
| `doc-too-large` | over the document size cap |
| `unit-retired` | the unit is retired |
| `wrong-kind` | key and document do not fit (a unit saved as a singleton, a key twice in a batch) |
| `sacrificial-at-enroll` | a unit is enrolled non-sacrificial |
| `doc-schema-too-new` | a stored document is newer than this build |
| `duplicate-unit-address` | another unit has that unit address |
| `duplicate-serial` | another unit has that serial |
| `terminal-id-exhausted` | all 24-bit terminal IDs are spent |
| `invalid-field` | a field outside its domain |
| `doc-undecodable` | a stored document does not decode (damage or a bug) |
| `store-error` | the store failed |
