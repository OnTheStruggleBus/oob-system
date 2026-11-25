# Runbook

The runbook is the lab notebook in Marshal's style: one entry minimum per
gate (AGENTS.md §3), each saying what was attempted, what broke, and the
lessons learned — written while the work happens, in the same commit as
the change it describes. Entries are dated Markdown files in this
directory (`YYYY-MM-DD-<slug>.md`); the book build picks them up as
sub-chapters automatically (`ops/docs-summary.py`), so adding one never
needs a document-map row. Operating procedures that outlive a single gate
(canary triage, flash recovery, the D0 restore drill) also live here once
they exist.
