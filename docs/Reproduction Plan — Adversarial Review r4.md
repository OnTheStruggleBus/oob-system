# Reproduction Plan — Adversarial Review r4

- **Date:** 2026-11-22
- **Target:** `rust-dac Reproduction Plan.md` (current rev, incl. §3.1
  install-manual findings, §8 decided tech stack, SEM V8 hardware path,
  Docker deployment, Passport Echo reference)
- **Stance:** hostile. Attack the *new* attack surface since r3 — the SEM in
  the signal path, the Pluto OOB assumption, the Docker deployment, and
  anything r3 missed that is still live. Prior rounds: r1/r2 against ADR-001,
  r3 against this plan (11 findings, disposition still pending).

## Verdict

The plan survives structurally again — the SEM-centric RF architecture, the
revised kill criteria, and the stack decisions hold under attack. What does
*not* survive: (1) the claim that a 104.2 MHz OOB carrier is "inside Pluto
range, no 70 MHz hack" is **factually wrong** — the whole hunt table is
below the stock Pluto's 325 MHz floor, and the level budget may not close
down there; (2) the SEM is now a single point of failure for *both* W4 and
W5, gated by a legacy-Java provisioning chain, and W4 no longer has a
documented fallback; (3) D0 flash extraction has quietly become load-bearing
for three workstreams, and the O13 listings format may be opaque to
mutation-based discovery. Eight findings; all are plan deltas, none
invalidates the approach. Two factual corrections to plan text are required
(F4-1, F4-8).

## Findings

### F4-1 (High) — Pluto at 104.2 MHz still needs the range hack, and the level budget is unproven

**Attack.** §7 item 5 and §8's OOB bullet state the high hunt carriers
(104.2/110.25 class) are "inside Pluto range, no 70 MHz hack." The stock
ADALM-Pluto (AD9363) covers **325 MHz–3.8 GHz**. The *entire* ten-carrier
hunt table (75.25–116.25 MHz) sits below that floor; reaching any of it
requires the same unofficial AD9364-identity firmware hack as 70 MHz. Worse,
the Pluto's RF front end is unmatched in the 70–150 MHz region: community
measurements show output power falling off steeply below ~150 MHz with
rising spurious content. Our level budget has to deliver −10…+5 dBmV at the
box input *after* the bandpass filter, combiner, and tap losses (§3.1) — a
weakened, spiky source through a lossy chain may not close, and we won't
know until we meter it. The plan currently asserts a carrier strategy on an
unmeasured assumption and misstates the hardware's stock capability.

**Why it matters.** W1's bench plan and the buy-list PA/filter sizing both
depend on the actual deliverable level at 104 MHz. If the Pluto comes up
short, the "no additional hardware" OOB story silently acquires a PA
requirement — or a different radio.

**Delta.** (a) Correct §7 item 5 and §8: the range hack is required at
*every* hunt carrier; (b) at bench, apply the hack and **meter actual
delivered level at 104.2 MHz through the full F3-7 chain** before
committing; if short, add the small PA the BOM already anticipates;
(c) reassign roles: the buy-list **HackRF One (native 1 MHz–6 GHz) becomes
the primary OOB TX** — it covers the whole hunt table in spec — and the
Pluto becomes spare/second-carrier TX or upstream monitor. The bandpass
filter stays mandatory regardless (Pluto spurs + FM-band adjacency, F4-8).

### F4-2 (High) — The SEM is now a single point of failure for W4 *and* W5, behind a legacy-Java provisioning chain

**Attack.** Since §8, every path to in-band RF runs through one used
2008-era unit. Three unaddressed failure modes:

1. **Provisioning deadlock.** SEM-EM requires JRE 1.2.2–1.5 served by the
   unit's onboard HTTP interface. If that server or its SEM-EM payload is
   dead/factory-wiped, and the RS-232 port (Appendix E) exposes only
   test/diagnostics rather than full provisioning, the unit is an
   unprovisionable brick — blocking M2 *and* M3 simultaneously. The revised
   kill criteria cover only the FPK tier; a dead SEM kills the clear tier
   too, and W4 currently has **no documented fallback** (the old SDR-QAM /
   HDMI→QAM options were deleted, not preserved).
2. **Feature licensing.** Era-typical Motorola practice: internal routing
   modes and encryption features could be license-keyed. A pulled unit
   usually carries its licenses in flash — but if FPK mode or internal
   routing is greyed out in SEM-EM, the ACP's physical presence (confirmed
   via P/N) doesn't save M3. This is checkable in the first bench session
   and currently isn't on the list.
3. **Age.** Fifteen-plus-year-old capacitors and fans; no spare on hand.

