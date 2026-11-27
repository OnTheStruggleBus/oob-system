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

The second chapter, [the pull seam](seam.md), is the contract
between the flowgraph (the clock master) and oob-mux's pacer: a framed
request/response over a Unix socket, with the null-stuffing handoff and the
F6-2 box-free loopback harness. It is Oob's own protocol, not a box wire
format, so it carries no verification status beyond the `hypothesized`
188-byte packet size it inherits.

The third chapter, [DCII PSI/SI](dcii-si.md), is the O4 "DCII PSI/SI
conformance set" row: the PAT, PMT and CAT assemblers, the runtime PID plan
and the GI CA descriptors, each with its status. Every row is `hypothesized`;
nothing is claimed accepted by a box.

The fourth chapter, [DCII CA message families](dcii-ca-messages.md),
documents the structure and semantics of the GI conditional-access
messages — ECM40, ECM41 and EMM95 — plus the key-hierarchy roles and the
box-internal ACP oracle. Synthesized from reverse-engineering research, it
carries structure and field roles only: no key values, no Unit Addresses,
and every family at `documented` or `hypothesized`, never `verified`.
