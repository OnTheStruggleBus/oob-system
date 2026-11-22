# ADR-001 — Emulated GI/Motorola Cable Headend for DCT2000-Series Revival

- **Status:** Proposed (rev 8) — awaiting approval
- **Date:** 2026-11-21
- **Author:** Marshal
- **Project codename:** mini-headend
- **Target CPE:** Motorola/GI DCT2000-series digital cable terminals (SCTE 55-1 OOB platform)
- **Rev 2 changelog:** incorporated all 15 findings of adversarial review round 1.
- **Rev 3 changelog:** incorporated all 9 findings of adversarial review round 2.
- **Rev 4 changelog:** DAC-6000 + SEM V8 manuals acquired and mined; R1 bypassed
  via documented Fixed_PK mode; R2/R3 reduced; O8 closed.
- **Rev 5 changelog (Marshal's direction):** the **reproduction is the path**.
  Surplus hardware (incl. the DAC-6000 oracle) is demoted to pure opportunism —
  no planning weight, no dependencies. Wire-format discovery is adopted as
  planned work via the **box-as-oracle** methodology, backed by the DAC manual's
  §11 message-operations catalog (connect, VCM, region, inband-processor,
  general, phone, control-channel timeout, init/refresh authorizations) which
  seeds the protocol document. Companion plan: "rust-dac Reproduction Plan."
- **Rev 6 changelog (Marshal's requirement):** guide fidelity target set to
  **classic i-Guide** (Gemstar TV Guide Interactive, offered for the DCT2000
  family). The look is box-side software (the resident "interactive program
  application"), so the requirement decomposes into app residency (census via
  d 10; donor-object re-serve via our own CDL) and a synthesized OOB listings
  feed (format discovery via box-as-oracle + Gemstar/StarSight patent trove).
  New workstream W8 in the Reproduction Plan; M6 exit criteria updated; new
  risk R12; new open items O12/O13.
- **Rev 7 changelog (Marshal's call):** guide fidelity target is **whatever
  guide application the fleet's boxes actually run** (residency-first, per the
  d 10 census) — expected i-Guide, but the census defines "correct." Navigator
  (S-A/Cisco Explorer ecosystem, identified by Marshal from reference footage)
  is logged as a cross-ecosystem comparison reference; it cannot run on
  Motorola hardware and is not a target.
- **Rev 8 changelog (text hygiene, pre-approval; no decision content
  changed):** naming sweep to the settled 2026-11-22 names (rust-dac →
  Oob / `oob-dac`; Reproduction Plan and Operations Reference titles) in
  the living text — historical changelog entries above are left as
  written. §11 kill criteria reconciled with the post-r4 formulation
  already adopted in the Milestones & Stage Gates doc (rule 5: FPK-tier
  failure requires *both* no working ACP *and* the O3 routes failing),
  and the M3 milestone row aligned to the SEM-ACP reality (the SEM
  scrambles; playout stays clear). Rev 8 is the approval candidate.

---

## 1. Context

A collection of DCT2000-series set-tops is inert without a headend. On boot,
each box hunts for the out-of-band (OOB) forward data channel; failing to
receive a **connect message**, it halts at `E 01 NOT CONNECTED`:

| OSD | Meaning | Headend message required |
|-----|---------|--------------------------|
| `E 01` | No connect message received | connect |
| `E 09` | Dead battery / RAM not initialized | factory initialization |
| `E 11` | Invalid unit address | unit creation |

Acquisition state machine: `INIT → CONFI(gured) → ACQUI(ring) → AUTH(horized)`.
Unit address: 40-bit (16-digit decimal display). Firmware rev determines
diagnostic depth; DCT2500 units expose d 15–d 21 plus a standard RS-232 port.

The platform being emulated (roles per the DAC manual, §1):

```
Business System ──(WireLink 600/700)──► DAC 6000 ──► RADD 6000 (polling, downloads/DLS)
KLS 1000 (keylists; GCR from Motorola ECC) ──┘           │
SEM V8 (remux, MediaCipher encrypt, QAM mod) ◄── EMMs (DAC as EMMS)
LFG/local EPG server ── guide ──┐
NC 1500 ── interactive ─────────┤
DAC/RADD/DLS ───────────────────┴──► OM 1000/2000 ──► QPSK OOB ──► combiner ──► plant
(RPD 1000/2000 ◄── STARVUE II upstream; modem banks ◄── STARFONE calls)
```

**Physical layer (SCTE 55-1, per box spec):** QPSK at **75.250 / 72.75 /
104.2 MHz** (DAC manual: OOB 71–129 MHz), 1.5 MHz occupied, −10 to +5 dBmV,
2.048 Mbps aggregate (1.024 Msym/s). Streams are asynchronous, no PCRs,
null-stuffed by the modulator; DAC OOB insertion bandwidth typically ~1000 Kbps.
**Upstream:** unscheduled contention on multiple frequencies (1×3.2 MHz or
3×192 kHz, 5–42 MHz); DCT2000 has **integrated STARVUE II** standard; STARFONE
II phone return optional (headend modem banks; per-terminal phone parameters —
numbers, baud, retry, timeout — are DAC-provisioned).

**Security model (documented):** per-service encryption modes — Full
Encryption / Fixed_WK / **Fixed_PK (FPK)** / Unencrypted / Pre-encrypted.
Only Full Encryption requires DAC authorization to decrypt; FPK requires **no
EMMS, no keylists, no per-box authorization** (SEM manual: FPK works "without
the need for any other external devices"). Residual unknown for the encrypted
tier: the universal FPK key constant + DCII scrambling flavor (DES vs CSA).

**Goal (decided):** full DAC feature parity, including code download — via
**reproduction in software**, not adopted hardware — **plus a guide
experience faithful to whatever guide application the fleet's boxes actually
run** (rev 7, residency-first; expected i-Guide on DCT2000-class hardware,
confirmed by the d 10 census). The guide look is
box-side software (the resident interactive program application, historically
delivered by code download), so parity means: the app runs on each box
(resident or re-served via our CDL from a donor object) **and** our OOB
listings feed populates its grid. CDL = Common Download; the DAC manual §12
documents the object model (.obj/.dat), control messages (Enable/Disable/
Purge/Delete, distributed in the EMM stream), tune-to-download and
bootcode-control messages, and the DLS stream-builder role.

---

## 2. Decision drivers

1. **Fidelity** — boxes accept the headend as real; box instrumentation is a
   research tool, not a fleet requirement.
2. **Documentation over reverse engineering; the box over the auction site** —
   semantics come from the manuals; wire formats come from the box-as-oracle
   discovery loop (§4), with the boxes' own diagnostics as the conformance
   instrument. Surplus hardware is opportunistic only.
3. **Cost** — SDR-class budget.
4. **RF and electrical safety** — closed terminated coax; CPSC-recalled
   2002-era cords checked at census.
5. **Estate consistency** — Debian 13, Gitea config-as-code, git-crypt secrets,
   Zabbix/VictoriaMetrics, Rust daemons; analog plant (IAD2432 + Asterisk +
   mgetty) for STARFONE modem-bank emulation.
6. **Incremental value** — every milestone leaves boxes visibly "more alive,"
   with an explicit fallback definition of done (§11).

---

## 3. Considered options — OOB RF generation

### Option A — SDR-generated OOB carrier (SELECTED)

oob-dac emits the OOB TS → GNU Radio QPSK (1.024 Msym/s, per SCTE 55-1) → SDR
TX → BPF + attenuator → combiner. Same SDR later demods STARVUE II upstream
and sniffs box responses for the discovery loop.

### Option B — Surplus OM-2000 (opportunistic conformance reference)

Accepts TS over UDP/IP (or RS-530/RS-232). Nice-to-have conformance check for
Option A; zero planning weight.

### Option C — Captured OOB replay (REJECTED as a service; secondary reference)

Cannot address our fleet; superseded by the manuals (semantics) and the
box-as-oracle loop (wire formats).

### Option D — Surplus DAC-6000 oracle (DEMOTED, rev 5)

Value was message ground truth and mating ops; cost is acquisition luck. Rev 5
verdict: the box-as-oracle loop plus the §11 catalog covers the same ground on
our own schedule, and Full Encryption is out of scope regardless (needs the
defunct Motorola ECC keylist chain). **No longer a tracked option** — if one
falls in our lap cheaply it becomes a bonus reference instrument, nothing more.

---

## 4. Decision

Build **Oob** (`oob-dac`), a Rust software DAC implementing the documented
DAC/RADD/DLS/NC roles end-to-end: connect/configure/time, virtual channel map,
Fixed-mode authorization, CDL carousel, guide (local-EPG-server equivalent),
EAS, polling — muxed by tsduck (asynchronous, null-stuffed), QPSK via GNU
Radio, SDR-transmitted at the fleet's carrier frequency into a closed coax
plant. In-band: Annex B 64/256-QAM playout; **`lineup.yaml` single source of
truth**. Return path: STARVUE II upstream demod (M6); STARFONE modem-bank
emulation via the mgetty NAS (M5, stretch). Admin API adopts the WireLink
600/700 verbs and codes (660/662/663/664/666/680/684/685/688/694/720–726/
760/770/794).

**Wire-format discovery — box-as-oracle (new, rev 5).** The one thing the
manuals don't contain — bit-level message layouts — is discovered against the
boxes themselves: candidate messages flow SDR → box; feedback comes from the
front-panel state machine, d 01/d 02/d 10 diagnostics, the DCT2500's d 15
Message-Types screen and RS-232 console, and an SDR sniffer on the STARVUE II
upstream. Discovery order: SCTE 55-1 spec first (transport framing), then
structure-guided construction from the §11 semantic payloads, then patents,
then firmware-assisted parsing, with community captures opportunistic. The
loop is safe (boxes are already inert; Cold Initialize is the documented
recovery) and every experiment is logged as a commit so the protocol document
(O4) grows from reproducible evidence. Detail: "Oob Reproduction Plan —
mini-headend."

**R1 strategy (unchanged from rev 4):** FPK encrypted tier as the parity
milestone (M3); residual unknown is the universal FPK constant + scrambling
flavor (R10), hunted via firmware analysis (DCT2500 Broadcom path),
documentation, or opportunistic reference hardware. Full Encryption is out of
scope (KLS/ECC chain defunct); MCSC/TvPC mating is a side quest if cards
surface.

**Level budget:** in-band QAM ≈ 0 dBmV at the tap; OOB ≈ −5 dBc relative.

---

## 5. Box bring-up sequence (the contract we must satisfy)

1. **Carrier acquisition** — box finds our QPSK carrier (steerable via d 02).
   Verify d 02: lock **plus** data activity, `PID 1 received`.
2. **Connect** — connect message (§11 op); box leaves `E 01`.
3. **Unit creation / Cold Initialize** — documented out-of-box recovery for
   `E 09`/`E 11` units.
4. **Configure** — `CONFI`: VCM channel map, time + timezone, region,
   inband-processor (EMM provider ID, network unit address, multicast-40/24/16),
   general (log port, code-download enable, RF bypass, purchase auth),
   transmission parameters (upstream plant, RPD channel/frequency, modem bank),
   phone parameters (numbers, baud, retry, timeout). Set **Control Channel
   Timeout long** during development.
5. **Acquire** — `ACQUI`: tune a virtual channel, read PAT/PMT.
6. **Authorize** — clear and **Fixed_PK** channels decrypt with no EMM.
7. **Steady state** — polling/keepalive, guide carousel, EAS relay.
8. **Code download** — DLS-built carousel; control in the EMM stream; d 10
   shows `DOWNLD → ENABLED`.

---

## 6. Phased milestones

| # | Milestone | Exit criteria |
|---|-----------|---------------|
| ~~M0~~ | ~~Docs + corpus~~ | **Done:** manuals mined; protocol document seeded (Operations Reference + Reproduction Plan §2 catalog) |
| M0.5 | Spec + PHY conformance | SCTE 55-1 (2009) acquired; flowgraph parameters from spec; real box shows d 02 data activity on our carrier |
| M1 | OOB on air | Connect message discovered and emitted; ≥1 box leaves `E 01` → `E 00`; d 02 shows data activity + `PID 1 received`; DCT2500 d 15 logs our connect type |
| M2 | Configure + tune | VCM + time accepted; `CONFI`/`ACQUI`; ≥1 clear 256-QAM channel with A/V |
| M3 | Fixed_PK encrypted tier | FPK constant + flavor obtained; SEM ACP scrambles; fleet decrypts with zero EMM infrastructure |
| M3b | ~~Full Encryption~~ | **Removed (rev 5)** — needs defunct ECC keylist chain; superseded by FPK |
| M4 | Code download | DLS-equivalent carousel from .obj/.dat; sacrificial box completes a module download (d 10 `DOWNLD → ENABLED`); flash recovery verified first |
| M5 | STARFONE modem bank (stretch) | Box dials provisioned number → mgetty NAS; poll req/ack advances on-box; purchases ingested |
| M6 | Parity polish | Guide grid (per below), EAS, NC stubs, STARVUE II upstream demod |

**Guide (rev 7, workstream W8):** exit criteria = the resident guide grid
renders on ≥1 box and populates from our synthesized listings feed — fidelity
measured against whatever application the census finds (expected i-Guide:
banner, color-coded genre cells, day-skip, MSGS behavior — verified against
period references).
Two sub-problems: app residency (O12: d 10 census; donor object re-served via
CDL) and listings format (O13: box-as-oracle discovery + Gemstar/StarSight
patent trove + DAC Source-ID semantics). Detail: Reproduction Plan §4.1.

M2 = visibly working fleet. M3 + M4 = full parity per the agreed definition.

---

## 7. Risks and unknowns

- **R10 — FPK constant + scrambling flavor (pivotal unknown).** Single,
  universal, firmware-resident target; three acquisition routes. Failure costs
  only the encrypted aesthetic (see §11).
- **R11 — NEW: wire-format discovery stalls.** If the box-as-oracle loop can't
  converge on the connect message, the whole plan idles at M1. Mitigations in
  order: SCTE 55-1 transport framing; the §11 semantic payloads as field
  checklists; multicast-40/24/16 addressing taxonomy as a structural hint;
  patents; DCT2500 console/firmware parsing; community captures. Escalation
  rule: if connect isn't cracked within the M1 timebox, re-evaluate against
  opportunistic surplus (a DAC-6000 sighting changes the calculus).
- **R2 — REDUCED.** Download semantics documented; residual is carousel
  wire-framing via the same discovery loop. Soft-brick mitigations unchanged.
- **R3 — LARGELY RESOLVED** (Cold Initialize documented; wire format via loop).
- **R4 — Spectral hygiene.** BPF + attenuators, terminated plant, leakage sweep.
- **R6 — STARFONE protocol (non-gating stretch).**
- **R7 — Guide format (upgraded to requirement, rev 6).** No public spec or
  hobbyist RE for the i-Guide listings feed; discovery is planned work (O13).
  Non-gating for M2–M5 but gates the M6 guide exit criteria.
- **R12 — Guide app residency.** If no fleet box carries a guide application
  module (d 10 census, O12), the fidelity target is decided at that point:
  donor object re-serve first (fleet-external DCTs are cheap), substitute
  grid only as the degraded last resort — in that case themed after
  **Navigator/Passport** (the interfaces Marshal remembers most; both
  S-A-ecosystem, per rev 7 reference footage). Mitigation: census early;
  community contacts (technotite, Litestream channels) for objects/captures
  as backup.
- **R8 — Fleet heterogeneity (asset).** Instrumented DCT2500 designated at census.
- **R9 — Electrical safety.** CPSC recall cord inspection at census.

---

## 8. Compliance

- **RF:** closed plant only; leakage sweep before prolonged operation.
- **Electrical:** R9 cord inspection in the runbook.
- **Content:** self-generated or rights-free programming only.
- **EAS:** emulated alerts unmistakably labeled test traffic.
- **Secrets hygiene:** FPK constant, box dumps, firmware images, and any key
  material in the git-crypt tier or outside git; gitleaks; fresh-clone CI.
- **Scope:** self-owned hardware and self-generated signals only.

---

## 9. Infrastructure and operability

- **Host:** Debian 13 VM/CT on Proxmox; Tailscale; Firewalla-restricted lab VLAN.
- **Config-as-code:** oob-dac config, flowgraphs, `lineup.yaml`, unit DB
  (DAC §9 schema) in Gitea; secrets per §8; discovery-lab experiment logs are
  commits (message hash → box response → verdict).
- **CI/observability:** nightly canary reboot → `AUTH` on the clear tier via
  diagnostics scrape on the instrumented DCT2500; OOB lock + TS bitrate into
  VictoriaMetrics; Zabbix on carrier loss; lineup-consistency check;
  bsilog-style provisioning audit log.

---

## 10. Open items

- ~~**O8**~~ **CLOSED (rev 4).**
- **O1:** Surplus watch (OM-2000, DAC-6000, RADD, TvPC) — opportunistic only;
  escalates only per R11.
- **O2:** Box census (variants, OOB frequencies, E 09/E 11, return modules,
  firmware revs, IDs, cord dates); designate instrumented DCT2500.
- **O3:** FPK constant + scrambling-flavor plan (order the three routes).
- **O4:** Protocol document — seeded from manuals; grows via discovery-lab
  commits; tracks wire-format status per message.
- **O5:** STARFONE bootstrap — simulated-line sniff + box poll counters.
- **O7:** Acquire ANSI/SCTE 55-1 (2009) — gates M0.5 and discovery step 1.
- **O9:** Probe DCT2500 RS-232 console.
- **O10:** Patent pass (General Instrument, ~1993–1999) on OOB messaging and
  download formats.
- **O11:** DCII scrambling flavor for Fixed modes (DES vs CSA) — folds into O3.
- **O12:** Guide app residency census — read each box's d 10 module list at O2;
  identify the i-Guide application module and a donor unit.
- **O13:** i-Guide listings format discovery — box-as-oracle loop seeded by the
  Gemstar/StarSight patent trove and DAC Source-ID semantics; self-authored
  listings content from `lineup.yaml`.

---

## 11. Kill criteria

FPK-tier failure is declared only when **both** conditions hold (Milestones &
Stage Gates doc, rule 5, post-r4 formulation): the estate SEM V8's ACP cannot
be made to scramble in a mode the fleet decrypts, **and** the FPK constant
(O3) cannot be obtained after a bounded effort (bound set by Marshal at M3
planning). If FPK-tier failure is declared, definition of done falls back to
**M2 + M4 + M5**: a fleet that locks, configures, tunes clear channels,
downloads code, and phones home — losing only the "encrypted tier" aesthetic.
If wire-format discovery stalls past the M1 timebox (R11), re-evaluate
opportunistic surplus before extending the box-as-oracle budget.

---

## References

- **Basic DAC 6000 (V3.1) Operation, Rev 1.2** (648 pp, 2006) — system roles,
  encryption modes, KLS/GCR/UKL, terminal lifecycle, WireLink command set,
  download management, §11 message-operations catalog, OM stream parameters.
- **Motorola SEM V8 SmartStream Encryptor Modulator, SW 7.1.x** (295 pp, 2015)
  — ACP/MediaCipher (DES+CSA), FPK without EMMS, CTE, EMMG interfaces.
- **Oob Reproduction Plan — mini-headend** (this repo) — workstreams,
  discovery methodology, repo layout, first-two-weeks plan.
- **DAC-6000 Operations Reference — Oob Design Basis** (this repo).
- DCT2000 Installation Manual; DCT2500 Installation/User Guide.
- CableLabs CM-SP-R-OOB-I03-160512; CM-SP-EQAM-VSI-I01-081107.
- SEC-filed IP License Agreement — MediaCipher IC family (MCSC/MC1.7/MC2.x).
- OM-2000 product manual; Oracle ASAP DAC-6000 Cartridge Guide.
- CPSC Recall 02-227.
- ANSI/SCTE 55-1 (2009) — acquisition pending (O7); SCTE 28 (CVT locator).
- bcm2-utils / ProgramStore — Broadcom firmware tooling precedent.
- i-Guide provenance: Gemstar–TV Guide Interactive renamed i-Guide by
  Macrovision/Rovi; offered for the DCT2000 family (Wikibooks Motorola DVR
  firmware notes); classic-look reference: TV Guide Interactive genre
  color-coding; DCT2000 install manual ("interactive program application
  resident on the DCT 2000"; d 10 module display).
