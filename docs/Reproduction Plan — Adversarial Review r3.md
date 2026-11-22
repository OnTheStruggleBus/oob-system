# Reproduction Plan — Adversarial Review r3

- **Date:** 2026-11-21
- **Target:** `rust-dac Reproduction Plan.md` (current rev, incl. W8 + 13-frame
  i-Guide analysis)
- **Stance:** hostile. Attack assumptions, dependency honesty, and operational
  realism. Prior rounds: r1/r2 against ADR-001 (separate documents).

## Verdict

The plan survives structurally — the box-as-oracle pivot, FPK strategy, kill
criteria, and residency-first guide strategy all hold under attack. What does
*not* survive unchanged: (1) the discovery loop's feedback path is manual and
will straggle the critical path; (2) flash-extraction capability is a hidden
shared dependency of W6 recovery and the W8 donor-object plan; (3) the O13
listings-transport search space is unbounded; (4) the R12 substitute grid is
priced wrong — it is a native 68k application project, not a fallback; (5)
several hard requirements (codec, RF chain, time message, always-on guide
carousel) are missing entirely. Eleven findings; all are plan deltas, none
invalidates the approach.

## Findings

### F3-1 (High) — M0.5 is serialized on acquiring SCTE 55-1 (O7)

**Attack.** W1 depends on O7, and M0.5 gates M1. The 2009 revision is a
paywalled standards-store item; availability, price, and delivery latency are
outside our control. The plan's first-two-weeks list puts acquisition first,
but there is no defined fallback if it stalls — the plan says "acquire before
brute-forcing anything," which reads as a hard block.

**Why it matters.** The single most parallelizable workstream (PHY bench)
becomes hostage to a purchasing task.

**Delta.** Define the no-spec fallback explicitly: we already know carrier
set, symbol rate, and constellation; the unknowns are FEC, interleaving, and
framing — a *small, enumerable* candidate space (period DCII-era coding
schemes) that the d 02 data-activity indicator can oracle just as well as it
oracles messages. Timebox O7 (e.g., 2 weeks); if it lapses, PHY discovery
proceeds via the same box-as-oracle loop one layer down. Update W1's
"Depends on: O7" to "O7 preferred; oracle fallback defined."

### F3-2 (Critical) — Discovery-loop throughput is manual

**Attack.** §3's feedback list — read the front panel, scrape d-screens by
hand, eyeball the OSD — is per-iteration manual labor. The harness contract
("every experiment is a commit") records box response, but nothing automates
*collecting* it. The connect-message search may need hundreds of iterations
across header layouts, checksum variants, and address modes. At even five
manual minutes per iteration the critical path dies of tedium, and humans
start skipping the log — which destroys the protocol document's evidence
chain, the plan's own risk control.

**Delta.** Build the capture rig *before* connect experiments, as a W3
sub-task with its own mini-milestone: (a) USB camera on the front panel with
OCR for 7-seg states; (b) cheap HDMI/composite capture card on the box video
out — the d-screens and OSD are rendered there, so screenshot + OCR scrapes
d 01/d 02/d 10/d 15 automatically; (c) RS-232 console scrape wired into the
harness (O9). Acceptance: harness records box state with zero human
interaction per iteration. This is the highest-leverage purchase in the whole
plan (camera + capture card ≈ tens of dollars).

### F3-3 (High) — Flash extraction is a hidden shared dependency

**Attack.** Two plan elements silently assume we can read box flash:
(a) W6's "sacrificial-box protocol with **verified flash recovery**" —
recovery from a bad download requires reading/writing flash out-of-band;
(b) W8's donor-object plan — "dump the object" from a donor box requires the
same read capability. Neither appears in any milestone, BOM, or the
first-two-weeks list. For m68k DCT2000s the routes are BDM (probe + gdb,
connector availability unverified per board rev) or flash chip removal/clip
to a TL866-class programmer. Both need hardware acquired and validated
*before* W6's first download enable — and before W8's donor path can even be
assessed.

**Delta.** Add a D0 acquisition/validation task (target: before M1): TL866 II
Plus + TSOP adapters and/or a 68k BDM probe; validate end-to-end on the
sacrificial box (full flash read, checksum, write-back). Gate W6's first
download enable on that validation. Add both to the dependency table: W8
donor path depends on D0 + W6, not merely "W3, W4."

### F3-4 (High) — R12 substitute grid is priced as a fallback; it's a native-app project

**Attack.** Fallback (b) is "a substitute grid app — explicitly degraded."
Understated. A substitute grid on DCT2000 means authoring a native 68k
application against an undocumented platform API, delivered through the very
CDL we haven't validated, then themed Navigator/Passport. That is a software
project comparable in risk to the entire W6 workstream — as a *fallback*.
If the donor path fails, this "fallback" fails the same way (it also rides
CDL and benefits from zero platform docs).

