# Programming & Ad Management Specification — mini-headend

*Status: proposed, 2026-11-24 (strategy session, Marshal). This specification
defines the programming schedule and the ad-management model for the platform.
It is the operator-side companion to the W4 schedule-DB amendment in the
Reproduction Plan and to the Encoding & Transport Specification. It does not
disposition any gate. Gate placement is in §12 and is for Marshal to decide.*

**Trust labels used here**, as in the Encoding & Transport Specification:
**documented (public standard)**, **documented (repo)**, **lab default** (a
choice this project makes), and **hypothesized** (a box behavior not yet
confirmed by a capture). Nothing here is **verified**.

## 1. Scope and non-goals

**In scope.**

- **Programming:** which program plays on which service at which time. The
  channel lineup, the service grid, dayparts, sign-on and sign-off,
  rotation, filler, and the program metadata the listings are built from.
- **Ad management:** avails (the break opportunities inside a program), the
  spot inventory, campaigns and their flights, the trafficking schedule that
  places spots into avails, the as-run record of what actually aired, and the
  affidavit generated from it.
- **Splice signaling:** the SCTE 35 cues that mark each avail, generated from
  the schedule and placed on GOP boundaries (the Encoding & Transport
  Specification §5).

**Non-goals.**

- **No addressable advertising.** DCT2000-class boxes are expected to receive
  no per-subscriber ad message. Every ad is spliced into the common program
  stream at the headend, and every subscriber on a service sees the same spot.
  **Hypothesized:** the repo's manual digests describe no per-subscriber ad
  path. A box capture would be needed to say more.
- **No real advertisers, real brands or real spots.** Campaigns, advertisers,
  spot copy and rating data are fictitious and self-authored (ADR-001 content
  rule; AGENTS.md §11).
- **No national network feed or live ad sales** to outside parties.
- **No distributed components, no message broker, no hot playlist injection**
  into a running queue (W4 amendment, non-goals).

## 2. Configuration model — everything is UI- and database-driven

This section applies to the whole platform, not only to this specification.

- **Every operator setting is a database row** in oob-dac's store (SQLite).
  Programming, avails, campaigns, flights, cue rules, encode profiles,
  transcode templates, service and PID plan, caption policy, listings
  metadata, brand skin, traffic-class budgets and entitlements are all rows.
  **Documented (repo):** ADR-003 Option A, the runtime-configurable operator
  system, is accepted and is the basis for this rule.
- **Edited through the Oob Console or the WireLink-verb admin API**, never by
  hand-editing a YAML file. An operator who needs a value changes it in a
  form, the same way for every entity.
- **Validated server-side on every save** (schema, lineup ↔ VCM ↔ playout ↔
  schedule consistency, and the ADR-003 §5 guardrails). Invalid input is
  refused with a reason. The Console is never the only check.
- **Revisioned with rollback, and journaled.** Every change to a schedule,
  campaign or encode profile is a revision that can be rolled back. Every
  change is written to the journal with the operator and the time.
- **Future-dated application.** A change that touches live services is
  applied as a future-dated DAC operation (Define Operations), never as a hot
  edit to live state (W4 amendment item 3).
- **`ops/*.yaml` are seed and example data only.** `ops/lineup.yaml` and
  `ops/units.yaml` load into the store on first install, and an operator then
  edits the store, not the file (ADR-003 §6). Packs for community use go
  through the import and export format (ADR-003 §4). YAML export of runtime
  state is a backup and diff aid. Nothing requires a YAML edit to operate the
  platform.
- **Lab defaults ship as seed rows**, labelled as lab defaults, so the
  operator can see what the project chose and change it in the UI.

The schedule DB is the spine (Reproduction Plan, W4 amendment). This
specification names the rows that sit on it.

## 3. Programming model

The chain of entities, from the lineup down to the content:

