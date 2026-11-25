# Management Console Specification — Oob Console

**The operator front-end for the whole estate — settings, templates, and day-to-day operations**

*Renamed 2026-11-22 with the naming adoption: the software is **Oob**, so
this is the **Oob Console** (formerly "mini-headend Console").*

- **Date:** 2026-11-22
- **Status:** Proposed (C0 design) — core tooling, not a stretch goal; console grows with the milestone plan
- **Parent docs:** `mini-headend Project Overview`, `Oob Reproduction Plan` (§5 repo, §8 stack), `Remote Service Delivery Specification`

---

## 1. What this is

Every piece of the original estate had an operator surface: the DAC 6000
had its serial operator terminals and the WireLink command interface; the
SEM has SEM-EM. Oob Console is ours — the single front-end
through which every setting, message template, lineup, object, campaign,
and hub is viewed and changed.

Three rules shape it:

1. **API-first, always.** The Console is a *client* of oob-dac's
   WireLink-verb admin API (tokio+axum, plan §8). It has no privileged
   back door and no state of its own. Anything the UI can do, `curl`, a
   script, or the nightly canary can do through the same verbs — the API
   is the product; the Console is its best showcase.
2. **Fully configurable in-system (ADR-003).** Every setting an operator
   needs — boxes, lineup, PID plan, class budgets, entitlements,
   campaigns, listings, message templates, the brand skin slot — lives
   in oob-dac's store and changes at runtime through the
   API. oob-dac validates every save, keeps a revision history with
   rollback, and journals every change (who, when, before/after). The
   running system needs no git and opens no PRs; git governs *developing*
   Oob, not operating it. Box-type profiles are the exception: they come
   from the upstream catalog, and a new box type is added only by
   upstream PR or a fork (Marshal, QA-6); the Console selects and shows
   them but cannot create them.
3. **The box is still the referee.** Wherever the Console shows a
   setting, it also shows — or links to — the box-visible evidence that
   the setting took: the d-screen scrape, the OCR'd front panel, the
   d 14 counters. Settings without telemetry are wishes.

### Aesthetic

Default skin is a nod to the period ops terminal: dark background,
amber/green phosphor accents, monospace for anything hex — but the
interactions are modern (tables, diff views, live charts). A plain light
theme exists for daylight bench use; a wallboard mode strips chrome for
an ops-room display. The skin is a theme, never an excuse for gimmicks
that slow the work.

## 2. Architecture

```javascript
┌────────────────────────────────────────────────────────────┐
│ Browser (bench laptop / tablet / wallboard)                │
│   SPA — Vite + React + TypeScript, "phosphor" theme        │
└──────────────┬───────────────────────────────┬─────────────┘
        REST/JSON│                    WebSocket │ (live telemetry)
               ▼ ▼
┌──────────────────────────────────────────┐
│ oob-dac admin API (axum)                 │  ← the ONLY backend
│  • WireLink verbs → REST resources       │
│  • WS channels: telemetry, experiments,  │
│    logs, alarms                          │
│  • OpenAPI (utoipa) — self-documenting   │
│  • config store: validate on save,       │
│    revisions + rollback, journal         │
└───┬──────────────────────┬───────────────┘
    ▼                      ▼
 sqlite (one file,      msg/oob-mux/cdl/
 F6-5): unit DB,        playout engines
 config, templates,     + capture rig adapters
 journal (box types:
 upstream catalog)
```

- **No Console-side database.** State lives in oob-dac's sqlite file
  (unit DB, configuration, templates, journal — ADR-003; box-type
  profiles are read from the upstream catalog) and
  the engines. Restarting the Console loses nothing.
- **AuthN/Z:** lab-scale. Owner account (full) + read-only "museum mode"
  guest (for demos and visitors). Token-based; the whole thing binds to
  the lab LAN / WireGuard only — never internet-exposed, matching the
  closed-plant doctrine.
- **Deployment:** one more container. The SPA is static, served by axum
  (or nginx); build in CI, version with the workspace.
- **Stack rationale:** React+TS for ecosystem and component depth; all
  real logic stays in Rust. The SPA is deliberately thin so it can be
  rewritten someday without touching the system of record.

## 3. The DAC menu, mapped

The DAC's own operator menu structure is the Console's information
architecture — period-faithful *and* proven. Top-level sections:

