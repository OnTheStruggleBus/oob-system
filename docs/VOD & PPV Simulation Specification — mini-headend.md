# VOD & PPV Simulation Specification — mini-headend

- **Date:** 2026-11-22
- **Status:** Proposed — awaits Marshal's disposition (gate-text amendments in §7 are proposals, not accepted changes)
- **Owner:** Marshal (@Marshal)
- **Companion to:** Oob Reproduction Plan (W2/W4/W7/W8), Project Milestones & Stage Gates (G3/G5/G6), DAC-6000 Operations Reference

---

## 1. The gap

The corpus covers boot, authorization, channel map, tiers, download, guide,
and the return path — but the **transactional product layer** of the period
service is unplanned: pay-per-view scheduling and authorization, impulse
purchasing, near-video-on-demand, and (at stretch) true session VOD. This
was a core part of what the DAC 6000 *was for* — the manual describes it as
managing "service databases and automated scheduling of services, including
subscriptions, IPPV and call-ahead pay-per-view (CAPPV)" and collecting
"impulse transactions from terminals" for the business system (DAC manual,
§1-4). A headend that cannot sell a movie is missing half the period
experience.

This spec defines the simulation in four tiers, each bound to the earliest
gate that can honestly host it, with box-visible evidence for each. **No new
crates** (AGENTS.md §4 naming freeze): the work lands in `oob-dac`,
`oob-playout`, `oob-msg`, `oob-guide`, and the Console. **No build is
scheduled by this document** — gate amendments are Marshal's to accept.

## 2. What the real system did (evidence base)

All citations are to documents in hand: *Basic DAC 6000 (V3.1) Operation,
Rev 1.2* ("DAC manual"), *DCT2000 Installation Manual* (GI, Dec 2001,
`7370782.pdf`, "install manual"), *Motorola SEM V8 SW 7.1.x* ("SEM manual"),
and the repo's own Operations Reference.

### 2.1 The PPV service and program model (DAC manual §7)

- PPV/IPPV **services** are defined with **Program Provision = Externally
  Supplied**; only then does the Define Programs button become selectable
  (§7-19/7-20). Subscription services have no programs.
- Each service carries a **BSG Service Handle**, range **32768–16777215**,
  coordinated with the business system; if not entered, the DAC auto-issues
  handles per service per business system (§7, Define Digital Service
  procedure). Each program carries a **BSG Program Handle** per business
  system (§7-29/7-30).
- A **digital program epoch** = interstitial time + program time. Inside the
  program: **Preview Time** (window "when the subscriber can view the
  program without purchasing it") and **Purchase Time** (window when the
  subscriber can impulse-purchase; ≤18 h; begins with and usually extends a
  few minutes past preview). Both windows begin at program start.
  Interstitial time is also viewable (§7-19, Figure 7-1).
- Program record fields (Define/Edit Digital Programs, §7-20..7-30): Source
  Name (80 char), Service Provider Name (80 char), Program Name (80 char),
  Interstitial (HH:MM), Start/End Date (MM/DD/YYYY), Start/End Time
  (HH:MM), Preview Time, Purchase Time, **Cost (In Units)** — five-digit
  integer, 1–65535 — plus Antitaping Support / Antitaping Override Enable.

### 2.2 Business-system interface and PPV Configuration Modes (DAC manual §4)

The DAC connects to business systems over WireLink ports (RS-232 and/or
TCP, WireLink 600 & 700 series commands). PPV behavior is governed per
business system by **Table 4-1, PPV Configuration Mode/Zero Handle**:

| Mode | Program download | Call-ahead PPV auth | Purchase upload |
| --- | --- | --- | --- |
| Standard | Each BS loads its own handles | Local handle only | Only if local program handle exists |
| Standard + Zero Handle | Each BS loads its own handles | Local handle only | Local handle if present, else upload with program handle = zero (721/725 only); zero handle unavailable in 600-series commands |
| Master BS | By each BS or by the Master BS | Local handle if it exists, else the Master-defined program handle | Local/Master handle as resolved; no Master program handle defined → purchase deemed **orphan** |
| Master BS + Zero Handle | By each BS or by the Master BS | Local, else Master handle | Local/Master handle, else zero handle (721/725); zero handle unavailable in 600-series |