1. **Service** — a mux service with its PID plan, entitlement tier and
   lineup position (the W4 amendment's **Service** entity).
2. **Grid slot** — a time window on one service, aligned to the half hour
   by default (lab default, the common period grid).
3. **Program instance** — a **Program** placed in a grid slot. A program is
   built from one or more **Assets** (the Encoding & Transport Specification,
   §9).
4. **Schedule rule** — rotation, filler and sign-off rules that place programs
   on services over time (the W4 amendment's **Schedule** entity).

**Dayparts and sign-on.** Each service has a sign-on time and a sign-off time.
Outside those hours the service carries a **sign-off loop** or a barker
channel. The loop is self-authored and labelled as test content. **Lab
default:** the sign-off loop plays a self-authored slate with the station
identity and the next sign-on time. Real-world sign-off content is not
reproduced.

**Filler.** When a grid slot has no program, filler fills it. Filler is a
configurable rule (a loop, a slate, or the sign-off loop). Filler is never
silent: a missing program is visible in the on-air monitor (§7).

**Program metadata.** Each program carries the fields that the listings and
the guide need: title, episode, duration, a rating from a fictitious
self-authored scheme, genre, closed-caption flag, audio language, and the
program's ad-avail plan (§4.3). This is the metadata the W8 listings
synthesizer reads (Reproduction Plan §4.1).

**Schedule changes.** A schedule change is a revision (§2). Changes that
affect the next grid slot or later are future-dated DAC operations
(Channel Map, Multiplex, PPV Authorization Update, carousel refresh, per the
DAC-6000 Operations Reference, section 5). A change inside the current slot
is refused, not applied in place.

**Emergency alert.** The EAS path is the W7 backlog entry and is not in the
M2 scope. Any emulated alert is labelled as test traffic (ADR-001 content
rule).

## 4. Ad management model

### 4.1 Avails

An **avail** is a break opportunity inside a program. Each avail has a start
time, a duration, a position (pre-roll, mid-roll, post-roll), a class
(local, national placeholder, public-service placeholder, promotional), and
a status (open, filled, filled-by-default, black).

**Lab default avail plan.** A half-hour grid slot carries two or three breaks
totalling about 4 to 6 minutes of avail time. **Hypothesized as the period
practice; the lab uses it as a default.** The default is a row the operator
edits, not a constant in code.

### 4.2 Spot inventory

A **spot** is a self-authored commercial placeholder. Spot length is one of
15, 30, 60 or 120 seconds (lab default; the period lengths). Each spot is an
Asset (Encoding & Transport Specification, §9) with its own transcode record,
so a spot is encoded to the same profile as program content and the splice
rules of §5 of that specification apply to it.

**Spot copy is fictitious.** Advertiser names, brands and slogans are invented
for the lab. A spot must not use a real trademark or a real advertiser's
material (AGENTS.md §11: no third-party content).

### 4.3 Campaigns, flights and rotation

A **campaign** is a fictitious advertiser's order:

- advertiser (fictitious), campaign name, and the contracted spot count;
- one or more **flights**, each with a start date, an end date, and a
  daypart and service restriction;
- a **rotation weight** and a **frequency cap** per hour (lab default: a cap
  per service per hour, so one spot does not dominate a break);
- a **make-good** rule: when as-run shows fewer airings than contracted, the
  shortfall is queued as make-good airings in the next flight (period
  practice; lab default).

### 4.4 Trafficking

**Trafficking** places spots into avails. The output is the **spot schedule**:
for each avail, which spot airs, on which service, at which time. The
trafficking run is a revision (§2), so it can be reviewed and rolled back
before it takes effect.

Local avails are filled from the local spot inventory of the service's ad
zone. National placeholder avails are filled from the national spot inventory.
If no spot is scheduled for an avail, the avail is **filled-by-default** with
a promotional or public-service placeholder, or **black** if none is set. Both
outcomes are visible in the on-air monitor (§7).

### 4.5 Splice signaling

Each avail becomes an SCTE 35 cue. The cue is generated from the schedule DB
when the schedule is applied, not written by hand:

- **Documented (public standard, ANSI/SCTE 35):** `splice_insert` or
  `time_signal` with a `segmentation_descriptor` marking the break start and
  end. The segmentation type codes used are the public-standard values; the
  exact codes are to be checked against the text when it is acquired (see
  the Encoding & Transport Specification §12).
- The cue is placed on the GOP boundary at the splice in-point (Encoding &
  Transport Specification §5).
- A cue that arrives late, or a splice that cannot be placed on a closed-GOP
  boundary, is a failed splice. It is counted and shown in the on-air monitor.
- **Box side:** the box's reaction to the cue PID is **hypothesized** (the
  Encoding & Transport Specification §7). The splice is verified on the lab
  side, and a box-visible claim needs a capture.

### 4.6 As-run and affidavit

The **as-run log** records what actually aired: which spot, on which service,
at what time, with the duration measured from the mux output (PTS of the
splice, from the on-air monitor). The as-run log is the source of truth for
delivery.

The **affidavit** is generated from the as-run log for a campaign and a
flight. It lists each airing with its time and service, and it lists the
make-good shortfall. The affidavit is a fictitious report for the lab's
own billing exercise (the Billing & Subscriber Management Specification
covers subscribers, not advertisers, so no advertiser ledger is defined there).

## 5. Interfaces

- **Console views** (proposed; the Management Console Specification does not
  list them yet, per the W4 amendment):
  - **Schedule grid and traffic view** — the week grid per service, with the
    avail plan and the spot schedule overlaid.
  - **Campaign and flight editor** — the traffic desk: campaigns, flights,
    rotation, caps, make-goods.
  - **Spot library** — spots as Assets, with transcode status and length.
  - **Avail calendar** — open, filled, filled-by-default and black avails.
  - **As-run viewer and affidavit export.**
  - **Ops board** — the Define Operations lifecycle queue for future-dated
    changes.
- **Admin API.** Every view above is backed by WireLink-verb endpoints in
  oob-dac. The Console adds no logic the API does not expose (AGENTS.md §9,
  API-first).
- **Import and export.** Programming and campaign data can be exported as a
  backup, and imported into a fresh install. Exports never include box
  identifiers or keys (ADR-003 §4).

## 6. Failure and degradation

These follow the period behavior the platform reproduces, and none is hidden:

- **Late or missing cue.** The splice is skipped or placed late, and the
  on-air monitor shows the drift.
- **Avail with no spot.** The avail is filled by default or goes black. The
  black is counted, not papered over.
- **Mux or playout outage.** The mux continues from its last applied
  revision. A changed schedule waits for the next future-dated operation. No
  hot edit.
- **As-run mismatch.** The as-run log and the on-air monitor disagree. The
  mismatch raises a drift alarm and is not silently corrected.

## 7. On-air monitor

The **on-air monitor** shows, for each service, what is on air now: the
program, the avail state, and the splice state. It is derived from the mux
feedback (continuity counters and PTS, per the W4 amendment) and compared with
the schedule. A disagreement raises the drift alarm described in §6.

## 8. Verification (lab-side)

These are checks on the lab, not on a box. A box-visible claim needs a capture
(AGENTS.md §2).

| Check | Pass condition |
| --- | --- |
| Schedule applied | The on-air state matches the schedule for every service |
| Avail count | Each grid slot has the planned number of avails |
| Cue placement | Every cue sits on a closed-GOP boundary (Encoding & Transport §5) |
| Spot length | Each scheduled spot's length equals its asset's measured length |
| As-run agreement | The as-run log matches the mux output for a test flight |
| Configuration round trip | A value set in the Console reads back identically through the API and after an export and import |

## 9. Gate placement (PROPOSED — Marshal dispositions)

- **Programming and the schedule DB** — the W4 amendment (M2).
- **Splice cues on the avails** — proposed for W4, because a clear channel with
  breaks needs them.
- **Campaigns, trafficking, as-run and affidavit** — proposed as post-M2
  deliverables with no gate exit of their own. Marshal decides whether they
  need a gate.
- **Configuration through the Console** — the Console phases (C0 is a G1 exit
  requirement; the rest per the Console spec §5). This specification adds no
  new gate requirement.

No gate is passed, claimed or dispositioned here.

## 10. Open items

- **Lab defaults to confirm:** the avail plan (§4.1), the spot lengths (§4.2),
  the frequency cap (§4.3) and the sign-off loop (§3). Each is a seed row and
  each can be changed in the Console.
- **Standards text:** ANSI/SCTE 35 segmentation codes (§4.5), acquired as an
  open item like the others in the Encoding & Transport Specification §12.
- **Period practice:** the avail-time figures in §4.1 are **hypothesized** as
  the period norm until a source is cited.

## 11. Cross-references

- Reproduction Plan, W4 amendment (the schedule DB, the Asset, Program,
  Service, Schedule and PPV-epoch entities; the Console views in the W4
  amendment).
- Encoding & Transport Specification (the profile, GOP and splice rules, the
  asset pipeline).
- DAC-6000 Operations Reference, section 5 (Define Operations: Change Set-Top
  Channel Map, PPV Authorization Update) and section 10.
- VOD & PPV Simulation Specification (PPV epochs; the W4 amendment supersedes
  its hand-simulated epochs, as a planned follow-up).
- ADR-003 (runtime configuration in the store; YAML is seed only; community
  packs).
- Management Console Specification (API-first; the phases).
- Billing & Subscriber Management Specification (subscribers only; no
  advertiser ledger).