**Delta.** Insert fallback **(c) before the native app**: a **headend-rendered
video guide channel** — grid rendered from `lineup.yaml` server-side and
played out as an ordinary MPEG-2 channel (Prevue/TV Guide Channel style —
period-authentic by definition). Zero box-side code, zero CDL dependency,
works on every connected box the day clear-tier playout works. The
Navigator/Passport theme applies to the *renderer*. Native substitute app
drops to (d), explicitly optional. Cheap authenticity beats expensive
authenticity as a fallback.

### F3-5 (High) — O13's transport search space is unbounded

**Attack.** §4.1 says listings ride "the OOB stream," citing the DAC's EPG
Source-ID mapping and EPG-inserter housekeeping — good evidence, but
period TV Guide/Gemstar ecosystems also used in-band private PIDs and,
earlier, VBI data insertion. If the resident app actually parses an in-band
carousel on a specific PID (e.g., on a guide barker service), every
OOB-injected candidate record produces *no feedback at all*, and the
genre-color per-field oracle never lights up. The plan's discovery order for
O13 does not identify transport first; a wrong default wastes the most
expensive resource (box iterations, see F3-2).

**Delta.** Add a transport-identification stage to O13 *before* record-format
guessing: (a) patent triage specifically for Gemstar's Motorola-era delivery
(transport claims, not data structures); (b) donor-box flash strings (F3-3)
— the app binary will name its data source/PDU types; (c) d-screens on a
donor box that still holds stale guide data may show source PIDs; (d) only
then candidate injection, OOB first, in-band private PID second. Also record
that the Guide Colors Setup finding (frame 11) weakens color-as-oracle on
non-default boxes — census must verify default palette (already noted in
plan; cross-link it to O13).

### F3-6 (Medium) — Codec requirement missing: the fleet decodes MPEG-2, not H.264

**Attack.** W4 says "ffmpeg/tsp playout" without pinning codecs. DCT2000-era
decoders are MPEG-2 MP@ML (SD) parts; H.264 playout would render as nothing
and could masquerade as a tuning/PSI failure — a debugging trap of our own
making. Audio likewise: AC-3 or MPEG-1 Layer II, not AAC.

**Delta.** Pin in W4 and in requirements: video MPEG-2 MP@ML 480i (4:2:0),
audio AC-3 @ 192–384 kbps or MP2, CBR transport per J.83 Annex B 256-QAM
(≈38.8 Mbps) with tsp policing; barker/EPG channels may run lower bitrates.
Add a decode-smoke-test to M2 exit on the oldest fleet box.

### F3-7 (Medium) — RF transmit chain is unspecified

**Attack.** The plan names SDRs but not the chain between SDR and plant.
HackRF-class outputs are spectrally dirty and far below cable levels;
reaching the box's −10…+5 dBmV window at the tap without adjacent-service
splatter needs a band-pass filter, a gain block, attenuation, and a
directional coupler — plus a dummy-load-first bring-up rule. r1 set the
level budget (QAM ≈ 0 dBmV, OOB ≈ −5 dBc); nothing turns it into hardware.
Also: three candidate carriers exist (75.25/72.75/104.2) and the plan
doesn't say we transmit **one** primary — nor which. (Census: read what the
fleet actually scans/locks on d 02.)

**Delta.** Add an RF-chain section to requirements: filter + PA + step
attenuator + coupler/tap + terminators; dummy-load-first, leakage check
before connecting the plant (compliance continuity from ADR r1/F10); primary
carrier decided at O2 census, others relegated to test fixtures.

### F3-8 (Medium) — M1 exit over-specifies on DCT2500 instrumentation

**Attack.** M1 exit requires "(DCT2500) d 15 logs our connect type" and the
first two weeks say "pick the instrumented DCT2500; O9 console probe." But
the census hasn't confirmed a DCT2500 exists in the fleet, and RS-232
consoles of the era are often disabled, password-gated, or
hardware-incomplete. As written, M1 can be fully achieved technically and
still fail its own exit criteria.

**Delta.** Restate M1 exit as tiered: core = `E 00` + d 02 data activity +
`PID 1 received` on a fleet box (sufficient evidence of connect);
instrumentation tier = d 15/RS-232 *if* the census yields an instrumentable
unit. Census (O2) explicitly designates the instrument or records its
absence.

### F3-9 (Medium) — Time service is absent from the discovery backlog

**Attack.** Everything date-driven — grid "Today," day-skip, scheduling, the
box clock in every uploaded frame — depends on GI time distribution. The
DAC manual documents time-zone auto-config (V2.90+) and time housekeeping,
but the §2 catalog backlog has no time message row, and W4 lists "time
service" as a task without a discovery owner. If boxes can't get time,
W4/W8 validation against the reference frames (clock top-right everywhere)
silently fails.

**Delta.** Add "Time/date message" to the §2 backlog and W3 discovery order
(ahead of W4 exit); headend time base = NTP-synced host, wire format
discovered like any other family.

