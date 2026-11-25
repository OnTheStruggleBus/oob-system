# oob-dac store

The oob-dac store is the single sqlite database that holds every table
oob-dac owns. It implements r6 F6-5 (per-table durability, the atomicity
rule, the outbox, versioned migrations) under ADR-002 decision 1 (oob-dac
owns all state) and ADR-003 §6 (the database is the installation). Code:
`crates/oob-dac/src/store/` (issue #21).

## One file, one writer

- **One database file.** Everything lives in one sqlite file, so a
  precious write and its derived consequences can share one transaction.
  `ATTACH` is impossible: every connection, the writer's and each
  reader's, is opened with `SQLITE_LIMIT_ATTACHED` set to 0, because a
  transaction spanning attached databases is not atomic in WAL mode. Tests
  check that `ATTACH` fails on both kinds of connection and that
  `PRAGMA database_list` lists only `main` (and `temp`) after a full
  workload.
- **Engine.** `rusqlite` with the bundled sqlite, so every install runs
  the same sqlite version.
- **One writer.** The store owns the only read-write connection and holds
  an exclusive lock on `<database>.lock` for as long as it is open, so a
  second open of the same file fails. Write transactions begin
  `IMMEDIATE`. Async callers reach the store through `StoreHandle`, a
  bounded channel to a dedicated writer thread. Readers open their own
  read-only connections, which WAL runs alongside the writer.
- **Pragmas on open:** `journal_mode=WAL`, `synchronous=FULL` (in WAL
  mode, `NORMAL` can lose the last commits on power loss, and precious
  ledgers must survive that), `foreign_keys=ON`, `recursive_triggers=ON`,
  a busy timeout (an install setting), and a `quick_check`. Each pragma is
  read back after it is set, and a wrong value fails the open. A failed
  check opens nothing and reports the file as corrupt; restore from backup
  is #24.

## Table classification

Every table carries a durability class in the code registry
`store::TABLES`. A test lists the tables in `sqlite_master` and fails if
any table is unclassified or if any registry entry has no table.

| Class | Meaning | On rebuild |
| --- | --- | --- |
| Precious | Nothing can regenerate it; it is backed up with the file | Never touched |
| DerivedFromEvents | A pure fold of `event_log` | Dropped, recreated, replayed exactly |
| DerivedObserved | A cache of what the plant reports, or ephemeral security state that is correct to lose (a confirmation token) | Recreated empty; polls and d-screen scrapes refill a plant cache, and a lost token is simply re-issued |

The first tables (migration `0001_initial`):

| Table | Class | Content |
| --- | --- | --- |
| `terminal` | Precious | The Define Terminal record (Plan §3.2): store-allocated 24-bit terminal ID, unit address (40-bit address plus three check digits, see below), 12-character serial, terminal type (One-Way, Fone-Way, Two-Way, Ethernet, DOCSIS), box type, account number, On-Plant, install and auto-registration dates (`YYYY-MM-DD`, real calendar days), Activated, Autodiscovered and IP address (DSG/DOCSIS; parsed as IPv4 or IPv6), BSI name, downstream and upstream plant, and the sacrificial-unit flag |
| `terminal_status` | DerivedObserved | Last Response Date, Failures, Power Leveling Attempts, Non-Participating Counts, the Chronic Non-Responding flag, and the last d-screen scrape |
| `event_log` | Precious, append-only | The outbox: sequence number, time, kind, JSON payload, actor |
| `derive_watermark` | DerivedFromEvents | How far each deriver has folded `event_log` |
| `audit_log` | Precious, append-only | The bsilog equivalent (DAC Reference §8): actor, role, front door, command, target, outcome, before and after |

Migration `0002_policy` (#23) adds the policy layer's two tables:

| Table | Class | Content |
| --- | --- | --- |
| `confirmation_token` | DerivedObserved | Short-lived F6-10 confirmation tokens, stored as a SHA-256 only; worthless after a restart, so losing them on a restore is correct |
| `install_setting` | Precious | Per-install owner settings: the sacrificial-only CDL lock (seeded ON) and the random install ID. See [the policy layer](oob-dac-policy.md) |

Append-only is enforced by the schema. On both journals, `BEFORE UPDATE`
and `BEFORE DELETE` triggers abort the statement, and a `BEFORE INSERT`
trigger aborts any insert that targets an existing sequence number. The
last one closes `REPLACE` and `INSERT OR REPLACE`, which would otherwise
delete the old row without a `DELETE` statement; `recursive_triggers=ON`
backs it up. The store reports every case as an append-only error.

**Unit address.** The printed form `nnn-nnnnn-nnnnn-nnn` has 16 digits:
13 address digits and 3 check digits. The split is **documented** for the
DCT700 and DCT1700 install manuals under "d 05: Unit Address" (cited in
oob-expert's research note on issue #21); applying it to the DCT2000 is
same-family inference. The check-digit algorithm is unknown, so
`ua_check` stores the printed digits verbatim. Oob never computes,
validates or generates them. `unit_address`, the 40-bit value of the
address digits, is the unique identity, and the printed form round-trips
without loss, so a later correction needs only a forward migration.

The other precious tables in the F6-5 list (the entitlement-change
journal, the purchase ledger, the campaign ledger) arrive with the issues
that own them. Each one registers in `store::TABLES` in the same commit,
and the registry test fails if it does not.

## No box instance comes from git

ADR-003 §6 keeps per-box records out of git. The store allocates every
terminal ID (sequentially, never reused), and the type a caller fills in
to create a terminal has no ID field. Migrations seed no box instance:
they may seed install settings (#23 seeds the sacrificial-only CDL lock),
but never a row in a box-identifying table. Those tables are listed in
`store::BOX_IDENTIFYING` (today only `terminal`), and a test checks that a
fresh store has none of their rows and that no migration inserts into
them. Unit addresses and serials identify real boxes, so they live only in
the store file (ADR-003 §4).

## The outbox

1. A precious change that has derived consequences writes its row and an
   `event_log` entry in **one transaction**. Creating a terminal, for
   example, writes the `terminal` row and a `TerminalCreated` event
   together, or neither.
2. After the commit, each deriver folds the events past its watermark.
   It applies the derived changes and advances the watermark in one
   transaction per batch.
3. A crash between steps 1 and 2 leaves events unapplied. The next write,
   or the next open, replays them.
4. **The commit is the outcome.** Once step 1 commits, the write returns
   its value (a new terminal's ID, for example), whatever happens in
   step 2. A derivation failure never fails a committed write, an open or
   a rebuild. It is reported by `derivation_health()` (pending events and
   the last error) for the daemon's health check and the canary to read,
   and the next write or open retries. A caller therefore never sees a
   committed change reported as failed, and never retries it.
5. Each deriver's `apply` is idempotent per event, so a lost watermark
   only costs a replay.
6. **A new event kind needs a new schema version.** Each `Event` variant
   names the migration that introduced it, and `store::EVENT_KINDS` lists
   the kinds by version; a test keeps them in step. An older build that
   meets a newer file then refuses it as too new instead of failing on an
   event it cannot decode.

The first deriver seeds a `terminal_status` row, with no observations,
when a terminal is created. The table is observed data, but the row's
existence comes from the event, so a rebuild recreates the table empty and
replay seeds the rows again.

**Rebuild** (`rebuild_derived`, used by restore in #24) drops and
recreates every derived table from its stored definition, then replays
`event_log` from the start. Precious tables are never touched. A test
checks that precious tables are unchanged, that event-derived tables come
back identical, and that observations are cleared.

## Migrations

- Migrations are numbered SQL files, applied forward only, with
  `PRAGMA user_version` holding the current number.
- Each migration runs in its own transaction, which also bumps
  `user_version`, so a failed migration leaves the file at the previous
  version. A test injects a failing second migration and checks exactly
  that.
- On open, an older file is migrated up and a current file is used as is.
  A file newer than the build is **refused**: an older Oob does not open a
  newer installation, and there is no downgrade.
- A migration that creates a table adds its registry entry in the same
  commit.

## Where the settings come from (ADR-003)

| Item | Layer |
| --- | --- |
| Schema, migrations, classification registry, append-only triggers, terminal-type set | Code (Layer 3) |
| Database path, busy timeout | Install settings (Layer 1, from the daemon configuration, #29) |
| Terminal rows, events, audit rows | Runtime data (Layer 1, through the command layer and API, #22 and #25) |
| Box-type IDs a terminal refers to | The upstream catalog (#30), read-only at runtime |

## Decision record: SQLite stays

*Recorded from the strategy session 2026-11-24 (Marshal). Logged so that
this is not re-litigated.*

The store stays on single-node SQLite. The system is single-node,
write-light and runs on a closed plant, and for that shape SQLite is the
correct engine. The pragmas and the single-writer rule under "One file, one
writer" are what make it correct. They are not restated as new decisions.

**Why not Postgres now.** A client-server database adds a server to operate,
a network and a second failure domain. It buys no robustness for one process
that owns one file. It would also turn ADR-003 §6 (the database is the
installation, one file that can be backed up and exported whole) into a
cluster problem. The schedule DB in the Reproduction Plan's W4 amendment
lives in this same store, so no second engine is implied.

**Revisit trigger (recorded honestly).** Revisit the engine only if either of
these appears:

- multiple networked write agents, or
- multi-user Console writes.

Until one of them appears, Postgres is complexity without robustness.

**Durability.** Every table carries a durability class in the code registry
(see the section above). The schedule DB tables from the W4 amendment are
classified there when the schedule code lands. This record adds no new engine
and no new durability rule.