| Console section | Original counterpart | Plan workstream |
| --- | --- | --- |
| Dashboard | headend status displays | all |
| Fleet | DAC §9 unit DB + §11 Manage Operations | W1–W3 |
| Discovery Lab | (no original — our box-as-oracle method) | W3 |
| Protocol | (the O4 document, browsable) | W3/O4 |
| Lineup & Plant | OM/DAC channel & source config | W2/W4 |
| Playout | (OM-side services) | W4 |
| Encryption | DAC EMMS config + SEM-EM (external) | W5 |
| Code Download | KLS/DLS console | W6/W8 |
| Guide | EPG inserter config | W8 |
| PHY / OOB | OM 1000/2000 front panel | W1 |
| Return Path | STARVUE II / STARFONE status | W7/S1 |
| Interconnect | (hubs — period interconnects) | S2 |
| Ops & Monitoring | DAC event/alarm logs | all |
| System | DAC system parameters | — |

## 4. Feature inventory (complete)

### 4.1 Dashboard

- Plant-at-a-glance: OOB carrier + regen rate + level log; SEM QAM
  summary; box state board (each unit: last E-code, auth state, last
  seen); guide carousel health; hub status; active alarms.
- Wallboard mode: auto-cycling full-screen status for a dedicated
  monitor — the ops-room vibe, zero interaction required.

### 4.2 Fleet (unit management)

- **Registry table:** every census field — serial, UA, Platform/Family/
  Model ID, tuner variant, firmware (d 06), resident modules (d 10),
  site/hub, cord date (CPSC 02-227 flag), battery state, role
  (sacrificial / instrumented / fleet), notes. Filters and saved views.
- **DAC terminal record (the Define Terminal schema, plan §3.2):** the
  operational half of every unit — Terminal Type (One-Way / **Fone-Way** /
  Two-Way / Ethernet / DOCSIS), 40-bit Unit Address (burned-in, shown
  `nnn-nnnnn-nnnnn-nnn`), 24-bit Terminal ID (system-assigned,
  1–16777215), Account Number, On-Plant flag, Last Response, Failures,
  Power-Leveling Attempts, Non-Participating Counts, Chronic
  Non-Responding flag (threshold tunable), Install/Activation dates,
  downstream/upstream plant assignment. Census fields identify the
  hardware; these fields run it.
- **Add box (enrollment wizard):** there is no over-the-air
  auto-enrollment on the classic platform (plan §3.2) — enrollment is
  operator-side. The wizard walks it: enter UA + serial from the unit
  label (or read them off d 01/d 05 via the capture rig) → assign
  Terminal ID, type, plants, account; pick the box type from the
  upstream catalog (a type not in it needs an upstream PR or a fork —
  ADR-003) → save to oob-dac's store
  (validated, journaled; no git, no PR — ADR-003) → guided
  recovery sequence with live box evidence: factory init if `E 09`,
  unit creation if `E 11`, then connect → watch `E 00` + d 02 data
  activity land inline. Dead NVRAM batteries (r4 F4-4) surface as
  recurring `E 09` and get a maintenance flag.
- **Unit detail:** identity card, state history timeline, diagnostic
  snapshots gallery (every d-screen scrape the capture rig has taken),
  message-traffic log excerpt for this UA.
