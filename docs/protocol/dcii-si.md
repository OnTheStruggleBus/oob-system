# DCII PSI/SI: PAT, PMT, CAT and the GI CA descriptors (generic MPEG-2 so far)

The O4 "DCII PSI/SI conformance set" row. Source of truth: the `oob_mux::psi`
and `oob_mux::pid_plan` module docs (`cargo doc -p oob-mux`); this page
renders the same status table. Built by issue #17 to r4 F4-6 and r5 F5-6
(accepted 2026-11-22) and the #17 design note.

**Nothing on this page is box-verified.** No table here is claimed to be
accepted by a box. The check that would say so is d 15 / d 16 on a box
(F4-6), a later capture-backed experiment; issue #18 cross-checks the same
bytes with tsduck as an independent decoder. Until a capture is referenced
from `lab/captures/`, every row is `documented` or `hypothesized`, and in
fact all are `hypothesized`.

## Scope

The tables are structurally generic MPEG-2 plus the GI CA descriptor;
"DCII-flavored" names the target, not yet the bytes. The DCII deltas (F4-6
PMT conventions, the master guide table on PID 8187, VCT/VCN) are **not
emitted** and are the next step toward d 15 / d 16.

`oob_mux::psi` is one builder library for the OOB stream now and the
in-band path later (Marshal, 2026-11-23). It emits PAT, PMT and CAT as
single-section, long-form tables and wraps each as a C0 system
`Submission::Cyclic` carousel, so the W2 skeleton stream is PAT, PMT and
CAT plus the pacer's nulls. Feeding these tables into the in-band path for
the d 16 check is a later issue. NIT, the DCII master guide table (PID
8187) and Huffman descriptor text are out of scope (F5-6 backlog).

## Conformance table

| Item | Status | Basis |
| --- | --- | --- |
| Long-form section syntax and CRC32 | `hypothesized` | ISO/IEC 13818-1 structure; no in-hand copy; tsduck cross-check is #18 |
| PAT layout | `hypothesized` | ISO/IEC 13818-1; as above |
| PMT layout (no DCII extras emitted) | `hypothesized` | ISO/IEC 13818-1; DCII PMT conventions (F4-6) are not emitted |
| CAT layout | `hypothesized` | ISO/IEC 13818-1; as above |
| CA_descriptor (tag 0x09) layout | `hypothesized` | ISO/IEC 13818-1; as above |
| GI/Motorola CA_system_id (0x0700) | `hypothesized` | DVB CA_System_ID registry as commonly reported; no DAC/SEM manual or d 16 value in hand. Sourcing is tracked in issue #56 (the value must match the real SEM's CA system) |
| CA descriptor private bytes (PMT) | `hypothesized` | F4-6; empty by default |
| EMM provider ID as CAT descriptor private bytes | `hypothesized` | Oob construction; the 0x0000/0x0001 value is the AGENTS.md §8 guardrail, the placement is not documented |
| PID 8187 (0x1FFB) reserved for the DCII MGT | `hypothesized` | r5 F5-6, single source; enforced anyway |

The Rust test `no_row_is_verified` keeps the code's copy of this table free
of `verified`. Promotion needs a capture, never a note.

## The PID plan is runtime configuration

`PidPlan` (ADR-003 Layer 1) is an input value: transport_stream_id,
programs (PMT PID, PCR PID, elementary streams, CA references), EMM streams
and the carousel period. oob-dac keeps it in its store; `pid_plan::validate`
runs on save and is the only way to get the `ValidatedPidPlan` the
assemblers accept. It refuses duplicate PIDs, ECM PIDs that collide with
other roles, reserved PIDs (0x0000-0x000F and 0x1FFB), program_number 0 or
repeated, over-long sections (section_length above 1021), oversize CA
private data and a zero period. The EMM provider ID type admits only
0x0000 and 0x0001. Fixed in code, not config: PAT on PID 0x0000, CAT on
0x0001, null only in the pacer, and the known CA systems with their IDs
(a new CA system is an upstream PR, like a box type).

`PidPlan::seed_default()` is example data for new installs and tests, not a
claim about any plant.

## Section layout as emitted (all `hypothesized`)

Every table is one section: `table_id`, `section_syntax_indicator = 1`,
private bit 0, reserved bits 11, 12-bit `section_length`, a 16-bit
table-id extension, `reserved 11 | version(5) | current_next = 1`,
`section_number = last_section_number = 0`, body, CRC32 (polynomial
0x04C11DB7, initial value all ones, no reflection, no final XOR).

| Table | table_id | PID | Extension | Body |
| --- | --- | --- | --- | --- |
| PAT | 0x00 | 0x0000 | transport_stream_id | per program: `program_number(16)`, `111`+`PMT PID(13)` |
| PMT | 0x02 | the program's PMT PID | program_number | `111`+`PCR PID(13)`, `1111`+`program_info_length(12)`, CA descriptors; per stream `stream_type(8)`, `111`+`PID(13)`, `1111`+`ES_info_length(12)`, CA descriptors |
| CAT | 0x01 | 0x0001 | 0xFFFF | one CA descriptor per EMM stream |

CA_descriptor: `tag 0x09`, `length`, `CA_system_id(16)`, `111`+`CA_PID(13)`,
private bytes. In the CAT the private bytes are the two-byte EMM provider
ID (Oob's construction, `hypothesized`).

Golden vectors, committed as hex under `crates/oob-mux/tests/golden/`:
`pat_1`, `pat_3`, `pmt_no_ca`, `pmt_program_ca`, `pmt_es_ca`, `cat_1`,
`cat_2`. Their CRCs were also checked outside the Rust code with an
independent implementation. Golden files catch regressions; they are not
conformance evidence.

## Versions and emission

A table's 5-bit `version_number` advances (mod 32) only when its bytes
change, and the counters live in a caller-owned `TableVersions` so oob-dac
can persist them. `TableVersions::snapshot` returns one
`TableVersionEntry` per table (kind, version, last bytes, retired) and
`from_parts` restores from them; oob-dac #25 wires the actual persistence.
A program that leaves the plan keeps its last version and bytes (withdrawn
once), so re-adding it continues the count: changed bytes advance the
version rather than reusing 0, identical bytes keep it.
`build_all` returns one cyclic C0 submission per table
plus the carousel ids of tables that left the plan; re-submitting an
unchanged plan repeats identical bytes, which the scheduler keeps on its
schedule. Continuity counters are the packetizer's, per PID; the test
`psi_through_the_pipeline_with_continuity_across_wrap` drives PSI through
scheduler, packetizer and pacer and checks the counters across the 4-bit
wrap.
