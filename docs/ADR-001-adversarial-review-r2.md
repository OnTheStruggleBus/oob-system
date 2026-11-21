# ADR-001 — Adversarial Review, Round 2: Can the Gating Challenges Be Overcome?

- **Date:** 2026-11-21
- **Reviewer:** second adversarial pass, requested by Marshal ("see if we can overcome some of those challenges")
- **Method:** take each gating risk from rev 2 and attack it with targeted deep research — vendor documentation archaeology, regulatory filings, box diagnostics as an oracle, and adjacent-ecosystem tooling.
- **Verdict:** **Yes — materially.** No challenge is declared solved, but every gating risk now has at least one concrete, evidence-backed path that didn't exist in rev 2. The single biggest shift: the crypto problem (R1) is no longer "impossible without lost keys" — it has four live paths, one of which (the hybrid "crypto oracle") bypasses key *knowledge* entirely. 9 findings, all recommended ACCEPT.

---

## Summary table

| Challenge (rev 2) | Round-2 verdict |
|---|---|
| R1 MediaCipher key establishment | **New paths found** — four concrete options, incl. one that sidesteps key knowledge (F2-1, F2-2, F2-3) |
| R2 Code download segment format | **Reduced** — 648-page DAC operations manual located; instrumented-box strategy added (F2-1, F2-6) |
| R3 E 09/E 11 recovery | **Reduced** — INIT flows documented to exist in the DAC ops manual (F2-1) |
| R5/O1 No ground-truth captures | **Bypassed** — generate our own ground truth with surplus headend hardware (F2-4) |
| R6 STARFONE protocol | **Downgraded to non-gating** — one-way operation is a documented mode; poll seq. numbers give observability (F2-5) |
| R8 Fleet heterogeneity | **Turned into an asset** — DCT2500 is the ideal instrumented reference box (F2-6) |
| (new) Hardware safety | **New finding** — CPSC recall on 2002-era DCT2000 power cords (F2-7) |

---

## F2-1 — The DAC-6000 operator training manual exists (648 pages)

**Finding:** "Basic DAC 6000 (V3.1) Operation, Rev 1" — a Motorola training manual, 648 pages — is publicly posted on Scribd (uploaded by Valerio Bitonti). Its searchable excerpts already reference "MediaCipher Smart which is used to mate an MCSC with a host set-top" and the "authenticator utilized by the Motorola Conditional Access System."

**Why this changes the game:** this is the operator-facing documentation of the exact system we're emulating. If it covers set-top initialization, key/authenticator management, EMM operations, and download management at the level operator manuals usually do, it attacks R1, R2, R3, and the message catalog (O4) simultaneously — replacing reverse engineering with reading.

**Action:** acquire it (Scribd download or mirror hunt). This becomes research item #1.

## F2-2 — MediaCipher's silicon is mapped; both ends are hardware roots — and "mating" is a documented product flow

