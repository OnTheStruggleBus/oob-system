# Protocol

This chapter is the human-readable face of the O4 protocol document. The
document itself is code: `crates/oob-msg` carries every GI message family
with its status — `documented` (manual-sourced), `hypothesized`
(constructed, unverified) or `verified` (box-accepted, capture-referenced)
— and its evidence backrefs into `lab/captures/`, and the pages here are
built from those annotations plus the PHY record (`phy.md`, the O7 record
required at G0.5). Nothing lands here from notes or memory: a wire-format
claim enters only with the experiment commit and capture behind it, and
the box is the referee (AGENTS.md §2). The first chapter, the oob-mux
pipeline, describes how the stream is produced and labels its framing
`hypothesized`; wire-format chapters arrive with the G0.5 PHY work and
the G1 connect experiments.