WireLink purchase/auth command IDs (Operations Reference §, from the DAC
manual's BSI chapter): **694 PPV Authorization Update**; **720 Upload IPPV
Purchases / 721 Return / 722 Receive**; **724/725/726** the single-terminal
variants; **620 Collect Event Purchases / 621 Return / 622 Received**. A
high-priority WireLink port can carry PPV authorization data; a third can
carry IPPV purchase uploads.

### 2.3 Authorization and the IPPV tier (DAC manual §9)

- **All IPPV services share one tier.** "Authorize IPPV" per terminal is a
  single on/off: the terminal is authorized *to purchase*, not per event
  (§9-8/9-9). Per-event authorization is the **PPV Program** path
  ("Authorize Programs" — service/provider/program + start date/time;
  §9-10/9-11) — the call-ahead flow.
- The DAC's auto-generated operations include **PPV Window Auth** (§11-3/11-4)
  — the scheduler emits window authorizations as program epochs arrive;
  this is a first-class row in our scheduler's auto-op table, alongside
  Global Auth Refresh and Insert System Time.
- Purchase behavior is also shaped per terminal by the **General** message
  (purchase authentication flag — plan §2 catalog) and by **Configure
  Transmission Parameters**: **Credit** (impulse credit limit, in units) and
  **Purchases** (impulse purchase limit) (§9, transmission-parameters
  window). "An IPPV purchase decreases the credit limit by the value of the
  program units" (§7, Cost field definition).
- **Clear Orphan Purchases** exists as a per-set-top cleanup action
  (Operations Reference, terminal actions table) — the period remedy for
  purchases uploaded with a zero handle that no business system claims.

### 2.4 Box-side purchase surface (install manual)

- **Front panel (d 01 LED):** alternates between the error code and the
  **purchases count** with an **IPPV status indicator**; the purchases count
  is explicitly documented as the CSR's tool for "uncollected purchases… to
  close out a subscriber's account" (Appendix B, d 01).
- **STARVUE II IPPV status (Table B-23):** LED/OSD tri-state —
  `ENABLED`, `UNSENT − ##` (## = unsent IPPV transactions), `DISABLED`.
- **Remote:** `LOCK/PPV` key opens the Pay-Per-View menu; `OK/SELECT`
  "selects menu options, Pay-Per-View events" (install manual, controls).
- **STARFONE:** the module exists "to send Impulse Pay-Per-View (IPPV)
  information to the DAC 6000 or other controller through the subscriber's
  telephone hookup" (§3); STARFONE diagnostics expose baud-rate,
  transmitter-status, and hang-up-code tables (B-24…B-27).
- Box-side purchase-dialog screens (the actual "buy" OSD flow on a
  non-guide box) are **not** covered by either manual → census/capture
  discovery item (§8), honestly labeled `hypothesized` until captured.
  i-Guide's own PPV ordering flow is app-side and lands with W8 (the guide
  frame analysis already records a **PPV icon in the Quick Menu** — plan
  §4.1).
- **Period ordering pattern (Marshal-supplied frame, 2026-11-22):** a
  Cablevision **PAY PER VIEW** instructional screen (YouTube frame, search
  "cable pay per view menu") documents the pre-guide ordering UX: tune the
  PPV channel "within 10 minutes of showtime" — the operator-set **Purchase
  Time** window (§2.1) made visible — then an orderable-state indicator on
  the box (flashing `E`), a minimal-key confirm (SELECT, then ENTER), and
  confirmation as indicator state change ("When the *E* stops flashing,
  your order is confirmed"). **Provenance caveat:** Cablevision branding on
  an earlier converter generation — the flashing-`E` behavior is *not*
  documented for the DCT2000 (our fleet's documented purchase surfaces are
  the d 01 purchases count and Table B-23 `UNSENT − ##`), so this frame is
  a **UX/barker theming reference, not box-behavior evidence**. Its value:
  (a) it corroborates that purchase windows are box-observable states, and
  (b) it is the visual template for our own PPV **ordering-instruction
  slate**, which belongs in the interstitial window — viewable by
  definition (§2.1) and therefore the natural home for "how to order"
  content on every PPV service we define.
- **Period PPV-in-the-grid pattern (Marshal-supplied frame, 2026-11-22):**
  a Passport (Echo-class) guide frame from **"PTCI Training –
  Pay-Per-View"** (an operator-produced training video on PTCI's own
  channel — PTCI is the **Panhandle Telephone Cooperative**, Guymon,
  Oklahoma Panhandle, whose "DITV" digital cable launched January 2002
  per the co-op's own history page — uploaded 2009-07-31; dated
  **Sun 6/21** in-frame — June 21
  fell on a Sunday in 2009, weeks before the upload; plan §4.1
  reference 8 carries the canonical URL; operator attribution corrected
  from an earlier "Insight Communications" misread, Marshal-confirmed
  2026-11-22) shows the
  guide-era ordering surface: **PPV 99 as an ordinary grid row** with the
  event title in-cell ("Amazing Journeys", 12:30–1:30 pm), a green
  **`$` purchase badge** and a **REC** badge both in-cell and in the cream
  info panel, a TV-PG rating badge, the maroon PIP banner "Channel 99 Pay
  Per View", and the A/B/C command bar (Time/Theme/Search). Read together
  with the Cablevision instructional frame above: the guide era replaced
  barker-slate instructions with **in-grid badging**, but the purchase
  still resolves to the same box-side transaction (§2.3). Consequence for
  W8: our synthesized listings feed must carry **per-program PPV
  attributes** (price/`$` badge, rating, epoch times, REC eligibility) for
  PPV/NVOD rows — not just titles and times. (Second operator instance of
  the Passport visual language after the Cincinnati Bell frame; hardware
  platform not proven by the frame alone.)
- **Period purchase-settings surface (Marshal-supplied frame,
  2026-11-22):** a guide-era **Preferences → Purchasing** menu from the
  same PTCI training video (its 1:09 chapter, "Pay-per-View Settings";
  GuideWorks-family visual language — yellow context-help panel, purple
  selector, SEL/Exit command bar) showing **Locking Status** (`Enabled`),
  **Change PIN**, and **PPV Purchases** items, the help panel spelling
  out: "When locking is on, PIN entry is required to complete purchases."
  The PIP runs the same maroon "Pay Per View" banner and the same event
  title ("Amazing Journeys") as the grid frame above — Marshal confirms
  both frames are the same recording, so this settings surface now has a
  canonical source, an operator, and a date. **Evidence link:** purchase
  locking is not mere guide-app chrome — the DAC documents a
  **box-resident Event PIN** "that control[s] IPPV program purchasing,"
  headend-clearable via Define Terminal → Clear PIN (All / Parental
  Control / Event; DAC §9-21/9-22).
  The frame's locking toggle is therefore the settings-UI projection of a
  documented DAC↔box feature. The **PPV Purchases** review list is now
  frame-documented for guide-era boxes (next bullet); it remains
  undocumented for non-guide boxes, and our purchases ledger (§4) is
  its natural backing store either way. Provenance caveat stands:
  hardware platform not proven by the frame alone.
- **Period purchase flow, end to end (six Marshal-supplied frames,
  2026-11-22):** the same PTCI training video documents the full
  guide-era purchase sequence across two recording sessions. Its 1:27
  chapter ("Review Your Pay-per-View Purchases") shows the **Events
  Summary** screen (Preferences → Purchasing → PPV Purchases): help
  text "Review a list of your recent Pay-Per-View purchases (up to
  **7 pending, and 8 past**). Cancel pending events, **if allowed**" —
  box keeps 15 purchase slots, pending cancelable, past not; the
  pending row is labeled tersely "**Event 6/21 12:30pm 99**" with
  scroll arrows; SEL = **Cancel Purchase**; box clock 12:17:30pm,
  thirteen minutes before the pending event's start; the PIP runs the
  same "Amazing Journeys" title as the grid frame above — the pending
  purchase *is* that 12:30 event. Its 2:46 chapter (ordering demo
  "on the Smaller Digital TV," dated **Wed 6/17** in-frame — June 17
  fell on a Wednesday in 2009; box clock 3:08–3:09am) shows the
  sequence: grid row **PPV 99** ("Africa: The Serengeti") with the
  **$3.95 price in the yellow info panel** → a full-screen **event
  info screen** (Channel 99 chip, title, 3:00–4:00am, $3.95, TV-PG,
  "Program Type: Travel"; A **Purchase Review** / B **Buy**) carrying
  a **"Buy until 3:17 am"** deadline — a mid-event purchase still
  open, closing 17 minutes after start: the operator-set Purchase
  Time window (§2.1) box-visible a second way, complementing the
  Cablevision "within 10 minutes of showtime" slate → a red-title
  **Purchase Selection** dialog — "**Buy Event**" / "**Buy Event and
  Set VCR Timer**" (period VCR-timer integration), A Buy / C Cancel →
  the post-purchase **tuned state**: banner "PPV 99 / Africa: The
  Serengeti / 3:00–4:00am / $3.95" (price in green), the event joined
  in progress. **No PIN prompt appears anywhere in the demo** — this
  box had purchase locking off, so the sequence documents the
  no-locking path; PIN-prompt capture stays an open §8 item
  (checkpoint 5). A sixth frame shows PTCI's own local channel
  (**PTCI 2**, "Wind in Oklahoma," 2:30–4:00am) under the plain
  non-PPV tune banner — local-programming flavor consistent with the
  co-op identity above. Consequences: the period guide-era flow is
  **grid → info screen (price + buy-until deadline) → Buy → confirm
  dialog (VCR-timer option) → tune**, and the video shows both period
  purchase modes — advance purchase (pending, cancelable-if-allowed)
  and impulse mid-event (buy-until window). Our P2 ledger (§4) must
  therefore support a **cancel-before-epoch** path for pending
  purchases, mirroring the box's own Cancel Purchase.