**Finding:** an SEC-filed Intellectual Property License Agreement names the MediaCipher IC family: **MCSC** ("MediaCipher Smart Card" die, SCWF65000 — the TV PassCard chip), **MC1.7** (set-top conditional-access IC — what's inside a DCT2000), **MC2.0** (**infrastructure** conditional-access IC — the headend-side crypto element), MC2.1 (multi-stream), MC2.7 (next-gen set-top). The box's own diagnostics confirm the removable **TV PassCard (TvPC)** with explicit mating states: `0a TvPC not mated`, `0b TvPC/base module unit address mismatch`, `0C New TvPC, but wrong version number`, and `TvPC NOT REQUIRED` on some units.

**Interpretation — four concrete paths for R1, in priority order:**
1. **Crypto oracle (new, strongest):** acquire a surplus DAC-6000 *with its CA hardware* (the MC2.0-side authenticator) and let the **real** DAC mint EMMs and run mating/initialization for our boxes, while rust-dac owns everything else (connect, config, channel map, guide, CDL). We stop trying to *reimplement* 1990s HSM silicon and instead *adopt* it. Feasibility hinges on surplus DAC availability and whether a DAC can initialize boxes whose original operator key database is gone — which is exactly what the mating/INIT flows exist for (F2-1's manual should answer this).
2. **TvPC card control:** TvPC is a *removable* smart card with documented new-card mating states. Source surplus/new TvPC cards; if the mating flow lets a fresh card pair with a base module under our headend, the card becomes the known-key element. (Also the cleanest explanation of "New TvPC, but wrong version number" — cards get provisioned on-mating.)
3. **Install-time INIT flow in software:** as rev 2 said — reproduce `INIT_SETTOP` semantics from the ops manual (F2-1) so boxes initialize into a known-key state under rust-dac alone.
4. **Firmware-level analysis (fallback):** DCT2500 is Broadcom-based, and the Broadcom cable ecosystem has mature open tooling precedent (bcm2-utils / ProgramStore for dump/parse/modify). DCT2000's d 11 shows 512K ROM / 1M flash — the whole platform is small enough to audit.

**Verdict:** R1 stays the gating risk, but "extract lost keys" is no longer the story. The story is now "adopt or reproduce documented mating flows," with a hardware fallback that needs no key knowledge at all.

## F2-3 — Honest boundary: what research did NOT find

- No public hobbyist DAC emulator (confirmed again — GitHub/code searches empty). First-of-kind stands.
- No wire-format documentation of the OOB message set, CDL segments, or STARFONE polling outside the DAC ops manual lead.
- Motorola "Privacy Mode" (a simpler fixed-key CA tier named in CableLabs EQAM-VSI) is real, but DCT2000-era support is unverified — logged as a low-confidence lead only, not a plan.

## F2-4 — Ground truth no longer depends on finding period captures

**Finding:** O1 (captures) can be bypassed entirely. The same surplus-hardware strategy as F2-2 path 1 — a real OM-2000 (accepts UDP/IP in) and ideally a real DAC-6000 — generates *live, correct* traffic for *our* boxes. That is better than a period capture: it's interactive, addressable to our unit addresses, and infinitely repeatable.

**Action:** reframe O1 from "find captures" to "acquire reference hardware (OM-2000 first, DAC-6000 opportunistically)"; captures remain a nice-to-have.

## F2-5 — STARFONE downgraded: one-way operation is documented; protocol RE has an oracle

**Findings:** the DCT2000 manual defines STARFONE's job narrowly — "send Impulse Pay-Per-View (IPPV) information to the DAC 6000 or other controller through the subscriber's telephone hookup," i.e., boxes are fully operable as **one-way addressable** terminals; the phone link is for IPPV reporting and polls. The DCT2500 diagnostic shows the polling layer's skeleton: `LAST POLL REQ: 170` / `LAST POLL ACK: 170` sequence numbers with timestamps, busy/no-answer/lost-carrier counters, and hang-up reasons — a built-in protocol analyzer on the box itself. STARFONE II runs to 14.4 kbps; default connect 300 baud 8E1.

**Verdict:** R6 drops from risk to stretch goal. The fleet works one-way; M5 becomes "make the poll/ack loop close," with the box's own counters as the test harness.

## F2-6 — Fleet heterogeneity becomes an asset: DCT2500 as instrumented reference box

**Findings:** DCT2500 = revised DCT2000 platform on a **Broadcom single-chip** design, with a standard **RS-232 serial port** and a much richer diagnostic suite (d 15 Message Types, d 16/d 17 in-band PAT/PMT decode, d 18 Task Status, d 09 poll req/ack). Also noted: the DCT2000 has **integrated STARVUE II RF return as standard** (STARFONE II is the optional module) — so RF-return capability is likely fleet-wide, strengthening the M6 upstream plan.

**Action:** census (O2) designates one DCT2500 as the **instrumented reference box**: serial console probe first (Broadcom-era Motorola boxes often expose a console), diagnostics scrape for protocol RE, and it becomes the canary for CI.

## F2-7 — New safety finding: CPSC recall on 2002-era DCT2000 power cords

**Finding:** CPSC/Motorola recalled ~1M DCT2000s distributed March–June 2002 — rear power-cord pins can break (shock hazard; ~30k units affected).

**Action:** add to census checklist (O2): record manufacture date per unit; inspect cord pins; any 2002 unit gets a cord check before prolonged operation. Goes in the build runbook's safety section next to RF hygiene.

## F2-8 — EAS/compliance note unchanged; add document-handling note

The DAC ops manual and any key material derived from it are sensitive-adjacent: store in the repo's git-crypt tier (or outside git entirely), same discipline as recovered box dumps. No other compliance change.

## F2-9 — Process: milestone reordering justified

The crypto-oracle path (F2-2 path 1) inverts the rev 2 assumption that M3/M4 are purely software-gated. Recommended: run the surplus-hardware watch **from day one** (it has long lead time and gates nothing), and let M3 planning decide between the four R1 paths with the ops manual in hand. Kill criteria (F15 of round 1) unchanged — but the fallback now reads "M2 + M5, with crypto via oracle if acquired."

---

## Recommended rev 3 changes

1. R1 rewritten around four named paths (oracle / TvPC / INIT-flow / firmware analysis).
2. Option D added to §3-adjacent architecture: hybrid headend (real DAC-6000 for crypto, rust-dac for everything else).
3. R6 downgraded; M5 reframed as closing the poll/ack loop.
4. O1 reframed: reference hardware over captures; surplus watch (DAC-6000, OM-2000, TvPC cards) starts immediately.
5. Census adds: manufacture date + power-cord inspection (F2-7); designate instrumented DCT2500 (F2-6).
6. New research items: acquire DAC-6000 ops manual (F2-1); probe DCT2500 serial console.
7. References: SEC IP license agreement (MediaCipher IC family), Scribd DAC ops manual, DCT2500 installation manual, CPSC recall notice.
