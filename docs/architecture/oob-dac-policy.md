# oob-dac policy layer

This page describes the server-side guardrail every client sits behind
(r6 F6-10, ADR-003 section 5, issue #23). It is crate-internal and tracks the
code in `crates/oob-dac/src/policy/`. Its class and rule tables are
drift-tested against the code (`the_docs_page_carries_every_class_and_rule`),
so a class or rule added in code without a row here fails the build.

Everything here is **box-free**: the evidence is deterministic tests, and no
behavior is claimed `verified` against a box.

## Where it sits

Every front door reaches oob-dac only through `CommandBus::submit`
([the command layer](oob-dac-command.md)). The bus calls `Policy::check` after
validation and target resolution and before anything is persisted or sent, and
`Policy::audit` with every outcome. `StorePolicy` is the real implementation
of that hook. `AllowAllPolicy` stays as a **test double**; the bus has no
default policy, so an adapter must pass a `StorePolicy`.

`StorePolicy` reads and writes the [store](oob-dac-store.md) through a
`StoreHandle` and fails **closed**: a store error refuses with
`policy-store-error`.

## Roles and front doors

| Actor | May do |
| --- | --- |
| Guest (museum mode) | nothing: every command is refused with `guest-read-only`. Reads do not pass through the command layer; the REST adapter (#27) enforces them |
| Operator | non-destructive commands, except a Lab injection |
| Owner through REST or Lab | everything, subject to the token rules below |
| Billing, WireLink-wire | non-destructive commands only; they cannot obtain a token, so they cannot run any destructive class, **Disconnect included** (Marshal, 2026-11-23: Disconnect stays owner plus token; revisit when S5 billing is dispositioned) |
| The DAC's own timers (Auto) | auto-generated operations only; they can never originate a destructive class (`system-cannot-originate`) |

## Destructive classes

The table is `policy::CLASSES`. A token is bound to the operation `name` and
to a target of one of the listed kinds. The typed phrase is the class phrase
followed by the target label: the terminal ID, the first 12 hex digits of the
plan digest, or the install ID.

| Class | Tier | Phrase | Target | Enters through |
| --- | --- | --- | --- | --- |
| `cold-initialize` | D-hard | `COLD INIT` | terminal, plan | `Command::Terminal`, bulk `Command::Operation` |
| `factory-init` | D-hard | `FACTORY INIT` | terminal, plan | `Command::Terminal`, bulk `Command::Operation` |
| `initialize` | D-hard | `INITIALIZE` | terminal, plan | `Command::Terminal`, bulk `Command::Operation` |
| `unit-creation` | D-hard | `UNIT CREATE` | terminal, plan | `Command::Terminal`, bulk `Command::Operation` |
| `disconnect` | D-reversible | `DISCONNECT` | terminal, plan | `Command::Terminal`, bulk `Command::Operation` |
| `code-download-enable` | D-hard | `CDL ENABLE` | terminal | `StorePolicy::authorize` (parameter-level; needs #19) |
| `cdl-campaign` | D-hard | `CDL CAMPAIGN` | plan | `Command::Auto(CodeDownload)` |
| `cdl-lock-off` | D-hard | `CDL LOCK OFF` | install | `StorePolicy::set_cdl_lock` |
| `mark-sacrificial` | D-hard | `MARK SACRIFICIAL` | terminal | `StorePolicy::mark_sacrificial` |
| `store-restore` | D-hard | `STORE RESTORE` | install | `StorePolicy::authorize` (the restore itself is #24) |

Unit Creation (it rewrites box identity) and marking a unit sacrificial (it
makes the unit CDL-eligible) are stricter than the issue text; Marshal had no
objection (2026-11-23). Both D-hard and D-reversible need the owner role and a
token; the tier is a label for the docs and the Console.

`code-download-enable` is a `General` message with `code_download_enable =
true`. The command layer cannot see message parameters until the encoders land
(#19), so the class exists and `StorePolicy::authorize` enforces it, but the
bus cannot route to it yet. Likewise `Command::Auto(CodeDownload)` carries no
resolved terminals today, so the CDL lock **fails closed** for it (below).

## Confirmation tokens

1. **Plan (bulk only).** A bulk operation built with `DefineOperation::new` is
   a dry run: the bus returns a `DryRunReport` with the resolved terminal count
   and a `plan_digest` (SHA-256 of the operation name and the sorted terminal
   IDs).
2. **Request a token**, a separate call: `StorePolicy::issue_token` with the
   operation, the target and the typed phrase. The server compares the phrase
   (`FACTORY INIT 12`, `CDL LOCK OFF <install id>`), so a script or fork cannot
   skip the typed confirmation of AGENTS.md section 7. Owner only, audited.
3. **Execute** with the token in `CommandContext::confirmation`.

A token is 128 random bits shown once (`oobtok-` plus 32 hex digits). Only its
SHA-256 is stored, in the `confirmation_token` table (DerivedObserved: losing
tokens on a restore is correct, and `StorePolicy::new` purges them at start
because their times are monotonic-clock milliseconds). The lifetime defaults to
120 s and is capped by the server at 600 s. A token is **single use**: it is
spent by the check that accepts it, in one store transaction on the single
writer, so of two racing spends exactly one wins. A refused attempt does not
spend it.

Binding is to the **terminal ID**, the store-allocated handle for the unit;
serials and unit addresses never leave the store (ADR-003 section 4), and the
command layer does not carry them.

Where the design note says the token is consumed in the same transaction that
persists the operation, that waits for the `operation` ledger (with the daemon,
#29): the token is spent at check time today, so an operation that then fails to
encode has still used its token. That errs on the safe side.

### Rules

Every refusal names its rule; the code is the `reason` of the
`CommandError::Policy`.

| Rule | Refuses |
| --- | --- |
| `owner-required` | a destructive class by a non-owner or through a door without tokens; a Lab injection by an operator |
| `guest-read-only` | any command by a guest |
| `system-cannot-originate` | a destructive class from the DAC's own timers |
| `token-missing` | a destructive operation with no token |
| `token-unknown` | a token this install did not issue, or purged |
| `token-expired` | a token at or past its lifetime |
| `token-reused` | a token already spent |
| `token-op-mismatch` | a token issued for another operation |
| `token-target-mismatch` | a token issued for another terminal, or a target kind the class cannot take |
| `token-plan-changed` | a bulk token whose resolved set is no longer the planned one |
| `confirm-text-mismatch` | a typed phrase that is not the required one |
| `cdl-lock-non-sacrificial` | a CDL class while the lock is on and a target is not sacrificial |
| `unknown-terminal` | marking a terminal that does not exist |
| `policy-store-error` | any store failure (fail closed) |

The order of checks is: guest, then owner and door, then the CDL lock, then
the token, so a lock refusal never spends a token.

## The sacrificial-only CDL lock (QA-7)

- It is the per-install setting `cdl_sacrificial_only` in `install_setting`
  (Precious). The migration seeds it **ON**, so every fresh store starts
  locked.
- While ON, a CDL class whose resolved target set contains any unit with
  `sacrificial = false` is refused (`cdl-lock-non-sacrificial`), naming the
  offending terminal IDs. An empty set (the bus resolves none for
  `Command::Auto(CodeDownload)` yet) is refused too, because it cannot be shown
  sacrificial.
- **Turning it OFF** (`StorePolicy::set_cdl_lock(false)`) needs the owner role
  and a token for `CDL LOCK OFF <install id>` **and nothing else**. The product
  imposes no D0 or evidence check (ADR-003 section 5, Marshal QA-7). The token
  is spent in the same transaction that changes the setting, which is audited
  with before and after values.
- **Turning it ON** needs the owner role only.
- **Marking a unit sacrificial** is D-hard; unmarking is owner-only.

This governs the product's CDL campaigns only. AGENTS.md section 7
(flash recovery before download) still binds this project's own lab and
agents, and ADR-003 section 4's write-back precondition for firmware import is
unchanged.

## Audit

Every outcome is appended to the precious, append-only `audit_log` with who,
role, front door, when, command, target (a count of terminals, never a serial)
and outcome. The bus reports each command through `Policy::audit`: `ok`,
`dry-run`, or the refusing rule or error code. Token issuance (and its
refusals) and setting changes write their own rows; a setting change shares its
transaction with the change and records before and after. A token is never
written to the log. `Policy::audit` cannot return an error, so a failed audit
write is counted in `StorePolicy::audit_failures` for the health check.

## Configuration layer (ADR-003)

| Item | Layer |
| --- | --- |
| class table, rule IDs, token binding, typed phrases, lock seeded ON, check order | 3 (code) |
| `cdl_sacrificial_only`, the per-terminal `sacrificial` flag | 1 (runtime, owner-only, audited) |
| token TTL (default 120 s, capped at 600 s) | 1; a `PolicySettings` value until #25 stores it |

## Not built yet

- Consuming the token inside the operation ledger's transaction (needs the
  ledger, #29).
- Parameter-level `code-download-enable` and auto Code Download repetitions
  that inherit an owner-confirmed campaign's authorization (need the encoders
  #19 and the CDL campaign builder). Until then an auto Code Download is
  refused with `system-cannot-originate`.
- API tokens as roles and the REST error shape (#27); a `WireLinkWire` actor
  with its fixed business-system role (#28).
