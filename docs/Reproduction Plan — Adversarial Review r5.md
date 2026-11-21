# Reproduction Plan — Adversarial Review r5

- **Date:** 2026-11-22
- **Scope:** Full-corpus review — Reproduction Plan, Milestones & Stage
  Gates, ADR-001 rev 7, Console spec, Interconnect spec, Hardware
  Acquisition List rev 4, Sci-Atlanta Ecosystem Specification, the agent
  workflow (`.github/agents/`, AGENTS.md), and the 2026-11-22 prior-art
  scan. **Dedicated Sci-Atlanta pass included per Marshal's directive.**
- **Method:** same adversarial stance as r1–r4. Undispositioned r3/r4
  findings are not re-litigated; anything found still valid is carried in
  the register (§5), not reissued. Every finding below is *new*.
- **Reviewer posture:** assume the plan is wrong somewhere; find where.

---

## 1. Verdict

**The approach survives.** Nothing found invalidates the PHY-first,
box-as-oracle, SEM-for-QAM/FPK architecture, the gate structure, or the
buy order. Fourteen new deltas (F5-1…F5-14): two touch gate exit
criteria (M3, M4), two are pre-G0.5 process tasks, the rest are
cheap-to-fix-now specification gaps. **None block starting the PHY
bench.** The Sci-Atlanta spec passes review with three deltas — it is
substantially better grounded than the GI side's public record, and the
review confirms the GI-first decision rather than weakening it.

## 2. Sci-Atlanta findings (the dedicated pass)

### F5-1 — Second-front activation trigger is informal prose, not a rule (medium, process)

The S-A spec §8 says "not recommended as a second front while
mini-headend is pre-M1" — good instinct, but it's a sentence in a
reference doc, not an enforced gate. AGENTS.md §9 registers S3 (analog
tier) explicitly but says nothing about an S-A activation. If an
Explorer lot ever shows up cheap, enthusiasm could open a second front
with no ADR, no kill criteria, and — critically — no answer to the
PowerKEY question the spec itself flags as "Unassessed" (§8.6).

**Delta:** add a short "Activation" section to the S-A spec: the second
front activates only post-M6, via its own ADR, with a **PowerKEY
feasibility pre-study as its first gate** (the FPK analog hunt — our
entire GI kill-criteria structure rests on FPK; no such shortcut is
known for PowerKEY). Register the same rule in AGENTS.md §9.

### F5-2 — The surplus watch is GI-only; Explorer feedstock is unwatched (medium, hardware)

HW list P3-3/P3-5 watches OM-2000, DAC-6000, RADD, SEM spares — all GI.
Explorer boxes (2000/2100/2200, 8300-class DVRs) are common, cheap
e-waste *today*; if F5-1's trigger ever fires, donor-box lead time
becomes the second front's first delay. Zero-cost insurance: watch now.

**Delta:** P3-3 gains a zero-planning-weight entry: "Explorer 2000/2200
donor boxes + one 8300-class DVR (Navigator reference unit) — log
sightings; **no buys before second-front activation** (F5-1)."

### F5-3 — Reference-material asymmetry: the R12 fallback target is documented thinner than the primary (medium, guide)

The fidelity ledger is lopsided: i-Guide has a 13-frame analysis;
Navigator/Passport — the **R12 substitute-grid theming target** — has
two YouTube references (spunker88, themilwaukeeboy) plus one Cincinnati
Bell Passport Echo frame. If the O12 census finds no resident guide on
any fleet box, R12 activates and Navigator fidelity becomes a
G6-blocking deliverable resting on the *thinner* reference pack.

**Delta:** research desk commissions a Navigator/Passport reference
pack (frame captures, grid geometry, palette, typography, banner
behavior) to i-Guide rigor before G4, when R12 go/no-go lands.
**Cross-find for the census (O12):** the module inventory should
explicitly watch for **Passport Echo** — it ran on DCT2000-class
hardware (Insight Communications; the Cincinnati Bell frame proves the
look crossed vendor lines). A Passport Echo donor object would satisfy
W8 residency *and* R12 theming in one artifact.

## 3. GI-core findings

### F5-4 — EMM authorship binding (oob-dac ↔ SEM ACP) is a named unknown with no escalation path (high, M3)