### F3-10 (Medium) — The guide feed is an always-on service; no ops model exists

**Attack.** Init/cold-init clears "guide data" (documented); boxes cache
listings and drain. Therefore the listings carousel must run *continuously*
— it's a service with an uptime requirement, not a build artifact. Nothing
in the plan covers watchdog/monitoring (estate has Zabbix/VictoriaMetrics
conventions), carousel health metrics, or the **repopulation latency** a
cold box experiences on power-up (full carousel cycle). An M6 demo that
works because the carousel was just restarted is not M6.

**Delta.** Requirements: guide-carousel service under watchdog with Zabbix
alerting (estate pattern); M6 exit gains "cold box (post-init) repopulates
the grid within N minutes of connect, unassisted" with N measured during
W8; nightly canary (already in ops/ci) asserts grid population, not just
AUTH.

### F3-11 (Low) — EAS is scheduled as independent polish; it rides message discovery

**Attack.** W7 lists EAS as a peer task. On GI boxes EAS is a force-tune +
crawl driven by OOB messaging — i.e., another family in the same discovery
backlog. Framing it as independent invites scheduling it before its
dependency exists.

**Delta.** Mark EAS in W7 as "post-connect-family message work, reuses W3
harness"; no separate discovery mechanism.

## What survived hostile review

- Box-as-oracle as the conformance instrument, with experiment-per-commit
  evidence — *provided* F3-2's capture rig exists; otherwise the method is
  sound and the throughput is not.
- FPK strategy, per-unit-key bypass, and the kill criteria (FPK failure →
  done = M2+M4+M5). Also survives: F3-3's flash capability slightly
  *strengthens* O3 route (a) (firmware-assisted constant hunt).
- WireLink verbs as the rust-dac admin API; `lineup.yaml` as single source
  of truth; Cold Initialize as the recovery primitive.
- Residency-first guide strategy (donor before substitute) — with F3-3's
  dependency honesty and F3-4's re-priced fallback chain.
- The 13-frame i-Guide analysis; no finding touches it beyond F3-5's
  color-oracle cross-link.

## Requested plan deltas (for Marshal's disposition)

1. W1: define no-spec PHY oracle fallback; timebox O7 (F3-1).
2. W3: add capture-rig mini-milestone *before* connect experiments (F3-2).
3. New task D0: flash-extraction capability (TL866/BDM) acquired + validated
   on the sacrificial box before M1; W6 gated on it; W8 donor path depends
   on D0 + W6 (F3-3).
4. R12: insert fallback (c) headend-rendered video guide channel; native
   substitute app demoted to optional (d) (F3-4).
5. O13: transport-identification stage before record-format guessing (F3-5).
6. W4: pin MPEG-2 MP@ML + AC-3/MP2 + Annex B CBR; M2 decode smoke test
   (F3-6).
7. New requirements section: RF chain BOM + dummy-load-first + primary
   carrier from census (F3-7).
8. M1 exit tiered (core vs instrumentation); census designates instrument
   (F3-8).
9. Backlog + W3: add Time message family (F3-9).
10. Requirements: guide-carousel always-on service + Zabbix watchdog; M6
    exit gains cold-box repopulation latency (F3-10).
11. W7: EAS marked as post-connect message-family work (F3-11).

## Disposition (2026-11-22 — Marshal, G0.5 agenda)

**All 11 findings ACCEPTED**, with amendments as noted; deltas applied to
the Reproduction Plan, Milestones & Stage Gates, and Hardware Acquisition
List in the disposition commit set.

| # | Disposition | Note |
| --- | --- | --- |
| F3-1 | Accept, narrowed | Install manual (§3.1) documented carriers/hunting/acquisition after this review; O7 shrinks to coding/framing unknowns only. 2-week timebox stands. |
| F3-2 | Accept | Capture rig is E1; precedes connect experiments. |
| F3-3 | Accept | Strengthened by r4 F4-3 — D0 gates W6 + W8 + O13. |
| F3-4 | Accept | Headend-rendered video guide channel is fallback (c); native app demoted to optional (d). |
| F3-5 | Accept | d 14 OOB-vs-INB counters are the transport-ID instrument. |
| F3-6 | Accept, amended | Audio pin refined by F5-7 (MP2 primary; AC-3 per census). |
| F3-7 | Accept | RF chain = filter + PA + attenuator + coupler/tap + terminators; dummy-load-first. |
| F3-8 | Accept, softened | Install manual documented d 14 fleet-wide; instrumentation tier no longer DCT2500-gated. |
| F3-9 | Accept | Absorbed into F5-8's mechanism (hypothesized message class in oob-msg). |
| F3-10 | Accept | Always-on carousel + Zabbix watchdog + M6 cold-box repopulation exit. |
| F3-11 | Accept | EAS = post-connect message-family work; registered zero-weight M6 backlog (F5-9). |