- **Unit operations (DAC §11 catalog as actions):** Connect; **Disconnect**;
  Factory Initialize; Unit Create; Cold Initialize; Init/Refresh
  Authorizations; **Purchase / Diagnostic / Chronic-Non-Responding polls**;
  Virtual Channel push; Region; Interactive; General (log port enable,
  **code download enable**, RF bypass, purchase authentication);
  Multicast-16; Phone; **Configure Transmission Parameters** (Map Name,
  Amplitude, plants, Modem Bank, RPD channel + frequency, interactive
  IDs, impulse credit limits); Control Channel Timeout. Each action shows
  the underlying message template, supports dry-run, and links the
  expected box-visible confirmation (e.g., "`E 09` clears after factory
  init").
- **Bulk operations (Define Operations model):** target selector =
  All Terminals / Terminal-ID Range / Single Terminal, filtered by RADD,
  plant, terminal type, RPD channel, or Business System; inter-terminal
  delay enforced ≥250 ms; optional Repeat-Every schedule. Every bulk op
  runs as a trackable campaign with the DAC status lifecycle — New →
  Request Sent → Timeout → Downloaded → Retrieving Status → Completed /
  Failed — per unit, with dry-run first. The arrows follow the manual's
  list order (p. 11-4), not a documented transition sequence. The
  definitions are in the DAC-6000 Operations Reference §10. Oob's
  transition table is a `hypothesized` reconstruction, recorded on
  [#22](https://github.com/OnTheStruggleBus/oob-system/issues/22#issuecomment-7106984109).
- **Safety:** destructive ops (factory init, cold init) require typed
  confirmation (`FACTORY INIT <serial>`); bulk ops always dry-run first;
  the sacrificial box is clearly badged so experiments default to it.
  The Console only *presents* these rules: oob-dac enforces them for
  every client (F6-10 — owner role, a short-lived confirmation token
  bound to operation + target serial, dry-run default, audit log). The
  **sacrificial-only CDL lock** is a per-install owner-role setting, ON
  by default, shown here and on every campaign screen; turning it off
  needs only the confirmation token (Marshal, QA-7, ADR-003 §5).

### 4.3 Discovery Lab (the W3 workbench; minimal live for G1, full scope after E1)

- **Experiment runner:** pick template → fill fields (typed editors per
  field: hex, enum, address, checksum-auto) → inject now / schedule →
  live feedback panel (capture-rig camera OCR of the front panel,
  d 02 lock state, d 14 PID/message counters, RS-232 scrape on the
  instrumented DCT2500) → record verdict → save the experiment record
  (template hash + evidence) in the store. Templates the lab creates are
  `user-defined`; contributing one to the upstream protocol document is a
  PR with its capture, and only that PR can promote it to `verified`
  (F6-6, F6-7, ADR-003 §3).
- **Hypothesis backlog board:** the discovery queue (factory-init →
  unit-creation → connect → VCM → …, per r4 F4-4 ordering), drag to
  reorder, each card carrying its current evidence count.
- **A/B injection:** send variant A to the sacrificial box, variant B
  after reset, compare d 14 counter deltas side-by-side.
- **Rate discipline:** injection rate-limited against the insertion
  bandwidth budget (§4.10) so experiments can't flood the 2.048 Mb/s
  OOB stream (1.024 Msym/s QPSK); lab injection is traffic class C5
  (F6-3), capped and isolated; budget gauge visible in the runner at all
  times.

### 4.4 Protocol (O4, browsable)

- **Message catalog:** every message family with status badge —
  *documented* / *hypothesized* / *verified* — field dictionary, wire
  layout (hex map), evidence links (experiments that proved it),
  provenance (DAC manual §, install manual §, patent #, RE artifact).
- **Template editor:** templates versioned in oob-dac's store (upstream
  templates read-only, edits saved as `user-defined` copies; import/export
  as community packs — ADR-003); structural lint
  (length fields, checksum, address-mode legality per provider-ID
  rules); diff view between versions; "used by N experiments" backrefs.
- **Addressing model:** the d 05 taxonomy as a first-class reference —
  40-bit UA unicast, the headend's own Network Address, and the seven
  labeled Multicast-16 groups (NET / EMM / SCC / DWLD / DATA / VCN /
  POLL), with EMM provider-ID (0x0000/0x0001) scoping. Every template
  shows its address mode against this map; lint flags illegal
  combinations.
- **DCII PSI/SI conformance row** (r4 F4-6) tracked here alongside the
  message families, with d 15/d 16 scrape evidence.

### 4.5 Lineup & Plant

- **Channel table:** virtual channel ↔ source ↔ program ↔ QAM output ↔
  frequency; VCM editor; source-ID mapping for the EPG; per-hub variant
  view when Interconnect exists.
- **Spectrum map:** visual band plan — QAM assignments, the OOB carrier
  (highlighted within the hunt table), guard bands; guardrails enforced
  in the editor (≤860 MHz lineup limit, STD/HRC consistency with the
  SEM's plan setting — r4 F4-8).
- **Validation:** oob-dac validates every save (schema + lineup ↔ VCM ↔
  playout ↔ SEM consistency, moved from CI into the save path — ADR-003)
  and shows the result inline. The server refuses unsafe configuration
  for every client — lineup above 860 MHz, STD/HRC mismatch, wrong EMM
  provider ID, class budgets that starve C0 — with no override; other
  failed checks block the save and name the conflicting objects.

### 4.6 Playout

- **Service matrix:** which playout instance feeds which SEM input /
  program / QAM; SRT destinations (hubs) when present.
- **Content library:** loops, barkers, the recreation "how to use your
  service" channel; preview in-browser; loudness/codec lint (MPEG-2
  MP@ML only — F3-6 enforced here, not discovered on a box).
- **Job control:** ffmpeg/tsp process status, bitrates, PCR health,
  restart actions; per-service SCTE-35 placeholder (future).

### 4.7 Encryption

- **Tier matrix:** clear vs FPK per program; DES/CSA flavor display.
- **SEM panel:** status scraped where possible (SNMP if the MIBs turn
  up); ACP presence/license state logged at bench; **deep link to the
  archived SEM-EM** with the XP-VM runbook entry.
- **Boundary (honest scope):** SEM-side provisioning (routing, FPK
  enable, Annex mode) is performed *in SEM-EM* — the Console displays
  and documents it, it does not re-implement Motorola's element manager.
  The oob-dac EMMS interface (TCP 8080) remains a non-goal (Full
  Encryption removed from scope).

### 4.8 Code Download (CDL/DLS)

- **Object library:** .obj/.dat registry with the platform compatibility
  table (F0 census output: Platform/Family/Model → object); donor
  provenance (which box, dumped when, by which D0 route); storage
  pointers into the git-crypt archive.
- **Carousel builder:** object set → carousel config (segmentation,
  repetition rates) → start/stop; live DLS segment statistics.
- **Campaigns:** target selector (platform / site / explicit unit list),
  schedule, progress tracking per box (d 10 module state via capture
  rig), success/failure ledger, one-click rollback to the pre-campaign
  object set; **prominent link to the D0 flash-recovery runbook** on
  every campaign screen (r3 F3-3 discipline).

### 4.9 Guide

*The guide stack is **VueGuide+** (crate `oob-guide`): the listings
synthesizer, the guide feed, and — if the R12 fallback is ever reached —
the substitute grid. This section is its control room.*

- **Residency census view:** d 10 modules across the fleet, per platform
  (r4 F4-5 language: which platforms have which guide build).
- **Listings pipeline:** self-authored listings from the schedule DB in
  oob-dac's store (W4 amendment) → synthesizer → carousel; transport status (OOB vs
  in-band, per the F3-5 identification experiment); opacity-check
  results (r4 F4-3) surfaced as pipeline warnings.
- **Presentation config:** genre palette (factory defaults from the
  13-frame analysis; reminder that boxes can recolor box-side), brand
  slot (our operator logo), MSGS indicator / operator-message composer
  (writes to the Messages inbox — W8 stretch), Local Weather applet
  config (W8 stretch).
- **Ops:** carousel watchdog state and restart history (F3-10),
  cold-box repopulation latency measurements (M6 exit metric), day-skip
  spot-check results from the canary.

### 4.10 PHY / OOB

- **Carrier control:** hunt-table carrier select (104.2/110.25 class
  default), EMM provider-ID config (0x0000/0x0001 — with the
  re-hunt warning from §3.1 shown inline), level target and meter log
  (the F4-1 measurement results live here permanently).
- **Radio health:** HackRF/Pluto status, flowgraph start/stop, loopback
  BER test runner and history, drift watch (GPSDO escalation flag if
  d 02 ever shows drift).
- **Bandwidth budget:** the ~1000 Kbps insertion shaper as traffic
  classes C0 system … C5 lab (F6-3, ADR-002) — live per-class rate bars
  against each class's floor and cap. Floors and caps are operator
  settings, editable at runtime (ADR-003); the server refuses any
  configuration that starves C0 system.

### 4.11 Return Path (S1 stretch, present but dormant until W7)

- STARVUE demod monitor (burst log, d 13 socket states, d 12 sign-on
  state machine — `MAC CONNECT → … → WAIT_SO_ACK → RUNNING` — scraped per
  box), STARFONE modem bank status, poll/purchase ledger, power-leveling
  attempt tracking per unit (mirrors the DAC's Power Leveling Attempts
  field), non-participating dial counts for Fone-Way units.

### 4.12 Interconnect (S2 stretch)

- **Hub registry:** site.yaml per hub (ID, region tag, lineup mapping,
  radio roles), health (regen rate, buffer depth, tunnel RTT/loss),
  last-seen; per-hub actions (restart regen, push lineup, revoke keys).
- **Kit provisioning:** generates the hub bootstrap bundle (WireGuard
  keys, container compose, config) with a printed quick-start — the
  "ship a friend a hub" workflow.

### 4.13 Ops & Monitoring

- **Canary CI:** nightly reboot → `E 00` → AUTH assert history, trends,
  failure drill-down (which stage, which box, scrapes attached).
- **Alarms:** E-code transitions, OOB loss/regen stall, carousel stall,
  hub offline, SEM unreachable; routing (email/webhook); silence rules.
- **Log explorer:** unified view — experiment commits, admin actions,
  message traffic, container events; filter by unit/section/time;
  everything exportable for the runbook.

### 4.14 System

- **Users:** owner + read-only museum-mode guest; API tokens for
  automation (canary, capture rig, hub agents).
- **Config versioning:** revision history for every configuration object
  (units, lineup, templates, carousel configs, class budgets)
  and the append-only change journal; diff + rollback in UI (ADR-003).
- **Backup/restore:** the database is the installation (F6-5, ADR-003):
  one-click full backup and restore of the single sqlite file
  (continuous whole-file backup underneath), whole-install export, and a
  restore drill button (does a dry-run restore to a scratch dir and
  reports). Optional YAML export of runtime state for diffing — local
  only, never a gate.
- **Community packs:** import/export of templates, lineups, guide themes
  and brand skins; box-type profiles only as references to upstream
  catalog entries — an import that would add a new box type is refused;
  imported items are `user-defined`; the exporter strips or refuses firmware, keys and
  box-identifying data (ADR-003 §4).
- **Containers:** status, restart, image versions; update workflow note.
- **API explorer:** the WireLink verb reference (OpenAPI), try-it
  console — the same API the SPA uses, documented for scripting.

## 5. Phasing (C0 is a stub to live for G1; the full workbench follows E1)

| Phase | With milestone | Sections delivered |
| --- | --- | --- |
| **C0** | M0.5–M1 (G1) | **Stub to live:** a minimal live workbench against the oob-dac REST adapter (#27), with Protocol + Fleet (read + unit ops) + Logs. The full Discovery Lab / experiment workbench (§4.3) follows the E1 capture rig and is not a G1 exit criterion |
| **C1** | M2 | Lineup & Plant, Playout, PHY/OOB, Dashboard v1 |
| **C2** | M3 | Encryption panel (SEM monitoring, tier matrix) |
| **C3** | M4 | Code Download |
| **C4** | M6 | Guide, Ops & Monitoring polish, wallboard |
| **C5** | S1/S2 | Return Path, Interconnect |

C0 is deliberately thin everywhere except the Lab: the worst outcome
would be building a beautiful fleet manager before the first box
connects. The Console earns sections as the headend earns milestones.

**C0 scope: stub to live (Marshal's direction, 2026-11-23).** C0 ships in
two steps.

1. **For G1 (stub-to-live):** C0 is a minimal **live** workbench against the
   oob-dac REST adapter (issue #27). It calls the real adapter and shows
   real results; it is not mock data. This is the scope the G1 exit
   requires (Milestones, G1 full tier).
2. **Post-E1 (full workbench):** the experiment-workbench functionality in
   §4.3 (typed runner with live camera-OCR and d-screen feedback, the
   hypothesis backlog board, A/B injection) is deferred until the E1 capture
   rig exists. Its scope follows the [Capture Rig Specification](Capture%20Rig%20Specification%20—%20mini-headend.md).
   Because E1 is a G1 entry criterion, this full scope lands after G1 entry
   and is not a G1 exit criterion.

Every C0 operation still goes through the WireLink-verb admin API (§2). The
stub does not add logic that the API does not expose.

## 6. Non-goals

- **Not a SEM-EM replacement** (§4.7 boundary).
- **No billing/subscriber-management system.** The period DAC interfaced
  to billing; we have no subscribers to bill. The purchase/poll ledger
  (§4.11) is a log, not an invoice pipeline.
- **No multi-tenancy.** One operator, one estate, hubs are sites not
  tenants.
- **No internet exposure, ever.** LAN/WireGuard bind only; museum-mode
  guest is read-only by construction.
- **No mobile app.** The SPA is responsive enough for a bench tablet;
  that's the whole mobile story.

## 7. What "fully featured" means at done

At M6 exit, an operator can — from the Console alone, without SSH:

1. Enroll a box end-to-end: add it to the registry (UA + serial →
   Terminal ID, type, plants) and drive it from cold (`E 09`/`E 11`) to
   authorized (`E 00`) using only guided unit ops, watching box evidence
   inline.
2. Author or edit a message template, lint it, inject it, and file the
   experiment — saved with telemetry attached (contributing it upstream
   is a separate PR, ADR-003 §3).
3. Change the lineup and have validation, guardrails, VCM push, and
   playout follow in one save, with a revision to roll back to.
4. Run a full code-download campaign against a platform, with rollback,
   and read the per-box module states as proof.
5. Watch the guide carousel health, push an operator message to the
   MSGS inbox, and verify repopulation latency after a box cold boot.
6. See every setting the system holds, its current value, its source
   (store revision and journal entry; `user-defined` vs upstream), and
   the box-visible evidence that it's live.

Anything missing from that list is a Console bug by definition.