M3 assumes the SEM's ACP solves FPK encryption. Likely true for the
*stream* — but who **authors addressed EMMs** (entitlement messages to
unit addresses, provider ID 0x0000/0x0001), over which interface
(SEM-EM? DAC-driven?), and in what format, is discovery work with no
timebox. r4 gave the connect message the R11 ladder; the EMM path has
no equivalent, yet it's the entire point of M3.

**Delta:** mirror R11 for M3 — an EMM-path timebox and escalation
ladder: (1) SEM manual re-read for EMM/ACP interfaces, (2)
box-as-oracle against SEM-generated streams with the entitlement matrix
varied, (3) surplus escalation (OM/RADD — same ladder rung as R11, one
purchase covers both). M3 exit criteria amended accordingly.

### F5-5 — M4 must prove format understanding, not replay (high, M4/code download)

Re-serving a captured donor object proves the carousel plumbing, not
the format. A replay-only pass would leave O13 (parser RE) technically
open while the gate reads green.

**Delta:** M4 exit criteria gain a **modified-object round trip**:
patch one behavior-visible element of a donor object (a string
resource), run it through oob-cdl, download to the sacrificial box, and
capture the changed behavior on the rig. Replay = plumbing; mutation =
understanding.

### F5-6 — "PSI/SI" phrasing risks a DVB-by-default oob-mux (medium, oob-mux)

oob-mux is specified as "TS writer, PSI/SI" — and every off-the-shelf
TS library defaults to DVB SI. The prior-art scan surfaced the DCII
specifics: DCII reserves **PID 8187 for the master guide table** (DVB
uses PIDs 16–31), extends program numbers with **virtual channel
numbers**, and permits **Huffman-compressed descriptor text**
(single-source: Wikipedia/DigiCipher 2; box-confirmable via d 14).

**Delta:** name "**DCII SI, not DVB SI**" as an explicit oob-mux
requirement, confidence-labeled single-source pending box-as-oracle
confirmation; add the PID 8187 anchor and Huffman text note to oob-msg's
research backlog. Cheap now, expensive after a DVB-shaped mux exists.

### F5-7 — Audio codec unpinned (low, playout)

Constraints pin MPEG-2 MP@ML video but say nothing about audio. Period
GI plants ran MPEG-1 Layer II (Musicam) with AC-3 on some services;
Explorer-era S-A boxes did AC-3 + Musicam. The DCT2000's supported set
must come from the fleet, not assumption.

**Delta:** pin "MPEG-1 Layer II primary; AC-3 only where the census
confirms decoder support" in the oob-playout spec; verify against
census d-screens.

### F5-8 — System-time distribution is unnamed in the message inventory (medium, oob-msg)

Guide listings, DVR scheduling, and front-panel clocks all need box
time. The DAC distributed it; our message inventory (factory-init,
unit-creation, connect…) doesn't name it, so M1 discovery might not log
the box behavior that reveals it.

**Delta:** add "system time/date" as a **hypothesized** message class
in oob-msg's inventory so the discovery loop hunts it from day one;
the capture rig notes any box-clock behavior during connect
experiments.

### F5-9 — EAS absent from scope statements (low, M6 polish)

CableLabs R-OOB confirms the DAC carried EAS; period plants ran weekly
tests with crawls. A fidelity-complete headend eventually wants one.

**Delta:** zero-weight M6 backlog entry — EAS message class + crawl on
the guide channel. Register only; no gate weight.

## 4. Process and infrastructure findings

### F5-10 — Docs-as-build-artifact has no scaffolding (high, process — pre-G0.5)

Ground rule 8 makes docs a CI-built artifact (mdBook, broken links fail
the build), but `docs/` is thirteen flat Markdown files with no
`book.toml`, no `SUMMARY.md`, and no CI workflow. The corpus is growing
faster than the scaffolding debt is shrinking, and gate rule 9 (no gate
passes with stale docs) is unenforceable until the build exists.

**Delta:** named pre-G0.5-exit task: scaffold mdBook (book.toml +
SUMMARY.md generated from the doc map), wire the CI link-check, and add
the utoipa drift-check job stub. Small, mechanical, unblocks rule 9.

### F5-11 — git-crypt secrets tier uninitialized (high, process — pre-G1)

The first box captures land at G1 and must be born encrypted (standing
secrets rule: FPK material, box dumps, key material never in
plaintext). Initializing git-crypt *after* captures exist risks
plaintext history in the repo's past.