- **Period PPV discovery surfaces, third instance (three Marshal-supplied
  frames from the 1999 TV Guide Interactive instructional tape,
  2026-11-22; plan §4.1 reference (11)):** TVGI documents a **dedicated
  "PPV by Title" menu** — a red-pill title list (**Enemy of the State** R,
  **Patch Adams** PG-13, **Rushmore** R, **She's All That** PG-13 — MPAA
  rating badges right-aligned in-pill) headed by a **"Watch Previews"**
  row, with the HBO Family bottom ad banner on the same screen. The
  period guide-era PPV discovery surfaces are now three: **grid badging**
  (PTCI frames above), **ad-slot promotion** (TVGI's "You've Got Mail —
  Now on PPV" left-rail panel), and a **dedicated PPV menu with a
  previews entry**. "Watch Previews" is period evidence for the
  barker/preview mechanism our P1 preview epoch (§2.1) reproduces —
  the guide deep-links into the operator's preview channel rather than
  streaming anything. The four titles (theatrical releases Dec 1998 –
  Jan 1999) also date the recording to mid-1999, consistent with the
  tape's 1999 label.

### 2.5 True VOD — the NC 1500 / SEM VOD Control path

- In the real ecosystem, interactive VOD required the **NC 1500** Network
  Controller, "an IP gateway between… the application server network and
  OAM&P network… used in digital cable networks that support interactive
  communications such as video on demand (VOD)" (DAC manual §1-5/1-6), with
  the STARVUE II returning upstream data *to the NC*; box-side, the d 12
  interactive sign-on state machine (`MAC CONNECT → … → RUNNING`) is the
  observable.
- The **SEM V8 is VOD-native at the edge**: its SEM-EM exposes a full **VOD
  Control** family — Manual Routing, UDP Mapping, Standardized UDP Mapping,
  8-Channel UDP Mapping, Ancillary PID Routing, and ICE EMM Configuration —
  for routing per-session SPTS from a VOD server into QAMs (SEM manual
  §4, VOD Control). The session *control plane* (box ↔ NC ↔ DAC) is the
  undocumented half.
- Consequence: on our one-way/fone-way fleet, period-authentic "VOD" is
  **NVOD** (§3.3); true session VOD is a registered stretch (§3.4), not a
  plan item.

## 3. Simulation design — four tiers

### 3.1 Tier P1 — Scheduled PPV / call-ahead (CAPPV) — *no return path needed*

- `oob-dac` gains a **program database** (sqlite, derived state per F5-12)
  implementing the §2.1 record: service (Program Provision = Externally
  Supplied, BSG service handle auto-issued ≥32768), programs (epoch fields,
  cost-in-units, antitaping flags), and per-business-system program handles.
  `lineup.yaml` references PPV services by name; the VCM maps them like any
  digital service; lineup-consistency CI covers them.
- The scheduler implements **PPV Window Auth** as an auto-generated
  operation: at each program boundary, emit window authorizations per the
  epoch (preview/purchase windows), visible in the ops lifecycle
  (New → … → Completed) like every Define Operations row.
- **Call-ahead flow:** the Console (acting as business system, §6)
  authorizes a terminal for a program in advance — the period CSR flow —
  and the authorization rides the normal entitlement/EMM machinery.
- **Preview semantics:** during interstitial + preview windows the service
  is viewable without purchase; after the preview window an unauthorized
  box gets the standard not-authorized state. This is the core
  box-as-oracle checkpoint (§8) — the *timing* of the transition is the
  experiment, against a real epoch clock.
- One-way boxes fully exercise P1: exactly how period one-way plants sold
  PPV (call ahead; Interconnect spec §, same observation).

### 3.2 Tier P2 — Impulse PPV (IPPV) — *needs the W7 return path*

- Per-terminal **Credit** (units) and **Purchases** (count limit) fields in
  the unit DB, set via the Configure Transmission Parameters analog;
  **Authorize IPPV** = the single shared impulse tier.
- Box-side impulse purchase during the Purchase Time window → purchase
  record stored box-side (unsent count grows — Table B-23 `UNSENT − ##` is
  our oracle) → collection via **STARFONE** (modem-bank bridge) or
  **STARVUE II** (upstream burst) → `oob-dac` **purchase ledger** → credit
  decremented by program units; purchase denied box-side at credit
  exhaustion (period degradation behavior — preserved, not papered over).
- **Orphan semantics implemented faithfully:** purchases referencing an
  unknown program handle upload with handle zero (721/725 semantics); the
  ledger marks them orphan; **Clear Orphan Purchases** is the Console op.
- **Purchase Poll** (already in the W3/W7 poll family) closes the loop:
  headend-initiated collection on a schedule, per Define Operations.
- Terminal Type (One-Way / Fone-Way / Two-Way, census field) determines
  which collection path each box exercises; one-way boxes accumulate unsent
  transactions forever, exactly like the period — and their purchases count
  stays visible on the front panel for the "CSR close-out" journey.
- **Purchase locking (Event PIN):** the box-resident Event PIN (DAC
  §9-21/9-22) gates impulse-purchase completion when locking is enabled —
  the period settings-UX per the Marshal purchasing frame (§2.4). Fidelity
  stance: the PIN itself is set and verified **in the box** — the real DAC
  stored none, it could only *clear* PINs — so `oob-dac` models exactly
  one PIN operation, **Clear PIN (Event / Parental Control / All)** as a
  Console per-terminal op (§6). PIN-entry and wrong-PIN denial UX stay
  box-side behavior to capture (§8), never simulated headend-side.

### 3.3 Tier P3 — NVOD (near video on demand) — *playout + P1 only*

- The period-honest "VOD" for one-way hardware: the same self-authored
  feature (our own loops; no real content, ever) carried on **N staggered
  PPV services** at a fixed start-time offset, so "start the movie now"
  means "tune the instance starting next." `oob-playout` generates the
  stagger set from one source; each instance is a Tier-P1 service with its
  own program epochs and window auth.
- Lineup cost is explicit and small: a 4-instance NVOD block = 4 services in
  the mux plan; the SEM has QAM capacity to spare (8 QAMs, ≤128 encrypted
  services).
- Guide integration (W8): NVOD instances appear as ordinary listings rows
  ("movie starts every 30 min") — period-accurate, since i-Guide listed
  NVOD/PPV channels in the grid. The PTCI Passport frame (§2.4)
  corroborates the pattern and adds the in-cell `$`/REC badging our
  listings feed must carry for PPV/NVOD rows.

### 3.4 Tier V4 — True session VOD — *registered stretch S4, no gate until scoped*

Registered the way S3 (analog tier) is: **name and boundary only, zero
planning weight, no gate defined until scoped, post-G6.** Scope sketch for
the register: NC 1500 role emulation (IP gateway + session control),
d 12 sign-on to `RUNNING` on a two-way unit, session setup discovery via
the box-as-oracle loop, SEM **VOD Control** UDP mapping for per-session
SPTS, ICE EMM configuration. Entry criteria when scoped: G6 + G5 passed,
a two-way census unit confirmed, and the S4 section promoted from this
paragraph into a full spec revision. New open item **O14** tracks the
control-plane protocol question; **O15** tracks acquisition of the full
WireLink command dialect reference (restricted document — acquisition item
for Marshal, upload pattern per doctrine; no paywall bypass).

## 4. Data model and config (sketch)

- `oob-dac` sqlite: `services` (already implied by entitlements work) gains
  `program_provision`; new `programs` (epoch fields, cost_units,
  antitaping), `program_handles` (BS × program → handle, zero-handle
  semantics), `purchases` ledger (terminal, program/handle, epoch,
  units, state: unsent/collected/orphan/cleared), terminal `credit_units`
  + `purchase_limit` fields.
- `units.yaml`: per-box `terminal_type` (census) + optional credit/limit
  overrides; schema-validated in CI like today.
- `lineup.yaml`: `ppv:` service blocks (service name, source, provision =
  externally-supplied, optional explicit BSG handle) and an `nvod:` block
  (source, instances, offset) expanded by playout; VCM unchanged in shape.
- Console: program schedule editor (the Define Digital Programs window,
  web-native), purchase ledger view (already specced), orphan-purchase
  queue, per-terminal credit display, **Clear PIN (Event / Parental
  Control / All)** per-terminal op (DAC §9-21).

## 5. oob-msg catalog additions

New rows in the O4 protocol catalog (all start `documented` — semantics
manual-sourced, wire formats undiscovered until W3 hunts them):

| Message family | Semantics source | Notes |
| --- | --- | --- |
| PPV Window Auth (auto-op) | DAC manual §11 | emitted per program epoch; the P1 workhorse |
| Purchase/auth flag in General message | plan §2 catalog | gates box-side purchase UI |
| Purchase Poll response (purchase records) | install manual B-23/B-24…B-27 + DAC §4 | rides the W7 poll family; 720/721/722 semantics on the BSI side |
| (V4 only) session setup family | — | `hypothesized` at best until S4 is scoped |

## 6. Console as the business system

The DAC↔BSI split is part of the period system, so we reproduce **both
ends**: `oob-dac` hosts a **WireLink 600/700-series server endpoint** (the
BSI), and the Oob Console — or a test harness — plays the business system
over it: downloading PPV programs with handles, sending call-ahead
authorizations (694 semantics), receiving purchase uploads (720/724), and
exercising the PPV Configuration Mode table (§2.2) as a config slot
(default: Standard + Zero Handle — the mode that exercises orphans). This
keeps the Console's purchase ledger and the WireLink dialect testable
independently of any box, and gives the nightly canary a software-only
transaction path (§8).

## 7. Gate and milestone mapping — PROPOSED (Marshal dispositions)

| Tier | Proposed host gate | Proposed exit-text addition |
| --- | --- | --- |
| P1 CAPPV | **G3** checklist item | One PPV service + one program epoch defined; PPV Window Auth emitted on schedule; box viewable during preview, denied after (capture); call-ahead authorization visible in ledger |
| P2 IPPV | **G5** exit-text amendment | …plus: an impulse purchase round-trips (box → return path → ledger), credit decremented, and the orphan path (zero-handle upload → Clear Orphan Purchases) is exercised |
| P3 NVOD | **G6** checklist item | One NVOD block (≥2 instances) in the lineup, listed in the guide grid, each instance epoch-correct |
| V4 true VOD | **none** — stretch **S4**, registered like S3; G6 review re-scores the register (S1/S2/S3/S4) | — |

Nothing here moves W1–W8 scope or the G0.5/G1 critical path; P1's data
model can be built any time after G1 entry without disturbing it, and the
BSI endpoint is a software-only track with no box dependency.

## 8. Evidence plan (box-as-oracle checkpoints)

Every claim below lands in `lab/captures/` via the E1 rig; message-family
status in `oob-msg` is promoted only on capture (AGENTS.md §2).

1. **Preview window:** capture box decoding a PPV service inside preview,
   and the not-authorized state after the window closes — epoch-edge timing
   measured, not assumed.
2. **Call-ahead auth:** authorize → box viewable post-preview → d-screen +
   OSD captures.
3. **Impulse purchase:** box purchase flow (screens TBD at census —
   `hypothesized` until captured; period patterns to compare against:
   orderable-state indication → minimal-key confirm → visible confirmation,
   per the Cablevision frame §2.4; and the guide-era sequence grid →
   info screen (price + buy-until deadline) → Buy → confirm dialog →
   tuned, per the PTCI ordering demo §2.4) → front-panel **purchases count**
   increments (d 01 LED alternation) → `UNSENT − ##` on the STARVUE II IPPV
   status screen → post-collection `ENABLED` → ledger row + credit delta.
4. **Denial:** credit exhausted → purchase refused box-side (capture the
   box's own denial UX — period degradation, preserved).
5. **Purchase locking:** with locking enabled, an impulse attempt →
   PIN-entry prompt (capture); wrong-PIN denial (capture); Console
   **Clear PIN (Event)** → box purchases complete without the prompt
   (capture). PIN storage stays box-resident; the headend never holds it
   (DAC §9-21 fidelity, §3.2). The complementary no-locking path —
   locking disabled, no prompt, purchase completes directly — is
   already frame-documented by the PTCI ordering demo (§2.4).
6. **Orphan drill:** zero-handle upload → ledger orphan → Clear Orphan
   Purchases → ledger clean.
7. **Canary extension (proposal):** nightly asserts (a) current-epoch
   window auth emitted, (b) a software-only WireLink transaction (Console
   as BS) lands in the ledger, (c) on hardware, purchases count matches the
   ledger's unsent total for the canary box.

## 9. Explicit non-goals

- No real content: PPV/NVOD programming is self-authored loops and authored
  titles only (ground rule 5).
- No true-VOD build before S4 is scoped and gated (post-G6).
- No Macrovision/antitaping hardware: the flags are modeled in the DB for
  fidelity of the *management* surface; no analog protection is generated.
- No changes to W1–W8 scope, gate G0.5's agenda, or the G1 critical path.
- No new crates, no new component names (AGENTS.md §4).

## 10. Open items

- **O14** — NC 1500 / session-VOD control plane (S4 research; far-future).
- **O15** — Full WireLink 600/700 dialect reference: acquisition item
  (restricted document; Marshal upload pattern). Until then the BSI
  endpoint implements the command IDs and semantics documented in the DAC
  manual and Operations Reference, marked `documented`-semantics /
  `hypothesized`-framing.
