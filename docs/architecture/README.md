# Architecture

This chapter records how the Oob software is built, as it is built: data
models, crate internals and the rules the code enforces. It is the
companion to the decision records (ADR-002 for the runtime shape, ADR-003
for configuration and distribution): an ADR says what was decided, and a
page here says what the code does today. Each page lands in the same
commit as the code it describes (AGENTS.md §3), and every other file in
this directory is picked up as a sub-chapter automatically.

- `oob-dac-store.md`: the oob-dac sqlite store (one file, per-table
  durability classes, the outbox, migrations).
