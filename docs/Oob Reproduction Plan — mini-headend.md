# Oob Reproduction Plan — mini-headend

*Renamed 2026-11-22 with the naming adoption: the software is **Oob**
(OOB — Out-of-Band; *an out-of-band headend for DCT-class cable boxes — an
MVPD of one*), the crates are `oob-*`, and the guide stack is **VueGuide+**.
Formerly "rust-dac Reproduction Plan".*

- **Date:** 2026-11-21
- **Status:** Proposed — companion to ADR-001 (rev 8)
- **Dispositions folded (2026-11-22, Marshal):** adversarial reviews r3 (×11), r4 (×8), r5 (×14) all accepted with the amendments recorded in each review's Disposition section; the deltas below carry their finding refs
- **Premise (Marshal's call):** the reproduction is the path. Surplus hardware is demoted to pure opportunism — no planning weight, no dependencies. Everything the project needs is built, or discovered with the boxes themselves as the oracle.

---

## 1. What we have vs. what we must discover

| Layer | Status | Source of truth |
| --- | --- | --- |
| Headend semantics (ops, lifecycle, download, encryption modes, provisioning) | **Known** | DAC-6000 + SEM V8 manuals (see Operations Reference) |
| Box-side observable contract (error codes, state machine, diagnostics, download LEDs) | **Known** | DCT2000 Installation Manual (Dec 2001, 86 pp — **in hand** as `7370782.pdf`, archive.org item manualzilla-id-7370782) + DCT2500 install manual |
| MPEG framing (PAT/PMT/CAT/NIT, PID concepts; CAT carries EMM PIDs) | **Known** | Standards + DAC manual §2 |
| PHY (QPSK 1.024 Msym/s @ 75.25/72.75/104.2 MHz + 7 more hunt carriers documented in the DCT2000 manual d 02; 1.5 MHz BW, −10…+5 dBmV at box input; coding/framing details) | **Acquirable** | ANSI/SCTE 55-1 (2009) — O7; carrier table + levels already in hand from the DCT2000 manual |
| **GI OOB message wire formats** (connect, VCM, region, inband-processor, general, phone, EMM, download control) | **TO DISCOVER** | Box-as-oracle loop (§3) + SCTE 55-1 + patents (O10) |
| DLS carousel segment framing | **TO DISCOVER** | Box-as-oracle + §12 object model |
| FPK constant + scrambling flavor (DES vs CSA) | **RESOLVED IN HARDWARE** | SEM V8 in hand (2026-11-22): ACP module does FPK mode — no EMMS needed — with DES **or** CSA as a config choice; O3 extraction demoted to optional knowledge |
| i-Guide app residency per box | **CENSUS** | d 10 module list at O2 (O12) |
| i-Guide listings data format | **TO DISCOVER** | Box-as-oracle + Gemstar/StarSight patent trove (O13); no public spec or hobbyist RE found |
| STARFONE poll protocol | **TO DISCOVER** (stretch) | Simulated-line sniff + box counters (O5) |

The discovery surface is deliberately small: one message layer, one carousel
format, one constant. Everything else is engineering.

## 2. Already in hand: the message operations catalog (DAC manual §11)

The DAC's per-terminal "Manage Operations" menu is effectively our backlog —
each entry is a message family oob-dac must emit, with documented *contents*:

| Op (DAC §11) | Payload (documented) |
| --- | --- |
| Connect Message | forward-direction connect (the E 01 killer) |
| Virtual Channel | VCM/channel-map information |
| Control Channel Timeout | how long the box waits for DAC signal before timing out |
| Init / Refresh Authorizations | authorization state |
| Inband Processor | EMM provider ID, network unit address, Multicast-40/24/16 addresses |
| Region | country code, currency region, EPG region, rating region |
| Interactive | upstream/downstream ID assignments |
| General | log port enable, **code download enable**, RF bypass, purchase authentication |
| Multicast-16 | multicast-16 values |
| Phone | line request, dial-retry limit/spacing, inactivity timeout, **phone #1/#2, baud rate** |
| Network (DVB only) | DVB network config (skip — we're GI) |

Plus OM-side parameters that define our mux budget: **Insertion Bandwidth**
(typ. 1000 Kbps for DAC OOB comm), background-service PID allocation, and the
Interactive Downstream Path ID shared with the NC. These seed the protocol
document (O4) — every row gets wire-format status tracked as discovery proceeds.

The DAC's **Define Operations** window adds the bulk/scheduled half of the
catalog — operations targeted at All Terminals / a Terminal ID Range / a
Single Terminal, filtered by RADD, upstream/downstream plant, terminal type,
RPD channel, or Business System, with **Delay Between Terminals ≥250 ms** and
optional Repeat-Every scheduling. User-defined operation types: **Refresh,
Purchase Poll, Diagnostic Poll, Chronic Non-Responding Poll**, Category Epoch
Transition, Flush and Fill, Fill Only, Resource OCAM, Synchronize CPMS.
Auto-generated operation types (the DAC schedules these itself): Global Auth
Refresh, Channel Map, Multiplex, **Insert System Time**, **Code Download**,
Analog Rekey, Analog Clear Service, PPV Window Auth, CEC EMM Generation,
Macrovision, Physical Channel Map, Periodic ERS EMM Generation, **Compute DST
Interval**, CableCARD SNT, CP Phone Number. Each operation carries a visible
status lifecycle (New → Request Sent → Timeout → Downloaded → Retrieving
Status → … → Completed/Failed) — our scheduler and the Console's ops views
use these status names. The arrows follow the manual's list order (p. 11-4),
not a documented transition sequence. The definitions are in the DAC-6000
Operations Reference §10. Oob's transition table is a `hypothesized`
reconstruction, recorded on
[#22](https://github.com/OnTheStruggleBus/oob-system/issues/22#issuecomment-7106984109).
Notable for the backlog: the poll family and
**Disconnect** are first-class messages we must emit, not side effects; and
"Insert System Time" is the manufacturer's own name for the r3 F3-9 Time
message, with Compute DST Interval as its companion.

## 3. Discovery methodology — box-as-oracle

No DAC oracle, so the boxes themselves become the conformance instrument. The
loop is fast and cooperative — the box *wants* to connect and its diagnostics
report exactly how far it got:

```javascript
┌────────────┐  candidate message   ┌────────┐   RF    ┌─────────┐
│ oob-dac    │ ───────────────────► │ GNU    │ ──────► │ DCT on  │
│ message lab│                      │ Radio  │         │ plant   │
└────────────┘                      └────────┘         └────┬────┘
       ▲                                                    │
       │         observable feedback                        │
       └────────────────────────────────────────────────────┘
  • front-panel OSD state (E 01 → E 00, CONFI/ACQUI/AUTH)
  • diagnostics: d 01 (errors/IDs), d 02 (OOB lock, data activity, PID 1,
    EMM activity), d 10 (download modules) — full d 01–d 18 set documented
    in the DCT2000 Installation Manual Appendix B
  • **Message-type sniffer is fleet-wide:** the DCT2000 manual documents
    d 14 **Message Types** (PID + message-ID counters, OOB *and* in-band) —
    the DCT2500 manual numbers the same screens d 15/16/17. Not a
    DCT2500-only instrument (upgrades the F3-8 instrumentation tier);
    RS-232 console scrape stays DCT2500-only (O9)
  • STARVUE II upstream sniffer (SDR RX): box poll responses reveal what
    it accepted
```

Discovery tactics, in order of preference:

1. **SCTE 55-1 spec (O7)** — it's the *transport* spec; may define FDC framing
directly. **Timeboxed to 2 weeks (F3-1).** If it lapses, PHY discovery
proceeds via the box-as-oracle loop one layer down: carrier set, symbol
rate, and constellation are already documented (§3.1), so the unknowns are
FEC, interleaving, and framing — a small, enumerable candidate space of
period DCII-era coding schemes that the d 02 data-activity indicator
oracles just as well as it oracles messages.
2. **Structure-guided construction** — GI messages of this era are typically
header (address/type/length) + body + checksum. The manual gives us the
semantic fields per message (§2 table); the box's `PID 1 received` and
data-activity indicators tell us when framing is right even before content
is. Multicast-40/24/16 addressing hints at the address-field taxonomy.
3. **Patent pass (O10)** — GI's own patents may document message layouts;
   the Gemstar/StarSight pass targets **decompression tables** specifically
   (F4-3).
4. **Firmware-assisted (DCT2500 serial console, bcm2-style dump)** — read the
receiver's parser if the above stall.
5. **Community** — period captures, retro-cable forums; opportunistic.

Risk control: the loop is safe by construction — malformed messages leave the
box in its prior state (it's already inert), and Cold Initialize semantics are
documented if we ever wedge one. Set **Control Channel Timeout long** during
development so boxes tolerate our iteration cadence.

## 3.1 What the DCT2000 Installation Manual pins down (in hand 2026-11-22)

The manual (GI, Dec 2001; archive.org `manualzilla-id-7370782`, uploaded by
Marshal) converts several §1 unknowns into engineering inputs:

- **Full OOB hunt table — ten carriers, fixed scroll order** (d 02):
  75.25 → 104.20 → 72.75 → 92.25 → 98.25 → 107.25 → 107.40 → 110.25 →
  116.25 → 103.75 MHz. After a cold reset the box round-robin hunts ("hunt"
  on the LED) until it finds a valid stream.
- **First concrete wire-format constraint:** the box accepts a stream only
  when it carries the proper **EMM provider ID — 0x0000 or 0x0001**.
  Receiving an `OOB_processor_config` message with a *different*
  EMM_provider_ID forces a re-hunt. Our emulator must set provider ID
  0x0000/0x0001 from day one, and the config message is the lever that moves
  boxes between carriers.
- **Manual carrier select exists** (d 02): scroll to a frequency, SELECT to
  search; 40 s without lock → warm reset back to last-known carrier. This is
  our acquisition test harness — no headend needed to point a box at our SDR.
- **RF targets for the F3-7 chain** (Appendix A): data carrier QPSK, 1.5 MHz
  bandwidth, **level window −10…+5 dBmV** at the box input; tuner passes
  54–860 MHz. Aim ≈0 dBmV at the tap — comfortably inside HackRF + filter +
  small-PA territory.
- **Official E-code table** (B-5/B-6) confirms the W1/W6 message set:
  `E 01 NOT CONNECTED` = "did not receive a connect message" (remedy:
  restore OOB + send connect); `E 09 BATTERY` = NVRAM lost/uninitialized →
  "**requires factory initialization message**"; `E 11 INVALID UNIT ADDRESS`
  → "**requires a unit creation message**". The recovery paths the plan
  assumes are the manufacturer's own remedies.
- **d 14 Message Types is a built-in protocol sniffer** (PID counters for
  OOB and in-band, message-ID counters) — and d 13 logs the MAC upstream
  frequency table + socket port states (UNUSED/OPENED/READY/RECVING/SENDING)
  for W7. d 15/d 16 dump the in-band PAT/PMT the box parses — direct
  feedback for W2 mux correctness.
- **The addressing taxonomy is printed on the box** (d 05 OSD): the unit
  address is **16-digit / 40-bit** (displayed as `nnn-nnnnn-nnnnn-nnn`), the
  box also shows a **Network Address** in the same format (the headend's
  own UA — the "network unit address" the §2 Inband Processor message
  carries), a TV PassCard address, and the **Multicast-16 address set
  broken out by flow: NET, EMM, SCC, DWLD, DATA, VCN, POLL** — seven
  labeled 16-bit groups, rendered in TCP/IP-style decimal bytes. This is
  the strongest addressing evidence yet: OOB traffic is UA unicast (40-bit)
  plus per-flow multicast-16 groups, and POLL being its own group confirms
  the poll family rides the same addressing scheme. d 05 also shows
  **Seed Health (0xFF = healthy)** — a security-seed diagnostic to scrape
  during W5. d 12 (Interactive Info) exposes the two-way sign-on state
  machine (`MAC CONNECT / UNCONFIG / INIT_WAIT… / WAIT_LM_ACK /
  WAIT_SO_ACK / WAIT_LA_OR_SO / RUNNING / …`) plus IP/upstream/downstream
  IDs — the box-side observable for the W7 STARVUE/STARFONE stretch.
- **F3-5 (O13 transport-ID) gets its instrument:** feed candidate listings
  records and watch whether the incrementing counters are on d 14's OOB side
  or INB side — transport identified before any payload is decoded.
- **Census fields for O2** (d 01): Platform ID / Family ID / Model ID
  (sample: 0x0008 / 0x0000 / 0x008F) plus tuner variant
  (V1000D2 / V1000D3C / V860 / V860D / V860DL) — record per box at census.
- **Scope note:** the platform also supports *analog* descrambling (gated
  sync suppression, dynamic gated sync, video inversion, audio privacy,
  Hamlin compatibility). Interesting period-authentic stretch, but out of
  the current digital-first scope.

O7 is unchanged but better timeboxed (F3-1): the carrier/hunt/acquisition
layer is now fully documented; SCTE 55-1 is needed only for coding + framing
of the 1.024 Msym/s stream itself.

## 3.2 What the DAC manual pins down — enrollment, targeting, and the terminal database (in hand 2026-11-22)

The DAC 6000 operation manual answers the "how do subscribers and boxes get
added" question directly — and rules out a whole class of guesses:

- **There is no over-the-air auto-enrollment for classic DCT2000s.** The
  DAC's auto-discovery polling exists, but it's **DSG/DOCSIS-specific**
  (IP-based; those terminals get `Autodiscovered` + `IP Address` fields).
  A one-way or Fone-Way DCT2000 joins the plant exactly the way the
  install manual's E-code remedies imply: the **operator adds the terminal
  to the DAC database**, then the DAC sends the enrollment message
  sequence — **unit creation** (fixes `E 11`), **factory initialization**
  (fixes `E 09`), **connect** (fixes `E 01`) — followed by the
  configuration sequence. oob-dac's unit DB *is* the enrollment
  mechanism; boxes never announce themselves into it.
- **The terminal record schema (Define Terminal window)** — this is the
  exact schema oob-dac's terminal table and the Console Fleet registry
  mirror (per-box records live in the store, not in git — ADR-003):
  Terminal Type (**One-Way / Fone-Way / Two-Way / Ethernet / DOCSIS**),
  Unit Address (16-char decimal `nnn-nnnnn-nnnnn-nnn`, **burned into
  NVRAM at manufacture**, printed on the unit), Terminal ID
  (DAC-generated, **1–16777215 = 24-bit**), Serial Number (12-char,
  burned in), Account Number (issued by the Business System), On-Plant
  flag, Last Response Date (poll-based; inactive for one-way units),
  Failures, **Power Leveling Attempts**, Non-Participating Counts (telco
  conditional dial), **Chronic Non-Responding** flag (days threshold is
  tunable), Install Date, Auto-Registration Date (DSG), Activated,
  Autodiscovered + IP (DSG/DOCSIS), BSI Name, Downstream/Upstream Plant.
- **Per-terminal actions (Define Terminal buttons)** — the W3/W6 op set,
  with exact NVRAM semantics: Authorize (IPPV / Services / Program /
  Packages / **Objects** / **Resources**); **Configure Transmission
  Parameters** (Map Name, Amplitude Level, plants, Modem Bank, RPD
  channel + frequency, interactive Downstream/Upstream IDs, impulse
  credit limits); **Initialize** (clears NVRAM *except* downloaded
  objects and view history, clears passwords, then sends the
  configuration sequence); **Cold Initialize** (clears NVRAM *including*
  download objects + static RAM, resets the user processor, returns to
  out-of-box state, then the configuration sequence; the ASTB variant
  erases flash except boot code + base platform); **Refresh**;
  **Disconnect**.
- **Bulk operation targeting (Define Operations, §2):** All Terminals /
  Terminal-ID Range / Single Terminal × filters, ≥250 ms inter-terminal
  delay, Repeat-Every scheduling, and the New→…→Completed/Failed status
  lifecycle. This is the model the oob-dac scheduler implements — every
  W6 download campaign and W8 guide-carousel refresh is a Define
  Operations row.
- **Consequence for the plan:** nothing here requires new discovery —
  enrollment is fully operator-side. The only box-visible unknowns remain
  the wire formats of the enrollment messages themselves (unit creation,
  factory init, connect, polls, disconnect), which W3 already hunts with
  the box-as-oracle loop, now with the d 05 addressing taxonomy (§3.1) as
  the address-field guide.


## 4. Workstreams and build order

| # | Workstream | Milestone | Key tasks | Depends on |
| --- | --- | --- | --- | --- |
| W1 | PHY + carrier | M0.5–M1 | Acquire SCTE 55-1 (timeboxed 2 weeks, F3-1); GNU Radio QPSK flowgraph per spec; loopback BER test; box d 02 data-activity test | O7 preferred; oracle fallback defined (F3-1) |
| W2 | oob-dac core + mux | M1 | TS writer (188B packets, async/null-stuffed), PID plan, **DCII-flavored** section assemblers (PAT/PMT/CAT/NIT + CA descriptors, F4-6 — O4 gains a DCII SI conformance row, documented/hypothesized/verified like the message layer; W2 exit: box d 15/d 16 shows our PMT with the expected CA descriptors), `lineup.yaml`, unit DB (DAC Define Terminal schema, §3.2), WireLink-verb admin API | — |
| W3 | Message discovery lab | M1 | **Capture rig first (E1, F3-2):** camera OCR + capture card + RS-232 scrape, zero human interaction per experiment iteration, *before* connect experiments begin. Harness: message templates → SDR → box feedback capture; enrollment order per the E-code remedies: **factory-init → unit-creation → connect** (r4 F4-4, now manual-confirmed), then VCM/region/inband-processor/general, then the poll family + disconnect (§2). **System time/date is hunted from day one** as a hypothesized oob-msg message class (F5-8, absorbing F3-9); the capture rig notes any box-clock behavior during connect experiments | W1, W2 |
| W4 | Clear-tier playout | M2 | ffmpeg/tsp playout → UDP/GigE → **SEM V8** (Annex B QAM, in hand); codec pin MPEG-2 MP@ML + MP2/AC-3 per census (F3-6/F5-7, §8); lineup consistency CI, extended to a schedule-DB drift alarm (W4 amendment below); time service | W3 (connect+VCM); SEM fallback = HackRF software QAM (F4-2) |
| W5 | FPK tier via SEM | M3 | SEM ACP module in **FPK mode** (no EMMS required — manual §2); flavor = DES or CSA per box support; verify d 01 auth reason on encrypted tier; software scrambler demoted to optional | W4 |
| W6 | CDL / DLS | M4 | .obj/.dat loader, carousel builder, download control in EMM stream; sacrificial-box protocol with verified flash recovery. **First download enable is gated on D0 validation** (F3-3) | W3, **D0** |
| W7 | Return + polish | M5–M6 | STARFONE modem-bank bridge (mgetty NAS), STARVUE upstream demod, per-terminal **Configure Transmission Parameters** (RPD channel/freq, interactive IDs, impulse credit limits, **power-leveling** attempt tracking), d 12 sign-on state machine + d 13 MAC table as observables, EAS (**post-connect message-family work, reuses the W3 harness** — no separate discovery mechanism, F3-11; zero-weight M6 backlog entry: EAS message class + crawl on the guide channel, F5-9), NC stubs | W4 |
| W8 | **Guide — classic i-Guide** | M6 | See §4.1. App residency census → data-feed discovery → listings synthesizer; fidelity target is the TV Guide Interactive grid | W3, W4; donor path also D0 (F3-3/F4-3) |

Sequencing note: W2/W3 are the critical path; W1 runs in parallel (spec
acquisition is the only external dependency). Nothing waits on eBay.

**D0 — flash extraction capability (named pre-M1 task, F3-3, strengthened
F4-3).** Two validated extraction routes — TL866 II Plus + TSOP adapters
*and* a 68k BDM probe — validated end-to-end on the sacrificial box (full
flash read, checksum, write-back) before M1. D0 now gates **three
workstreams**: W6 recovery, the W8 donor path, and O13 parser RE (the
resident app's parser/decompressor is the fallback oracle if the listings
format is opaque — the d 14 message-ID enum registry points straight at the
message handlers). Risk-register entry: if neither route works, mitigation
is board-level practice on the sacrificial box plus alternate flash packages
per board rev.

### W4 amendment — schedule DB as the playout spine

*Recorded from the strategy session 2026-11-24 (Marshal). This is a plan
amendment. It sets deliverables and criteria for W4; it does not disposition
any gate, and the gate exit criteria in the Milestones document are unchanged
by it.*

The clear-tier playout is driven by **one schedule database as the single
source of truth**. Every consumer reads it; none keeps a private copy:

1. **oob-playout** builds its playlist queues from the schedule.
2. **The W8 listings synthesizer** derives guide listings from the same
   schedule. The lineup consistency CI is inverted into a **drift alarm**:
   it reports when on-air state, the listings and the mux output disagree
   with the schedule, instead of only checking that the config files agree
   with each other.
3. **oob-dac operations.** A lineup change becomes a future-dated DAC
   operation (Define Operations: Channel Map, Multiplex, PPV Window Auth,
   carousel refresh; see the DAC-6000 Operations Reference, Oob design basis).
   It is never a hot edit to live state.

**Entities.** The schedule DB, held in the oob-dac store (SQLite, see the
[oob-dac store](architecture/oob-dac-store.md) decision record), has five
entities:

- **Asset.** A transcoded file: MPEG-2 MP@ML video with MP2 or AC-3 audio,
  per the codec census (§8). Asset metadata feeds the listings.
- **Program.** A scheduled item built from one or more assets.
- **Service.** A mux service with its PID plan and entitlement tier.
- **Schedule.** Rotation, filler and sign-off rules that place programs on
  services.
- **PPV event epoch.** Barker or preview, then sale, event and replay windows.
  Epochs come from this entity instead of the hand-simulated epochs in the
  VOD & PPV Simulation Specification.
  Re-pointing that specification is a follow-up; this amendment does not edit it.

**Management interface.** The WireLink-verb admin API comes first. The Oob
Console views are:

- week grid and traffic view;
- on-air-now per service, derived from mux feedback (continuity counters and
  PTS), with a drift alarm when it disagrees with the schedule;
- asset library with transcode status;
- ops board: the Define Operations lifecycle queue;
- fleet dashboard.

**Non-goals.** No distributed components of any kind, no message broker, no
hot playlist injection into a running queue, and no session-VOD. SQLite stays.

**W4 exit criterion (set by this plan).** W4 is accepted only when clear-tier
playout is driven through the schedule DB. An exercise that runs on
`ops/lineup.yaml` alone does not satisfy it. Any box-visible claim about the
playout still needs a capture under `lab/captures/` (AGENTS.md §2).

## 4.1 W8 — Guide, classic i-Guide (Marshal's requirement)

**Fidelity target (Marshal's call, rev 2 of this section):** whatever guide
application the fleet's boxes actually run, per the d 10 residency census
(O12) — expected to be i-Guide on DCT2000-class hardware, but the census
defines "correct," not a fixed external reference. If boxes carry i-Guide:
the classic TV Guide Interactive grid (channel banner on top, color-coded
genre cells — general = dark blue, kids = light blue, sports = green,
movies = purple — day-skip keys, MSGS indicator), validated side-by-side
against period references. Marshal-supplied references: (1) YouTube
"Motorola DCT2244 Drawing Vector Graphics" (technotite, 2017; supplied
twice) — later i-Guide build on a DCT2244, the *updated* interface but close;
(2) YouTube "TV Listings" (Litestream) — Marshal-labeled "i Guide Demo";
(3) YouTube "Navigator Cable Box Guide Time Warner Cable" (spunker88) —
**cross-ecosystem reference only**: Navigator is the Scientific-Atlanta/Cisco
guide for Explorer boxes (SCTE 55-2 platform), not runnable on Motorola
hardware; logged for comparison since it's the interface Marshal recognized;
(4) YouTube "Time Warner Cable Channel Guide - 6/30/2000" (themilwaukeeboy) —
**Passport** (S-A's earlier guide, dated June 2000 footage), second
cross-ecosystem reference. Marshal's note: he has used all three (i-Guide,
Navigator, Passport); **Navigator and Passport are the ones he remembers
most** — both S-A interfaces — so if the R12 substitute-grid fallback is ever
reached, its look is themed after Navigator/Passport rather than i-Guide;
(5) YouTube "Time Warner Cable Demo" (Matt Selvy / @usaisnojoke) — an actual
**TWC-produced how-to video** per Marshal; canonical reference for period
feature behavior and terminology (interface shown TBC by Marshal), and
template material for a recreation "how to use your service" barker channel
in our own playout (W4 stretch item); (6) YouTube "Browsing the TV guide in
August 2009..." (Nicktoons Network) — **Comcast, August 2009** period guide
footage per Marshal; late-era reference — interface **confirmed as i-Guide**
by Marshal's 13-frame screenshot set (see frame-analysis notes below);
(7) screenshot, **Cincinnati Bell** digital cable — **Passport Echo** grid
dated Mon 12/29 in-frame (2008: December 29 fell on a Monday in 2008, the
HDNet branding predates its 2012 AXS TV rename, and the visible titles are
2004-era) — Marshal-supplied 2026-11-22. **Cross-vendor reference of the
first order: the S-A Passport look running on Motorola DCT-class
hardware**, at a third operator (neither Comcast nor TWC). Proof the
Passport visual language is period-authentic *on our own platform*, not
only on Explorer boxes.
(8) Frame from **"PTCI Training – Pay-Per-View"** — operator-produced
training video on PTCI's own YouTube channel (PTCI = **Panhandle
Telephone Cooperative, Inc.**, Guymon, Oklahoma Panhandle; its "DITV"
digital cable launched January 2002 — ptci.net/history), uploaded
2009-07-31; canonical URL `youtube.com/watch?v=T79N9pSAazw`; chaptered:
1:09
Pay-per-View Settings, 1:27 Review Your Pay-per-View Purchases, 2:46
ordering demo "on the Smaller Digital TV" — Passport (Echo-class) grid
dated **Sun 6/21** in-frame (June 21 fell on a Sunday in 2009, weeks
before the upload) — Marshal-supplied 2026-11-22; **operator attribution
corrected from an earlier "Insight Communications" misread**
(Marshal-confirmed the frame is from this video, 2026-11-22). Second
operator instance of the Passport visual language (hardware platform not
proven by the frame alone), and the first reference showing **PPV in the
grid**: PPV 99 as an ordinary row, in-cell green **$** purchase badge +
REC badge, cream info panel with TV-PG rating and epoch times, maroon PIP
banner "Channel 99 Pay Per View", A/B/C command bar (Time/Theme/Search).
Doubles as PPV UX evidence — see the VOD & PPV Simulation Specification
§2.4.
(9) Frame from the **same PTCI training video as reference (8)** — its
1:09 chapter ("Pay-per-View Settings"), Marshal-confirmed 2026-11-22:
GuideWorks-family **Preferences → Purchasing** settings menu (yellow
context-help panel, purple selector, SEL/Exit command bar) —
Marshal-supplied 2026-11-22. Shows **Locking Status** (`Enabled`),
**Change PIN**, and **PPV Purchases** items; help text: "When locking is
on, PIN entry is required to complete purchases." PIP carries the same
maroon "Pay Per View" banner and event title ("Amazing Journeys") as
reference (8) — same source recording, confirmed. Third GuideWorks-family
instance; hardware platform not proven by the frame. PPV evidence value:
purchase locking is the settings-UI projection of the DAC-documented
box-resident **Event PIN** (DAC §9-21) — see the VOD & PPV Simulation
Specification §2.4/§3.2.
(10) Six frames from the **same PTCI training video as (8)/(9)** —
its 1:27 chapter ("Review Your Pay-per-View Purchases") and 2:46
chapter (ordering demo "on the Smaller Digital TV") — Marshal-supplied
2026-11-22. Purchases review: the **Events Summary** screen — "up to
**7 pending, and 8 past**" purchases, "**Cancel pending events, if
allowed**" (past events cannot be cancelled), a pending row labeled
tersely "Event 6/21 12:30pm 99", SEL = Cancel Purchase, box clock
12:17:30pm — thirteen minutes before the pending event (the same
"Amazing Journeys" 12:30 event as reference (8)'s grid cell; the PIP
runs that title). Ordering demo, dated **Wed 6/17** in-frame (June 17
fell on a Wednesday in 2009; box clock 3:08–3:09am): PPV 99 grid row
("Africa: The Serengeti", **$3.95** in the info panel) → full-screen
event info screen carrying a "**Buy until 3:17 am**" deadline —
mid-event purchase window closing 17 minutes after start; TV-PG;
Program Type: Travel; A Purchase Review / B Buy → red-title
**Purchase Selection** dialog ("Buy Event" / "Buy Event and Set VCR
Timer") → post-purchase tuned banner (PPV 99, title, times, $3.95 in
green), event joined in progress. **No PIN prompt anywhere in the
demo** — this box had purchase locking off; the frames document the
no-locking path. A sixth frame shows PTCI's local channel (**PTCI 2**,
"Wind in Oklahoma", 2:30–4:00am) under the plain non-PPV tune banner —
local-programming flavor consistent with the co-op identity in (8).
Same source recording as (8)/(9), so the GuideWorks-family instance
count is unchanged. PPV evidence value: the full period purchase
sequence and both purchase modes (advance-pending vs impulse
mid-event) — see the VOD & PPV Simulation Specification §2.4/§8.
(11) YouTube "**[VHS] TV Guide Interactive Instructional Video -
(1999)**" (Magnetic Memory Museum, uploaded 2021-05-24, 15:59, no
description), canonical URL `youtube.com/watch?v=7IeSbxrEWI8` —
Marshal-supplied 2026-11-22 **for guide metadata**: a period
Gemstar-era **TV Guide Interactive** instructional tape — the actual
TVGI, corporate-ancestor lineage of i-Guide via the Comcast /
Gemstar-TV Guide GuideWorks venture (background knowledge, not
frame-verified; note the fidelity-target prose above uses "TV Guide
Interactive" loosely for the i-Guide grid — this reference is the
real 1999 artifact, a different UI generation). Partially mined
2026-11-22: six Marshal frames in two batches analyzed in the TVGI
frame-analysis blocks below (Denver lineup, TVGI palette, ad-slot
model; context-dependent action pills, reminder flow, PPV by Title
menu); a transcript or further frames remain a Marshal acquisition
item (screenshot pattern).
Two technical takeaways from source (1): (a) **the DCT2000-series CPU is a
Motorola 68K-series** — vector graphics are drawn by the guide app itself,
slowly enough that the "drawing" effect is visible, so firmware RE on DCT2000
means m68k (BDM debug, m68k disassembly; the .obj download objects are 68k
code modules, matching the §12 load-address/constructor-offset fields);
(b) the visible redraw lag is **period-authentic behavior** — do not "fix"
it in validation; it's part of the classic look.

**Frame analysis (Marshal-supplied i-Guide screenshots, 2026-11-21 — 13
frames, confirming reference (6) is i-Guide):**

- **Grid:** 4 channel rows × 3 half-hour slots; PIP top-right; info panel
top-left (title, channel, time range, rating badge, description); header
"TV Listings" + clock + TV Guide logo; day-skip "Today ◄ ►" plus
jump-to-time icon; channel column = stacked number + callsign.
- **Genre colors (confirmed per Marshal):** sports = **green** (NFL Live,
SportsNation, College Football, Baseball's Golden Age…), kids = **light
blue** (Suite Life of Zack & Cody, SpongeBob SquarePants), movies =
**purple** (Psycho, Live and Let Die), general = dark teal/blue (news,
Paid Programming). Selection highlight = pale yellow-green. Anomaly
logged for O13: NIK "iCarly" cells render dark teal while DIS/SpongeBob
render kids-blue — genre-tag granularity lives in the feed data.
- **Surf/info banner:** blue gradient; title light green (consistent
across genres — seen on sports, kids, general, and movie titles alike;
**genre coloring lives in the grid cells, not the banner title**);
channel number + callsign; clock; time range; rating badge with
content letters (TV-14,L / TV-PG / TV-Y7); HD + Dolby Digital badges;
"Watch in HD" action button; description with quoted episode title, year,
truncation ellipses.
- **Program detail / action screen:** title + time range + day
("11-11:30p Wed") + channel number and callsign ("28 D-KID"), rating
badge floated right; full description below (quoted episode title, year,
parenthetical audience tag e.g. "(Teens)"); bottom action-icon bar —
back, **Record (red dot)**, Reminder (bell), Other-Times (clock),
Watch (eye), Favorite (heart), Lock — with a contextual help line
("Set or cancel a recording"). Record/Reminder imply a DVR-capable
reference box; on non-DVR DCT2000s the record action should be absent —
confirm at census.
- **Mini-guide (browse):** 3-row overlay on video, current time +
now-marker icon, same cell colors as the full grid.
- **Quick Menu:** bottom icon bar — Home, DVR, PPV, HD, Favorites (heart),
Search, Movies, Sports, Kids, Locks — with per-icon help lines
(heart = "My Favorite Channels - Go to a list of your favorite
channels"; Home = "TV Guide Main Menu - Go to the Main Menu." —
confirms Home → Main Menu navigation).
- **TV Guide Main Menu:** header keeps operator logo + clock + TV Guide
logo + PIP; left help panel shows the highlighted item's name in green
with an icon and help line ("Setup" + wrench + "Go to TV Guide Setup");
two-column button grid — **Setup, Messages, Local Weather**, … — paged
with a "More ▲ Choices" scroll indicator. **Messages** = the
operator-message inbox behind the MSGS indicator (headend-sendable
notices — maps to our message/EMM layer, W8 stretch); **Local Weather**
= data-driven applet (synthesized feed, W8 stretch).
- **Favorite Lists Setup:** multiple named favorite lists per box
("Dads", "Abbey", factory-default "List 01"); left help panel shows the
selected list's name + "Contains N channels." in green, with edit/NEW
hints; bottom-left back-arrow and "NEW" buttons. Favorites are box-side
NVRAM state — no headend feed needed, but this is the validation
reference for the Quick Menu Favorites flow.
- **Bottom ad banner:** rotating sponsor banner with a "CLICK HERE" action
(Ice Road Truckers / America's Got Talent / Terminator Salvation frames)
— i-Guide's ad slot; natural home for our own house promos/barker links.
- **Setup submenu (Main Menu → Setup):** ten entries in the 2-col grid —
Guide Setup, Cable Box Setup, Audio Setup, **Guide Colors Setup**, DVR
Setup, Favorite Lists Setup, Screen Saver Setup, Parental Control
Setup, Screen Position Setup, Text Language Setup. Guide Colors Setup
means **genre colors are user-recolorable box-side config** — our
documented palette is the factory default; verify at census before
trusting cell colors as a validation signal.
- **Setup screen:** "Cable Box Setup" — Front LED Display: Current Time;
AC Outlet: Unswitched; **RF Bypass: Off**; Configuration: Select to
display. RF Bypass is also a **DAC General-message parameter** (§2) — the
guide exposes box settings the headend also controls; keep them
consistent in W8.
- **Branding:** operator logo is a skin slot — grid frames show generic
"DIGITAL CABLE"; the setup frame shows **Grande Communications**
(mixed-operator provenance in this frame set; brand-specific elements are
not fidelity targets). Our build skins from the deployment's brand config
— a per-deployment config slot, no preset operator identity (naming rev
2026-11-22).

**Frame analysis — Passport Echo, Cincinnati Bell (Marshal-supplied
screenshot, 2026-11-22; dated Mon 12/29/2008 in-frame):**

- **Layout anatomy (differs from i-Guide):** cream/pale-yellow info panel
  top-left (program title in bold; "Mon 9:00-9:30am" + **HD flag**; quoted
  episode title + description with truncation ellipses; ↻ + CC badges);
  PIP top-right; status bar *under* the PIP (channel 528, date
  "Mon 12/29", clock 9:03am); grid below spanning full width.
- **Monochrome grid — no genre colors:** all cells the same
  lavender/purple regardless of content (movies, series, blocked alike);
  selection = pale yellow-green cell with dark text (same highlight
  concept as i-Guide); selected channel row inverts to dark
  purple/white. Cell titles render in *italics*; programs continuing
  from earlier slots marked "◄◄".
- **Grid geometry:** 5 channel rows × ~2.5 half-hour slots; channel
  column = callsign + number (BRVOD 526 … APHD 530, the HD tier); time
  header with ◄ ► half-hour navigation on the selected slot; scroll
  chevrons on the channel column.
- **A/B/C color-key command bar (absent in i-Guide):** yellow-triangle A
  = "Browse By"; blue-square B = "Date" with its own date badge
  ("MON 12/29"); red-circle C + green return arrow. Maps to the
  Motorola DRC remote's color keys — box-side confirmation this UI runs
  on DCT-class hardware.
- **Operator/lineup facts:** Cincinnati Bell; HD tier in the 520s
  (HDNet 528, HDNMU 529, APHD 530; SCIFI 527, BRVOD 526); "Title
  Blocked" rendered as literal cell text (parental-control state
  in-grid).

**Frame analysis — TV Guide Interactive, 1999 instructional tape
(Marshal-supplied frames, 2026-11-22; reference (11)):**

- **Provenance:** the visible lineup is **Denver broadcast** — KWGN 2,
  KTVD 3, KCNC 4, KMGH 7, KUSA 9 ("NBC affiliate"), plus SHO 5,
  DSC 11 — under a generic "DIGITAL CABLE" watermark, so the deployer
  is not frame-proven. The Denver + 1999 + TVGI combination fits the
  documented corporate facts (SEC filings / trade press): TVGI's
  owner United Video Satellite Group was **TCI-controlled** (TCI was
  Denver-headquartered) until it became TV Guide, Inc. (March 1999)
  and then Gemstar-TV Guide (announced October 1999, closed July
  2000); TVGI reached ~2 million households in 2000, growing ~5,000
  homes/day. The tape's generic branding reads as a vendor demo for
  the anchor market, not operator evidence.
- **Genre color coding, TVGI palette (differs from i-Guide):**
  general/series = **purple** (Popular, Diagnosis Murder, Whose
  Line?, Jesse), sports = **blue** (WWF Wrestling), movies = **red**
  (Last Days of Disco). The corpus now holds three palettes — i-Guide
  (sports green / kids light-blue / movies purple / general dark
  blue), Passport Echo (monochrome), TVGI (this) — proof the palette
  is per-guide-app, not per-platform; the census-first palette
  verification finding (i-Guide frame 11) applies a fortiori.
- **Grid anatomy:** single-time-column list grid — ◄ ► half-hour
  stepper around the slot header ("7:30PM"), 7 channel rows visible,
  channel chip = number + callsign on olive/khaki, continuation "◄"
  on cells that started earlier, selection = green cell outline,
  scroll chevrons above/below. Header: operator watermark left,
  **envelope (messages) icon + clock** center, TV Guide logo right.
- **Browse banner (over video):** TV Guide logo, ◄/► time stepper,
  envelope + clock, channel chip ("9 KUSA") + title cell ("Friends"),
  time range "7-7:30p" — and a **bottom-left ad panel inside the
  banner itself** ("See the Wizard of Oz on TBS").
- **Program info screen:** title (purple), time + channel
  ("7-7:30p 9 KUSA"), a **network-affiliation line** ("NBC
  affiliate"), action pill-row **Last / Watch / Lock ►** (Watch
  highlighted), and a description panel restating the title in green
  with a cast-parenthesized synopsis ("Monica (Courteney Cox) and
  Chandler disagree…") plus a down-chevron continuation marker.
- **Ad model is structural:** the left rail stacks promo panels
  ("You've Got Mail — Now on PPV", "HBO Family — Click for more"),
  the grid adds a bottom ad banner ("Give your family the best - HBO
  Family"), and even the browse banner carries a panel — **including
  PPV promotion via ad slot** ("Now on PPV"). Period guide-era PPV
  discovery rode these slots as well as grid badging; our W8 ad slot
  (i-Guide analysis, "Bottom ad banner") inherits both patterns.
- **Metadata fields evidenced for O13 / listings synthesis:** title,
  time range, channel number + callsign, network affiliation string,
  genre (cell color), cast-parenthesized synopsis with truncation /
  continuation, PPV-promo flag (ad slot), message-envelope indicator,
  cell continuation markers.

**Frame analysis — TVGI batch 2 (three more Marshal-supplied frames,
2026-11-22; reference (11)):**

- **The action pill row is context-dependent:** the batch-1 info
  screen for a *currently airing* program (Friends, 7-7:30p, clock
  ~7pm) offered **Last / Watch / Lock ►**; the batch-2 info screen
  for a *future* program (Frasier, 8-8:30p, header clock 7:23 PM,
  TV-PG) offers **Last / Remind / Lock ►** — Watch becomes Remind
  when the program hasn't started. Same layout otherwise: purple
  title, time + channel, "NBC affiliate" affiliation line,
  cast-parenthesized synopsis with down-chevron continuation.
- **Reminder flow documented:** selecting Remind opens a confirm
  state — "Would you like to set a reminder?" with **Yes / No**
  pills (Yes highlighted); the header center slot switches from the
  clock to the **tuned channel** ("2 KWGN"). Reminders are box-side
  NVRAM state (like Favorite Lists in the i-Guide analysis) — no
  headend feed needed, but this is the validation reference for the
  R12/i-Guide reminder action (bell icon in i-Guide's action bar).
- **Dedicated "PPV by Title" menu:** a full menu screen listing PPV
  movies as red pills (movies = red, consistent with the TVGI
  palette) with **MPAA rating badges right-aligned in-pill** —
  Enemy of the State (R), Patch Adams (PG-13), Rushmore (R),
  She's All That (PG-13) — headed by a **"Watch Previews"** row in
  the magenta/purple accent, with the HBO Family bottom ad banner
  on the same screen. Third period PPV discovery surface (after
  grid badging and ad-slot promotion); "Watch Previews" is period
  evidence for the barker/preview mechanism the VOD/PPV spec's P1
  preview epoch reproduces — folded into that spec §2.4 same-turn.
- **Internal dating corroboration:** the four PPV titles are
  theatrical releases of December 1998 – January 1999; the mid-1999
  PPV window fits the tape's 1999 label exactly.
- **Left-rail promos carry graphical key art,** not just text: the
  rail in these frames shows image panels ("Wasteland — Thursdays"
  with abc branding, "King of Queens" key art). Our W8 ad slot
  inherits an image-capable promo primitive, not text-only.
- **Header center slot alternates:** envelope + clock when idle on
  info screens (7:23 PM), envelope + tuned channel elsewhere
  (2 KWGN in the reminder dialog, 3 KTVD in the PPV menu) — the
  header is a context readout, not a fixed clock.
- **Additional metadata fields evidenced for O13:** TV parental
  guideline badge on the info screen (TV-PG), MPAA rating badge in
  PPV list rows, reminder-set flag (box-side), preview-channel link
  on the PPV menu.

**Architecture reality:** the look is *box-side software* — the install manual
describes an "interactive program application resident on the DCT 2000,"
historically delivered over our own CDL and fed listings over the OOB stream
(initialize/cold-init clears "guide data" from NVRAM; the DAC's Region message
carries the 16-bit EPG region; the DAC maintains a Source-ID mapping "for the
TV Guide EPG"; headend-side there was an EPG inserter adding guide data to the
OOB stream). So W8 = two sub-problems:

1. **App residency (O12).** At census, read each box's d 10 module list. Any
box showing the guide application module is both proof-of-look and a
potential **object donor**: dump the object, serve it back to the rest of
the fleet via our CDL (W6). **Donor objects don't cross platforms (F4-5):**
DCT2000 = 68k, DCT2500 = Broadcom/MIPS — census records Platform/Family/
Model ID (d 01) + module list (d 10) per box, the CDL platform→object
compatibility table is keyed on those IDs, and donor sourcing (O1)
specifies the *target platform*, not just "DCT-class. Boxes that never received the app get it from
us — a legitimate first use of the download infrastructure. Donor supply is
not a constraint: DCT-class boxes are e-waste feedstock (recyclers grind
them for plastic), so additional donor units — including sacrificial boxes
for W6 — are cheap eBay/recycling-channel purchases if the fleet census
comes up short.
2. **Listings data feed (O13).** Synthesize the OOB listings stream in the
format the resident app parses. No public spec or hobbyist RE exists.
Discovery runs in staged order (F3-5, F4-3): **(a) transport
identification first** — feed candidate records and watch whether d 14's
OOB-side or INB-side counters increment (§3.1); OOB first, in-band private
PID second; **(b) format-opacity check** — delta probes of increasing
granularity; if grid response is non-linear to field mutation (period
Gemstar feeds were famously compressed), flip discovery immediately to the
RE path via the D0 flash dump rather than burning box iterations;
**(c) record-format discovery** — box-as-oracle loop (genre color-coding
gives per-field feedback), plus the Gemstar/StarSight patent trove (~90 IPG
patents), plus the DAC manual's Source-ID mapping semantics. Census must
verify the default genre palette before colors are trusted as an oracle
(Guide Colors Setup finding, frame 11). Source content: self-authored
listings from `lineup.yaml` + a schedule editor; no real broadcaster data
needed.

**Fallback (risk R12, re-priced by F3-4):** if the census finds *no* guide
application resident on any fleet box, the target is decided at that point
in order: (a) source a donor object (fleet-external DCTs are cheap e-waste
feedstock; census watches explicitly for **Passport Echo** modules — one
artifact would satisfy residency *and* R12 theming, F5-3) and re-serve via
CDL; (b) **headend-rendered video guide channel** — grid rendered from
`lineup.yaml` server-side and played out as an ordinary MPEG-2 channel
(Prevue/TV Guide Channel style, period-authentic by definition): zero
box-side code, zero CDL dependency, works the day clear-tier playout works;
(c) only if both fail, a **native substitute grid app** — explicitly
degraded and *optional*: authoring a native 68k app against an undocumented
platform API over the very CDL we haven't validated is a project comparable
in risk to all of W6, so it is no longer priced as a "fallback." The theme
for whichever fallback renders is per Marshal's stated preference
**Navigator/Passport** (the interfaces he remembers most) using references
(3) and (4) as the visual baseline — now strengthened by reference (7): the
Passport look is period-authentic *on DCT-class hardware itself* (Passport
Echo at Cincinnati Bell), making it the strongest single theming baseline.
F5-3 adds a pre-G4 task: the research desk commissions a Navigator/Passport
reference pack (frame captures, grid geometry, palette, typography, banner
behavior) to i-Guide rigor, since the R12 target is documented thinner than
the primary. Community angle: the technotite channel
ran a DCT2244
with i-Guide rendering in 2017, and the Litestream channel posted an i-Guide
demo — both are potential contacts for period captures, donor objects, or
feed knowledge (O1/O13).

## 4.2 Platform compatibility — DCH70 & DCT6000 questions (answered 2026-11-22)

Where do DCH70-class boxes fall in the compatibility matrix? Grounding:
DCH70 Installation Manual (© 2007 Motorola, Comcast-hosted PDF) + the
Motorola product brief (Adams Cable Equipment mirror). The DCH70 is the
DCT700-line successor — an all-digital SD receiver, and a **third platform
family** alongside DCT2000 (68k) and DCT2500 (Broadcom/MIPS):

| Layer | Verdict | Basis |
| --- | --- | --- |
| OOB control channel (G0.5/G1) | **Compatible** | SCTE 55-1 OOB transmitter/receiver, agile 70–130 MHz, 2.048 Mbps QPSK. Hunt mode RR is documented as "searching OOB frequencies trying to find an EMM Provider of 0 or 1" — our 0x0000/0x0001 provider-ID constraint — and the manual frequency menu enumerates the same ten carriers in the same scroll order as our hunt table (75.25, 104.20, 72.75, 92.25, 98.25, 107.25, 107.40, 110.25, 116.25, 103.75). Same OOB stream taxonomy on its Unit Address screen (Multicast-16: Net/EMM/SCC_ECM/DWLD/DATA/POLL). |
| Clear-tier playout (G2) | **Compatible on spec** | MPEG-2 MP@ML SD-only video processor; ITU 64/256 QAM + Annex B FEC. Caveat: the manual states the M-Card "is required to view cable television programs" — whether a cardless unit tunes clear QAM is a bench question for E3, not an assumption (boxes are the referee). |
| Encrypted tier (G3) | **Not reachable** | DES/DCII access control runs via inserted M-Card CableCARD (**separable security**), not embedded MediaCipher/FPK. No FPK story on this platform; no card infrastructure in the estate. First separable-security data point if one ever joins. |
| Code download / donor objects (G4) | **Does not cross** | Own platform family (MIPS RISC, but distinct Platform/Family/Model ID in its d 01-equivalent General Status screen). Donor objects never cross platforms (F4-5). |
| Diagnostics (E1) | **Same language, different screen set** | OSD-based d 01–d 22; LEDs-only front panel — E-codes moved into the General Status OSD. Same channel-status state vocabulary; the OOB Status screen (SNR / EMM Data / Carrier Lock) is arguably better instrumentation than the DCT2000's. E1 capture card reads OSD fine. |
| Return path (G5 stretch) | **Present** | On-board real-time RF return; upstream-modem diagnostic (MAC frequency table, RPD power). |
| Guide residency (W8) | **Open census question** | Which guide app this line ran is undocumented here; E3 settles it if one joins. |

Net: a DCH70 would witness G1 and (cardless-clear-QAM permitting) G2, and
stop there. **Not** a G0.5/G1 blocker; surplus = opportunism, zero planning
weight. If one joins the estate it extends the E3 census by one platform
family. "Compatible with Motorola DCT legacy APIs" per the product brief.
(Corpus lead: a DCT700-line user-manual PDF is mirrored at
gditechnology.com — its Appendix B shows the classic d 01–d 21 diagnostic
taxonomy persisted into that line.)

**The DCT6000 series (same question, same day):** the DCT6200 / 6208 /
6412 / 6416 line is the **integrated-security HD generation** (DCT6200
~2002; DCT6412/6416 user guide © 2004 Motorola) — and it lands *better* in
the matrix than the DCH70:

| Layer | Verdict | Basis |
| --- | --- | --- |
| OOB control channel (G0.5/G1) | **Compatible** | Same DCT-platform SCTE 55-1 agile OOB family (2.048 Mbps); ITU 64/256 QAM front end to 860 MHz per the install-manual features list. Provider-ID hunt behavior bench-verified like any box. |
| Clear-tier playout (G2) | **Compatible** | MPEG-2 decode with HD output — our SD MP@ML tier decodes fine. |
| Encrypted tier (G3) | **In scope** | **Integrated-security generation:** embedded MediaCipher DCII/DES, no CableCARD slot — the same architecture as DCT2000/2500 (the DCT2500's own spec line: "DES-based encryption/DCII access control"). The DCH card-host line replaced this architecture in 2007 under the separable-security mandate. The SEM-ACP/FPK tier story applies. |
| HD upside (stretch) | **Noted, zero weight** | MPEG-2 **MP@HL** decode; 1080i/720p/480p/480i output over YPbPr / DVI-D / IEEE 1394. A period-plausible (~2003+) MPEG-2 HD tier is physically reachable with the existing stack (SEM QAM + ffmpeg MPEG-2 HD) — but no H.264 (DCX-era). Not in scope; estate trivia until one joins. |
| Code download / donor objects (G4) | **Does not cross** | Broadcom/MIPS generation, own Platform/Family/Model IDs — census-keyed (F4-5). |
| Diagnostics (E1) | **Same family** | Front-panel 4-digit display + the classic d-screen taxonomy (persisted across the generation — see the DCT700-line TOC lead above). DVR variants (6208 single-tuner, 6412/6416 dual-tuner) add Record-action surfaces for W8. |
| Guide residency (W8) | **Promising** | The 6200/6412 were the canonical Comcast-era i-Guide HD boxes — a census find raises the odds of an i-Guide donor object. |

Net: a DCT6000-family box is a **first-class citizen** of Oob — G1 through
G3 in scope, HD decode as free upside, period-adjacent (2002–2004) rather
than period-core but fully DCII/55-1 era. Same census treatment if one
joins; zero planning weight until then. If fleet-acquisition effort ever
goes hunting, a DCT6200/6412 is worth more to this project than any DCH.

The platform matrix now has four rows: **DCT2000 (68k) · DCT2500
(Broadcom/MIPS) · DCH70 (DCT700-line, card security) · DCT6000
(Broadcom/MIPS HD, integrated security).**

## 5. Repository and harness design

```javascript
mini-headend/            # the Oob workspace
├── crates/
│   ├── oob-dac/         # daemon: unit DB, entitlements, scheduler, admin API (WireLink verbs)
│   ├── oob-mux/         # TS writer, PSI/SI assemblers, PID plan, insertion-bandwidth shaper
│   ├── oob-msg/         # GI message layer: templates + discovered wire formats (the O4 protocol doc, as code)
│   ├── oob-cdl/         # DLS: .obj/.dat loader, carousel builder, EMM download-control
│   ├── oob-starfone/    # modem-bank bridge to mgetty NAS (stretch)
│   ├── oob-playout/     # clear-tier playout → SEM GigE now; software scrambler optional at M3
│   └── oob-guide/       # VueGuide+: listings synthesizer + guide feed + R12 fallback grid (W8)
├── flowgraphs/          # GNU Radio: qpsk_oob_tx.grc, starvue_rx.grc
├── lab/
│   ├── harness/         # message-lab runner: inject → capture box state → log
│   └── captures/        # sniffed upstream, diagnostic scrapes (git-crypt tier)
├── ops/
│   ├── lineup.yaml      # seed/demo lineup: RF lineup ↔ VCM ↔ playout (ADR-003)
│   ├── units.yaml       # upstream box-type catalog + seed; no box instances (ADR-003)
│   └── ci/              # nightly canary: reboot box, assert AUTH, scrape d 02/d 15
└── docs/
    ├── protocol/        # O4: message catalog w/ wire-format status per message
    └── runbook/         # Marshal-style build log + lessons learned
```

Harness contract: every experiment is a commit — message template hash, box
response (OSD state + scraped diagnostics), verdict — so the protocol document
grows from reproducible evidence, not notes.

## 6. Definition-of-done deltas (vs rev 4)

- M0 gains one artifact: `docs/protocol/` seeded from the manual (message
catalog + stream parameters) — mostly written already via this plan.
- M1's exit is **tiered** (F3-8, softened by the install manual): *core* =
box reaches `E 00` + d 02 shows data activity + `PID 1 received`;
*instrumentation tier* = d 14 message-type counters log our connect type —
d 14 is fleet-wide per the install manual, so this tier is no longer
DCT2500-gated — plus the RS-232 console *if* the census yields an
instrumentable unit (census designates the instrument or records its
absence).
- M3 routed through the physical SEM V8's ACP module in FPK mode (see §8)
  instead of a software scrambler + extracted constant. **M3 exit amended
  (F5-4):** EMM authorship gets an R11-style timebox + escalation ladder —
  (1) SEM manual re-read for EMM/ACP interfaces, (2) box-as-oracle against
  SEM-generated streams with the entitlement matrix varied, (3) surplus
  escalation (OM/RADD — one purchase covers both this and R11). Who authors
  addressed EMMs (provider ID 0x0000/0x0001), over which interface, and in
  what format is the discovery question; "the ACP encrypts" alone does not
  pass the gate.
- **M4 exit amended (F5-5):** add a **modified-object round trip** — patch
  one behavior-visible element of a donor object (a string resource), run
  it through oob-cdl, download to the sacrificial box, capture the changed
  behavior on the rig. Replay = plumbing; mutation = understanding. A
  replay-only pass would leave O13 parser RE technically open while the
  gate reads green. M5 unchanged.
- M6 exit criteria now include: **the resident guide grid renders on ≥1 box
per platform family the census found** (F4-5) **and populates from our
synthesized listings feed** (color-coded cells, day-skip working), per W8;
plus **cold-box repopulation** (F3-10): a post-init box repopulates the
grid within N minutes of connect, unassisted — N measured during W8.
- Kill criteria revised (2026-11-22): FPK failure now requires *both* the
  SEM to lack a working ACP module *and* the O3 extraction routes to fail —
  then done = M2 + M4 + M5.
- **Guide feed is an always-on service (F3-10):** the listings carousel runs
  continuously under watchdog with Zabbix alerting (estate pattern); the
  nightly canary asserts grid population, not just AUTH. An M6 demo that
  works because the carousel was just restarted is not M6.
- **Stage gates adopted (2026-11-22):** milestone exits are formally
  governed by the gate criteria in `Project Milestones & Stage Gates —
  mini-headend.md` (entry/exit/evidence/review per gate G0–G6, enabling
  tracks E1–E4, living gate register). Where this section and the gates
  document differ, the gates document wins.

## 7. First two weeks (suggested)

1. O7: acquire SCTE 55-1; record PHY parameters into `docs/protocol/phy.md`.
2. O2: box census (variants, OOB frequencies, firmware revs, cord dates,
   **NVRAM battery state (F4-4)**, Platform/Family/Model ID + d 10 module
   list per box (F4-5) — d-screen set per install manual Appendix B); pick
   the instrumented DCT2500; O9 console probe.
3. SEM bench bring-up: power on; verify ACP on the console (P/N
   507199-001-00 = "Full Edge, ac" — ACP **Yes** per manual Table 2-2, but
   verify via "Show ACP Unit Addresses"); **first-contact archive (F4-2):
   download and commit everything the SEM's HTTP server exposes — SEM-EM
   jar, MIBs, help pages — into git/git-crypt, and record the default-IP /
   factory-reset procedure in the runbook; verify FPK mode and internal
   UDP-port routing are *selectable*, not merely present in menus, and log
   firmware rev + license state**; provision via **SEM-EM** (Java app
   served by the unit itself — JRE 1.2.2–1.5 era, so stage an XP-era VM);
   set Annex B (DCII) QAM mode + HRC/STD plan; loop a tsp-generated MPTS
   in via GigE and verify RF out on the meter.
4. W2 skeleton: `oob-mux` emits PAT/PMT/CAT + nulls at 1.024 Msym/s; loopback
   verify with tsp on the receive side.
5. W1 bench: flowgraph on the **OOB TX supported set** (HackRF One clone,
   ADALM-Pluto range-hacked, LibreSDR; primary chosen at the G0.5 bench by
   the F4-1 level log, Marshal 2026-11-23) at a high hunt-table carrier
   (104.2/110.25 class) into a terminated tap; d 02 on a sacrificial box.
   **F4-1 correction (rationale kept):** the stock Pluto floors at 325 MHz —
   the *entire* hunt table needs the AD9364-identity hack, and its front end
   is unmatched down there. Whichever radio transmits, **meter actual delivered level at
   104.2 MHz through the full F3-7 chain** before committing to it; if
   short, add the small PA the BOM anticipates.
6. W3 first experiment: connect-message hypothesis #1, logged in the harness.
## 8. Tech stack & requirements (decided 2026-11-22, with Marshal)

**Software.** One Rust workspace on the Debian 13 host — crates `oob-dac`
(unit DB sqlite per the DAC Define Terminal schema §3.2, entitlements,
scheduler, WireLink-verb admin API on tokio+axum), `oob-mux` (TS writer,
**DCII SI — not DVB SI** (F4-6/F5-6): DCII reserves PID 8187 for the master
guide table, extends program numbers with virtual channel numbers, and
permits Huffman-compressed descriptor text — confidence-labeled
single-source pending box-as-oracle confirmation via d 14; PID plan,
~1000 Kbps insertion shaper), `oob-msg` (GI message layer;
the crate *is* the O4 protocol doc — every message carries
documented/hypothesized/verified status), `oob-cdl` (DLS loader + carousel),
`oob-playout` (clear tier now), `oob-guide` (**VueGuide+** — listings
synthesizer, guide feed, R12 fallback grid; W8). tsduck (tsp) as early
mux/playout scaffolding; ffmpeg for MPEG-2 MP@ML encodes (F3-6). GNU Radio
3.10 for PHY prototyping; flowgraphs versioned in-repo.

**Playout codec pin (F3-6 + F5-7):** video MPEG-2 MP@ML 480i (4:2:0), CBR
transport per J.83 Annex B 256-QAM (≈38.8 Mbps) with tsp policing;
barker/EPG channels may run lower bitrates. Audio: **MPEG-1 Layer II
(Musicam) primary; AC-3 only where the census confirms decoder support**
(period GI plants ran Musicam with AC-3 on some services — the fleet's
supported set comes from census d-screens, not assumption). **M2 exit gains
a decode smoke test on the oldest fleet box.** Never H.264/AAC — a box
that can't decode masquerades as a tuning/PSI failure.

**Documentation is a build artifact (adopted 2026-11-22, Marshal's
directive):** heavy human-readable docs are built *with* the software from
day one, not after it — mdBook site from `docs/` on every CI run (broken
links fail the build), `cargo doc` for every crate, the OpenAPI spec with
drift checks, and a runbook entry per gate. Gate rule 9 (see the Milestones
& Stage Gates doc §8): no gate passes with stale docs.

**Development toolchain (doc scope, 2026-11-22):** the build/discover/
validate toolchain — rustup-pinned Rust (clippy/fmt/deny/nextest/criterion),
Node LTS + pnpm + Vite for the Console with utoipa→openapi.json drift
checks, Python 3.12 + uv lab glue with pydantic schemas for
`units.yaml`/`lineup.yaml`, inspectrum/tsduck/Wireshark bench analysis, the
E1 capture-rig stack (OpenCV+Tesseract, v4l2, picocom/expect), the RE
workbench (Ghidra m68k, binwalk, capstone, imhex, minipro, 68k BDM,
sigrok), and containerized CI on the lab host (tests, schema validation,
lineup CI, OpenAPI drift, nightly hardware canary post-M1). Full tables
and rationale live in `Project Milestones & Stage Gates — mini-headend.md`
§6; milestone exits are governed by that document's gate criteria (§6 of
this plan defers to it).

**Deployment (Marshal's call):** Docker/Podman for the daemons. Clean fit:
the SEM is network-attached (GigE data + web/SNMP control), and the Pluto
presents iiod over its USB-network gadget — containers talk TCP, no USB
passthrough or device-node juggling. Realtime tolerance: OOB is a
1.024 Msym/s constant-rate stream; the flowgraph (not the daemons) owns
timing. **Deployment notes (F4-7):** host networking (or macvlan) for the
playout container so UDP multicast behaves; explicit multicast group/port
convention in `lineup.yaml`; the managed switch (buy list) sits between
playout host and SEM to keep multicast clean; a container-restart canary
re-asserts the Pluto/iiod connection (the USB-net gadget's link-local
addressing must survive restarts).

**RF architecture (revised by the SEM):**

```javascript
                ┌──────────────┐   UDP/GigE   ┌─────────────┐
playout host ──►│ ffmpeg/tsp   │─────────────►│ SEM V8      │── QAM (Annex B,
(Docker)        └──────────────┘              │ 8×mod, 4×up │   STD/HRC plan)
                                              │ ACP: FPK    │──┐
                ┌──────────────┐  104.2-class │ DES or CSA  │  │
OOB flowgraph ─►│ HackRF One   │── filter ────┴─────────────┘  ▼
                └──────────────┘                    combiner ──► tap ──► plant
  (Pluto = spare/second-carrier TX, F4-1)                     (dummy load
                                                               first, F3-7)
```

- **OOB carrier strategy (F4-1 corrected):** one **primary** carrier from
  the high hunt table (104.2/110.25 class), decided at census; the rest are
  test fixtures (F3-7). **The OOB TX is a supported set** (HackRF One
  clone, ADALM-Pluto range-hacked, LibreSDR); the primary is chosen at the
  G0.5 bench by the F4-1 level log. HackRF One covers the whole ten-carrier
  hunt table (75.25–116.25 MHz) in spec. The stock
  Pluto (AD9363) floors at 325 MHz: every hunt carrier needs the
  AD9364-identity hack, and its front end is unmatched below ~150 MHz, so
  the level budget through the F3-7 chain is metered, not assumed. Level
  target ≈0 dBmV at box input (manual window −10…+5 dBmV).
- **In-band:** playout → UDP/GigE → SEM (up to 8 QAMs, Annex B DCII mode).
  No SDR-QAM or HDMI→QAM appliance needed. **W4 fallback re-documented
  (F4-2):** if the SEM dies or won't provision, M2 fallback = HackRF-based
  software QAM at reduced channel count (one Annex B 256-QAM carries the
  whole clear tier at ~38 Mbps). Deep fallback: DAC-as-controller / SNMP
  provisioning of the SEM if Motorola SEM MIBs are obtainable.
- **Crypto tier (M3):** SEM ACP module in **FPK mode** — the manual states
  FPK "without the need for any other external devices"; DES vs CSA is a
  config parameter. R10 (constant extraction) demoted to optional knowledge;
  kill criteria revised in §6.
- **Frequency housekeeping (F4-8):** the SEM tunes to 861 MHz but the box
  tuner passes 54–860 MHz — keep the lineup ≤860 MHz. STD vs HRC/IRC is our
  free choice via the VCM; the SEM's plan setting, `lineup.yaml`, and the
  VCM must agree — **STD is the default**, recorded here deliberately. Three
  hunt carriers (104.2/107.25/107.40 MHz) sit in the FM broadcast band — a
  closed, terminated plant radiates nothing of consequence, but this is a
  third reason the dummy-load-first discipline and shielded passives in the
  F3-7 chain are policy, not preference.
- **Upstream (M5 stretch):** STARVUE II RX at 5–42 MHz is out of Pluto
  range → buy-list SDR (HackRF One or SDRplay RSP1A). STARFONE phone return
  unchanged (mgetty modem bank).

**Buy list (estate gap):**

1. **OOB TX from the supported set** (HackRF One clone, ADALM-Pluto
   range-hacked, LibreSDR); the primary is chosen at the G0.5 bench by the
   F4-1 level log. The F4-1 radio-role assignment is superseded per
   AGENTS.md §0 (Marshal, 2026-11-23). HackRF One covers the full hunt
   table in spec. A second RX SDR
   (SDRplay RSP1A class) only if STARVUE upstream work wants a dedicated
   receiver.
2. F3-7 RF chain: 2-way combiner, bandpass filter for the chosen OOB
   carrier, variable attenuators, directional tap/coupler, 75 Ω dummy
   loads, RG6 + F-connectors, splitter for census bench.
3. Capture rig (F3-2): USB camera (7-seg OCR via Tesseract), composite/HDMI
   capture card (d-screens), USB RS-232 adapter (DCT2500 console — fleet
   confirmed to include DCT2500s).
4. D0 flash work: TL866 II Plus + TSOP-48 adapter set (and/or 68k BDM
   wiggler).
5. NVRAM batteries (F4-4): soldered lithium cells + holders for fleet
   rehab; per-box battery state logged at census. (A headend that can
   factory-init makes dead-battery boxes fully functional with zero
   soldering — batteries are opportunistic maintenance, not a blocker.)
6. Optional: managed switch for clean GigE multicast between playout host
   and SEM; GPSDO only if carrier drift shows up in d 02. Spare SEM on the
   O1 surplus watch (F4-2: the ACP-less 507199-002 suffices for clear tier;
   another -001 for full redundancy) — pure opportunism, zero planning
   weight.

**Fleet (Marshal's answers):** DCT2000 + DCT2500 confirmed, DVR models
present, exact variants TBD at census (O2). Consequences: the RS-232
instrumentation tier exists (F3-8 fully unlocked — DCT2500 console +
fleet-wide d 14); guide **Record** actions are testable on a real DVR box;
d 06/d 10 census fields per install-manual Appendix B.

**Hardware checks for Marshal:** SEM ACP module presence — **CONFIRMED
2026-11-22 via part number** (seller P/N 507199-001-00 = "Full Edge, ac";
manual Table 2-2: ACP Yes; the sibling 507199-002 is the ACP-less variant) —
bench-verify on power-up. Remaining: whether the GigE SFP cages are
populated (copper 1000BASE-T SFP if not — approved vendor list in manual
Appendix B); QAM/UC module population (listing says 8 QAMs / 4
upconverters); SEM firmware rev; which DVR models are in the fleet (for W8
Record-action validation). Seller specs logged: 3× bidirectional GigE (SFP),
8× ASI (4 in, 4 configurable, ASI↔IP bridge), 64/256-QAM J.83 A/B/C,
tuning 177–861 MHz (verify at bench), ≤1024 input TS, ≤128 encrypted
services, ≤900 Mbps aggregate.
