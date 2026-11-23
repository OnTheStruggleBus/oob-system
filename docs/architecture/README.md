# Architecture

This is the architecture chapter of ADR-002 action 7. It describes how the
Oob software is put together, and it holds two kinds of page:

- **The decided architecture.** The runtime topology, the PHY pull seam,
  the insertion-budget traffic classes and the ADR-003 configuration
  layers, as ADR-002 and ADR-003 decided them. These pages land with #38
  and may describe parts that are decided but not yet built; each one says
  which is which.
- **Crate internals.** Data models and the rules a crate enforces. These
  pages land in the same commit as the code they describe (AGENTS.md §3)
  and track it from then on.

The ADRs remain the decision records; pages here explain the result and
point back to them. Every other file in this directory is picked up as a
sub-chapter automatically.

- `oob-dac-store.md`: the oob-dac sqlite store (one file, per-table
  durability classes, the outbox, migrations).