**Delta:** before G1 entry: init git-crypt, write `.gitattributes`
(`lab/captures/**`, `ops/secrets/**`), commit a canary secret file and
verify it renders as ciphertext on GitHub; Marshal escrows the key
offline. Add to the G1 entry checklist in the Milestones doc.

### F5-12 — Unit-DB durability semantics unstated (low, data)

Config-as-code covers `lineup.yaml`/`units.yaml`, but runtime-learned
state (enrollment results, granted entitlements, box-reported status)
lives in the sqlite unit DB. Nobody has said whether it's precious or
derived.

**Delta:** one sentence in AGENTS.md §5: the unit DB is **derived state**
— rebuildable from ops config plus box re-enrollment — and therefore
excluded from backup policy (or the alternative, if Marshal disagrees).
The ambiguity is the bug; either answer fixes it.

### F5-13 — Release-tag format needs a collision-safe shape now (low, process)

Hunt-carrier release tags (75.25, 104.20, …) are adopted, but bare
dotted-decimal tags will eventually collide with semver-expecting
tooling (cargo, container registries, GitHub's own release UI
conventions).

**Delta:** fix the format in AGENTS.md §4 as `rel/104.20` (tag) with
the carrier order recorded in the Milestones doc; cosmetic while zero
releases exist.

### F5-14 — Interconnect control plane has no security paragraph (low, stretch)

Hub kits phone home over the internet; the Interconnect spec is rightly
lean, but "how hubs authenticate to the master" is unanswered, and a
retrofit after hub hardware exists is exactly how hobby projects end up
with telnet on a public IP.

**Delta:** one paragraph in the Interconnect spec now: WireGuard (or
mutual TLS) per hub, per-hub keys, hub-initiated tunnels only, no
inbound listeners on the master's public interface. Post-G6 weight:
zero. Cost now: one paragraph.

## 5. Carry-forward register (previously issued, still open)

Not new findings — consolidated so nothing is lost between rounds:

1. **r3 (11 findings) and r4 (8 findings) dispositions** — Marshal's,
  first G0.5 agenda item.
2. **ADR-001 rev 7 approval** — open since M0.
3. **HW list rev-2 radio-role note** (Pluto-first vs r4 F4-1's HackRF
   primary) — offered for disposition alongside r4.
4. **Branch protection** (AGENTS.md §6.6 checklist) and **Projects
   board + labels** (`.github/agents/README.md` spec) — manual owner
   actions, still pending.
5. **Prior Art & External References doc** (2026-11-22 scan) — landed in
   `docs/` during this review round. F5-6's DCII SI facts and the
   oobscte55 cross-references in the S-A spec are consistent with it.

## 6. Checked and found sound

So the record shows the review did the work, not just issued deltas:

- **HW list P-order vs gate entry criteria** — P0-1→G0.5, P0-3→G1,
  P0-4→E2/G4, P2-1→G5: all consistent; no purchase lacks a gate and no
  gate entry lacks a purchase.
- **Agent roster vs workstreams** — all W1–W8 and E1–E4 have an owning
  role; handoff directions are acyclic enough for a board-driven flow.
- **S-A spec internal consistency** — 772 ksym/s, SRRC α=0.30,
  RS(55,53), Forney I=5/M=11, SL-ESF 4632-bit superframe, VCI 0x0021 /
  0x0fa2 roles, and the 70–130 MHz / 8–26.5 MHz windows are internally
  consistent and match its cited sources (oobscte55, CableLabs R-OOB).
  The 55-1/55-2 contrast table (§3.3) is accurate against the DCT2000
  manual facts in hand.
- **GI-first decision** — *strengthened* by this review: the S-A CA
  side has no known FPK analog (spec §5), the S-A stack is heavier
  (ATM/DSM-CC vs TS sections), and the only open OOB oracle
  (oobscte55) is Mode B. Every asymmetry favors finishing GI first.
- **Kill criteria** — re-checked against all fourteen findings: none
  approach a kill condition.

## 7. Disposition

All F5 findings go to the **G0.5 agenda** alongside the r3/r4
dispositions. Sequencing: F5-10 and F5-11 are pre-gate-exit tasks (they
gate G0.5 and G1 respectively); F5-4 and F5-5 amend M3/M4 exit criteria
and must land before those gates' *entry*, not before G0.5 closes;
everything else is same-commit doc work any role agent can pick up.

*Review closes with the project where every previous round left it:
the approach survives; here are the deltas.*