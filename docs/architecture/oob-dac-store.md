# oob-dac store

The oob-dac store is the single sqlite database that holds every table
oob-dac owns. It implements r6 F6-5 (per-table durability, the atomicity
rule, the outbox, versioned migrations) under ADR-002 decision 1 (oob-dac
owns all state) and ADR-003 §6 (the database is the installation). Code:
`crates/oob-dac/src/store/` (issue #21).

## One file, one writer

- **One database file.** Everything lives in one sqlite file, so a
  precious write and its derived consequences can share one transaction.
  The store never issues `ATTACH`: a transaction spanning attached
  databases is not atomic in WAL mode. A test asserts that
  `PRAGMA database_list` lists only `main` (and `temp`) after a full
  workload.
- **Engine.** `rusqlite` with the bundled sqlite, so every install runs
  the same sqlite version.
- **One writer.** The store owns the only read-write connection. Async
  callers reach it through `StoreHandle`, a bounded channel to a
  dedicated writer thread. Readers open their own read-only connections,
  which WAL runs alongside the writer.
- **Pragmas on open:** `journal_mode=WAL`, `synchronous=FULL` (in WAL
  mode, `NORMAL` can lose the last commits on power loss, and precious
  ledgers must survive that), `foreign_keys=ON`, a busy timeout (an
  install setting), and a `quick_check`. A failed check opens nothing and
  reports the file as corrupt; restore from backup is #24.

## Table classification

Every table carries a durability class in the code registry
`store::TABLES`. A test lists the tables in `sqlite_master` and fails if
any table is unclassified or if any registry entry has no table.

| Class | Meaning | On rebuild |
| --- | --- | --- |
| Precious | Nothing can regenerate it; it is backed up with the file | Never touched |
| DerivedFromEvents | A pure fold of `event_log` | Dropped, recreated, replayed exactly |
| DerivedObserved | A cache of what the plant reports | Recreated with no observations; polls and d-screen scrapes refill it |

The first tables (migration `0001_initial`):

| Table | Class | Content |
| --- | --- | --- |
| `terminal` | Precious | The Define Terminal record (Plan §3.2): store-allocated 24-bit terminal ID, 40-bit unit address with its three printed trailing digits, 12-character serial, terminal type (One-Way, Fone-Way, Two-Way, Ethernet, DOCSIS), box type, account number, On-Plant, install and auto-registration dates, Activated, Autodiscovered and IP address (DSG/DOCSIS), BSI name, downstream and upstream plant, and the sacrificial-unit flag |
| `terminal_status` | DerivedObserved | Last Response Date, Failures, Power Leveling Attempts, Non-Participating Counts, the Chronic Non-Responding flag, and the last d-screen scrape |
| `event_log` | Precious, append-only | The outbox: sequence number, time, kind, JSON payload, actor |
| `derive_watermark` | DerivedFromEvents | How far each deriver has folded `event_log` |
| `audit_log` | Precious, append-only | The bsilog equivalent (DAC Reference §8): actor, role, front door, command, target, outcome, before and after |

Append-only is enforced by the schema: `BEFORE UPDATE` and `BEFORE DELETE`
triggers on both journals abort the statement, and the store reports an
append-only error.

The other precious tables in the F6-5 list (the entitlement-change
journal, the purchase ledger, the campaign ledger) arrive with the issues
that own them. Each one registers in `store::TABLES` in the same commit,
and the registry test fails if it does not.

## No box instance comes from git

ADR-003 §6 keeps per-box records out of git. The store allocates every
terminal ID (sequentially, never reused), and the type a caller fills in
to create a terminal has no ID field. Migrations seed no rows; a test
checks that a fresh store has no terminals. Unit addresses and serials
identify real boxes, so they live only in the store file (ADR-003 §4).

## The outbox

1. A precious change that has derived consequences writes its row and an
   `event_log` entry in **one transaction**. Creating a terminal, for
   example, writes the `terminal` row and a `TerminalCreated` event
   together, or neither.
2. After the commit, each deriver folds the events past its watermark.
   It applies the derived changes and advances the watermark in one
   transaction per batch.
3. A crash between steps 1 and 2 leaves events unapplied. The next write,
   or the next open, replays them. If derivation fails without a crash,
   the write reports that its precious change committed and that
   derivation is pending.
4. Each deriver's `apply` is idempotent per event, so a lost watermark
   only costs a replay.

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
  version.
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