**Delta.** (a) **First-contact archive:** at first bench session, download
and commit everything the SEM's HTTP server exposes — SEM-EM jar, any MIB
files, help pages — into the repo (git-crypt if license-sensitive); if the
unit later dies, its provisioning tooling survives. Record the default-IP /
factory-reset procedure in the runbook. (b) **License/mode verification:**
in the same session, confirm FPK mode and internal UDP-port routing are
*selectable*, not merely present in menus; log the firmware rev and license
state. (c) **W4 fallback re-documented:** if the SEM dies or won't
provision, M2 fallback = HackRF-based software QAM at reduced channel count
(one Annex B 256-QAM carries the whole clear tier at ~38 Mbps) — cheap,
in-scope, and uses a radio already on the buy list. (d) **O1 surplus
watch:** add a spare SEM (the ACP-less 507199-002 suffices for clear tier;
another -001 for full redundancy) as pure opportunism per Marshal's standing
direction. (e) Note the DAC-as-controller / SNMP provisioning path as the
deep fallback — obtain Motorola SEM MIBs if the unit exposes them.

### F4-3 (High) — D0 is load-bearing for three workstreams; O13 may be opaque to mutation

**Attack.** r3's F3-3 made D0 (flash extraction) a shared dependency of W6
recovery and the W8 donor path. Two developments since make it more
central, and neither is reflected in the plan:

1. **O13 format opacity.** Period Gemstar/StarSight listings feeds were
   famously compressed with proprietary schemes. If the DCT-resident
   i-Guide listings stream is compressed or otherwise serialized opaquely,
   the box-as-oracle loop degrades badly: single-field probes don't survive
   a compressor, so "mutate one field, watch one grid cell" stops working
   and the genre-color feedback channel (§4.1) goes dark. Transport-ID
   (F3-5) still works, and whole-record replay from captured data would
   work — but we have no captured live feed, and self-authored listings are
   the plan's stated source content.
2. **The parser is on the board.** The fallback oracle for an opaque format
   is the resident app's parser/decompressor itself — reachable via the D0
   flash dump plus the §3.1 RE anchors (the d14 message-ID enum registry
   points straight at the message handlers; the feed parser hangs off the
   same dispatch). That makes D0 load-bearing for **W6 recovery, W8 donor
   re-serve, and O13 parser RE** — three workstreams — yet it still appears
   in the plan only as a buy-list line and a first-two-weeks absence.

**Delta.** (a) O13 gains an explicit **format-opacity check** stage right
after transport-ID: delta probes of increasing granularity; if grid
response is non-linear to field mutation, flip discovery to the RE path
immediately instead of burning cycles. (b) Extend the O10 patent pass to
target Gemstar decompression tables specifically. (c) Promote D0 in the
plan text to a named pre-M1 milestone task with **two validated extraction
routes** (TL866 + 68k BDM), precisely because it now gates three
workstreams; its failure mode (neither route works) should appear in the
risk register with the mitigation (sacrificial-box board-level practice,
alternate flash packages per board rev).

### F4-4 (Medium) — Fleet cold-boot reality: E09/E11 precede connect

**Attack.** §3.1 documents that E09 (battery/NVRAM lost) "requires factory
initialization message" and E11 (invalid unit address) "requires a unit
creation message" — but W3's discovery backlog still starts at *connect*.
Fleet boxes are ~25 years old; soldered lithium NVRAM cells will be dead on
some units, and those boxes will sit at E09/E11 ignoring connect messages
until the earlier messages exist. The plan's own canary (reboot box, assert
AUTH) will exercise this path every run, so it will surface — but as an
unplanned blockage on the critical path rather than a sequenced backlog
item.

**Delta.** Reorder the W3 backlog: **factory-initialization → unit-creation
→ connect** → VCM/region/etc., citing the manual's own remedy column.
Add NVRAM battery replacement to fleet maintenance and the buy list
(soldered lithium cells + holders; log per-box battery state at census).
Note the operational upside: a headend that can factory-init makes
dead-battery boxes *fully functional* with zero soldering — lead with that
in the canary design, fix batteries opportunistically.

### F4-5 (Medium) — Donor objects don't cross platforms

**Attack.** The W8 donor plan says "dump the object, serve it back to the
rest of the fleet." But the fleet is mixed-platform: DCT2000 = 68k, DCT2500
= Broadcom (MIPS), and the DVR models add more variants. A guide object
dumped from a DCT2500 cannot run on a DCT2000; if the census finds the
guide resident on only one platform, the donor path serves only that
platform, and "donor supply is not a constraint" (§4.1) holds only
per-platform. The CDL platform→object compatibility table is already noted
as F0 output, but its consequence for M6's exit criteria is not drawn.

**Delta.** F0 census records guide build + Platform/Family/Model ID (d 01)
+ module list (d 10) per box; the compatibility table is keyed on those
IDs; e-waste donor sourcing (O1) specifies the *target platform*, not just
"DCT-class." M6 exit criteria name the platforms covered ("≥1 box" → "≥1
box per platform family the census found"). No schedule change; this is a
precision fix to census and exit language.

### F4-6 (Medium) — In-band SI must be DCII-flavored, not vanilla MPEG

**Attack.** §2's catalog mentions the CAT carries EMM PIDs, but W2's mux
requirements stop at generic PAT/PMT/CAT/NIT. DCT2000s expect
DigiCipher-II-flavored signaling: CA descriptors carrying GI/Motorola
CA-system IDs, DCII table conventions, and (on the encrypted tier) the
MediaCipher entitlement structure the box's d 15/d 16 screens were built to
dump. A vanilla tsp mux may produce streams the box *tunes* (video is just
MPEG-2) while ignoring our EMMs or mishandling authorization state — a
failure mode that looks like a W5 crypto problem but is actually a W2 SI
problem, and could send the FPK validation down a wrong debug path.

**Delta.** Add an O4 backlog row — "DCII PSI/SI conformance set" — tracked
with the same documented/hypothesized/verified status as the message layer;
d 15/d 16 PAT/PMT dumps (§3.1) are the validation instrument. W2's exit
criteria gain: box d 16 shows our PMT with the expected CA descriptors.
Source material: the DAC and SEM manuals' CA-parameter sections, plus
whatever the d-screens reveal about what the boxes parsed from period
captures (O1 community angle).

### F4-7 (Low) — Docker networking details are unaddressed

**Attack.** §8's deployment paragraph asserts containers are clean because
everything is TCP. Mostly true — but playout's UDP **multicast** from
inside a container needs host networking or macvlan to behave, IGMP
interaction with the SEM's GigE port is untested, and gr-iio's connection
to the Pluto's USB-net gadget depends on the gadget's link-local addressing
surviving container restarts. None of these are hard; all of them are
surprise-shaped if discovered during M2 instead of designed now.

**Delta.** One deployment-notes paragraph: host networking (or macvlan) for
the playout container, explicit multicast group/port convention in
`lineup.yaml`, the managed switch (already on the buy list) between playout
host and SEM to keep multicast clean, and a container-restart canary that
re-asserts the Pluto connection. No architecture change.

### F4-8 (Low) — Frequency housekeeping: 861 vs 860, band plan, FM adjacency

**Attack.** Three small factual/consistency items: (a) the SEM tunes to
**861 MHz** but the box tuner passes **54–860 MHz** — keep the lineup
≤860 MHz; (b) STD vs HRC/IRC is a free choice *we* define via the VCM, but
the SEM's plan setting, `lineup.yaml`, and the VCM must agree — pick
deliberately (STD is the honest default) and record it; (c) the OOB
carriers sit in the FM broadcast band (104.2/107.25/107.40 MHz) — a closed,
terminated plant radiates nothing of consequence, but this is a third
reason the dummy-load-first discipline and shielded passives in the F3-7
chain are policy, not preference.

**Delta.** One line each in §8's RF architecture notes. No cost.

## What holds up under attack

- **The OOB = MPEG-TS-framed hypothesis** (implicit in W2's skeleton:
  "PAT/PMT/CAT + nulls at 1.024 Msym/s") is *corroborated* by box-side
  evidence: d 02 reports "PID 1 received" and d 14 counts PIDs on the OOB
  side — PIDs are an MPEG-TS concept. The plan should state this lead
  hypothesis explicitly in O4; it's currently scattered across §3.1 and the
  W2 row. (If true, SCTE 55-1's framing layer likely carries 188-byte
  packets, which also makes the insertion-bandwidth shaper arithmetic
  match the manual's ~1000 Kbps figure.)
- **The ACP-via-part-number decode** (507199-001 = "Full Edge, ac" → ACP
  Yes) is sound against manual Table 2-2, and the revised kill criteria
  correctly require a *bench failure* rather than a missing feature to
  trigger.
- **The Docker call** holds: the two hard-to-containerize interfaces (USB
  for the Pluto, none for the SEM) are both network-presented in this
  design. F4-7 is polish, not a reversal.
- **The residency-first guide strategy and the R12 theming fallback**
  (Navigator/Passport, strengthened by the Cincinnati Bell Passport Echo
  frame) survive; F4-3 and F4-5 sharpen their execution but don't
  challenge the direction.
- **The SEM-EM Java-VM wrinkle** is correctly identified in §7; F4-2 turns
  it from a provisioning inconvenience into a managed risk (archive,
  license check, documented fallbacks).

## Disposition summary

| # | Severity | One-liner | Plan delta required |
| --- | --- | --- | --- |
| F4-1 | High | Pluto needs the range hack at 104.2; meter the level; HackRF becomes primary OOB TX | §7, §8 text + buy-list role change |
| F4-2 | High | SEM SPOF for W4+W5; archive SEM-EM, verify licenses, document W4 fallback, spare on O1 | §7, §8, W4 row, O1 |
| F4-3 | High | D0 now gates W6+W8+O13; O13 opacity check; two extraction routes | W3/O13, D0 task, risk register |
| F4-4 | Medium | E09/E11 precede connect; reorder W3 backlog; batteries on buy list | W3, §7, buy list |
| F4-5 | Medium | Donor objects are per-platform; census and M6 exit language | F0, §4.1, M6 |
| F4-6 | Medium | DCII SI conformance row in O4; W2 exit via d 15/d 16 | §2/O4, W2 |
| F4-7 | Low | Docker multicast/macvlan/Pluto-reconnect notes | §8 deployment |
| F4-8 | Low | ≤860 MHz lineup, STD/HRC consistency, FM-band shielding note | §8 RF notes |

All eight await Marshal's disposition, alongside the still-open r3 set.
