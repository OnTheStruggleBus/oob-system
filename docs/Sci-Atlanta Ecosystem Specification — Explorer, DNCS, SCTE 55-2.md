# Sci-Atlanta Ecosystem Specification — Explorer / DNCS / SCTE 55-2

- **Date:** 2026-11-22
- **Status:** Reference — companion to the GI/Motorola spec set (ADR-001, DAC-6000 Operations Reference, rust-dac Reproduction Plan)
- **Purpose:** (1) authoritative reference for the **Navigator/Passport look** — the interfaces Marshal remembers most and the R12 substitute-grid theming target; (2) a parallel specification if an Explorer fleet is ever revived or a dual-vendor plant is attempted; (3) a contrast document that sharpens what is unique about the GI side we are actually building.

---

## 1. The two ecosystems at a glance

Every GI/Motorola element in mini-headend has a Scientific-Atlanta counterpart:

| Function | GI / Motorola (ours) | Scientific-Atlanta |
| --- | --- | --- |
| Plant architecture | DAC-centric headend | **DBDS** — Digital Broadband Delivery System |
| Headend brain | DAC 6000 (+ RADD/DLS/KLS) | **DNCS** — Digital Network Control System |
| CA / encryption | MediaCipher (SEM V8) | **PowerKEY** (+ TED, Netcrypt) |
| Set-tops | DCT2000 family | **Explorer / DHCT** family |
| Box OS | GI native (68k) | **PowerTV OS** (later Cisco-era stacks) |
| OOB forward | SCTE 55-1 Mode A, QPSK 1.024 Msym/s, MPEG-TS payload | **SCTE 55-2 Mode B**, QPSK 772 ksym/s, **ATM/DSM-CC** payload |
| OOB return | STARVUE II (RF, 5–42 MHz) / STARFONE (phone) | DAVIC RDC, QPSK TDMA, 8–26.5 MHz |
| Code download | DLS carousel, .obj 68k modules | Broadcast File Server carousel, network download |
| Guide (period) | i-Guide (Gemstar/TV Guide Interactive) | **Passport → SARA "Navigator"** |
| Ops interface | WireLink verb API | DNCS GUIs (X/Java), SNMP, ROSA EMS |

The ecosystems are fully parallel but wire-incompatible — an Explorer cannot
hear a 55-1 stream and vice versa. Real MSOs ran mixed plants (Charter, TWC)
with both OOB carriers present, which is why later CableLabs documents treat
55-1 and 55-2 side by side.

## 2. Headend — DBDS and the DNCS

DBDS splits into headend / transport network / hub / access network. The
headend equipment roster (per Cisco DBDS training material):

- **DNCS** — the control-system brain. Two control suites (per catv.wiki):
  - **Broadband Control Suite** — broadcast equipment + DHCT control
  - **PowerKEY Control Suite** — conditional access
  Originally Sun **Solaris** servers (process names and paths like
  `/dvs/dncs/bin`, `svcadm`, `/etc/rc2.d` in field docs), later ported to
  Windows. Database `dncsdb`; web GUIs via Tomcat/Java SOAP servers; SNMP
  agent. Field releases numbered like "SR 4.3".
- **TED** — Transaction Encryption Device; secures purchase transactions;
  paired with a DNCS health-check process (`camTedChecker` green/yellow/red
  = in service / keys uninitialized / down).
- **App Server** — workstation serving DHCT applications; the IPG is one
  such app.
- **Encoders** — Continuum DVP line: D9020 / D9022 (SD MPEG-2), D9032
  (PreSightPlus pre-processing, closed-GOP statmux with Regulus controller),
  D9050 (HD MPEG-2).
- **DCM** — Digital Content Manager: statmux, transrating, digital program
  insertion, interface conversion.
- **Netcrypt** — bulk/edge encryption.
- **QAM modulators** — Continuum 9825M/9825S + 9815 RIM era; 9811/9814
  controllers manage chassis modules. Later ROSA EMS for element management.
- **Hub** — QPSK modulators/demodulators for OOB; one 55-2 modulator can
  serve >10,000 DHCTs, with up to 8 upstream demodulators per modulator.

### 2.1 DNCS process anatomy (from the field glossary)

The DNCS glossary reads exactly like our DAC §11 catalog did — a message/
service backlog:

| Process | Role |
| --- | --- |
| `bsm` | Broadcast Segment Manager — broadcast segment definitions; notifies CA system when broadcast sources start |
| `caaServer` | Creates/sends CAA and Entitlement-Authority **EMMs** for staging set-tops |
| `camAm` | Creates EMMs for security elements (authorize DHCTs for secure events) |
| `camAuditor` | Finds boxes whose EMMs expire within 20 days; regenerates for 30 days (does not transmit) |
| `camEx` | VOD exclusive sessions — interactive session keys (ISKs), ISK EMMs; generates ECMs for QAMs |
| `camFastRefresh` | Stages EMMs in files on the **BFS** (Broadcast File Server); sends staging EMMs in-band to Fast Refresh candidates |
| `camPsm` | PPV + CA Time-of-Day **GBAMs** (global broadcast authenticated messages); inserts CA info + ECMs into program segments |
| `camTedChecker` | TED health (green/yellow/red) |
| `dncs-snmpd` | SNMP management |

Plus GUI servers (`dbUIServer`, `rpcUIServer`, …) and Tomcat/SOAP web tiers.

## 3. OOB PHY — SCTE 55-2 Mode B (DAVIC / DVS-178)

Mode B is DAVIC-derived (the spec's own revision history begins "QPSK Out of
Band Channels based on DAVIC"), and it is architecturally nothing like Mode
A: **the payload is ATM cells, not MPEG-TS**, and the framing is borrowed
from T1 ESF.

### 3.1 Downstream (forward data channel)

Fully characterized by the oobscte55 project's decode of a live
Charter/Spectrum (S-A/Cisco) plant:

| Parameter | Value |
| --- | --- |
| Standard | SCTE 55-2 (DVS-178 / DAVIC Mode B), Grade A |
| Modulation | Differentially-encoded QPSK, SRRC α = 0.30 |
| Symbol rate | **772 ksym/s → 1.544 Mb/s** |
| Channel width | 2.0 MHz (window 70–130 MHz; plant-typical carriers 75.25 / 104.25 MHz per CableLabs R-OOB) |
| Randomizer | Self-synchronizing LFSR, **x⁶+x+1** (alternate polynomial x⁶+x⁵+1 per CableLabs config) |
| Framing | **SL-ESF** superframe: 4632 bits = 24 × 193-bit frames |
| Frame alignment | F1..F6 = 0,0,1,0,1,1 |
| Interleaver | Forney convolutional, I = 5, M = 11 |
| FEC | Reed-Solomon **(55,53)** over GF(256), p(x)=x⁸+x⁴+x³+x²+1 |
| Data link | ATM → AAL5 → IPv4 → UDP (ports 13818/13821) → DSM-CC object carousel → BIOP objects |

Two VCIs do the system's heavy lifting (per oobscte55):

- **VCI 0x0021 — MAC control channel:** downstream/upstream provisioning
  frequencies, upstream power levels, slot configuration, upstream rate.
- **VCI 0x0fa2 — host/OCAP configuration:** in-band carousel QAM frequency +
  program number, firmware/code-version server URLs, OCAP navigator Xlet
  names, multicast parameters, object versions.

The **EPG lineup itself rides the OOB** as DSM-CC carousel objects
(`airtime*.grp.z` grid modules etc.) — the oobscte55 pipeline recovers
callsign→channel-number mappings and display names from a 30-second
recording; full program grids need a few minutes for all carousel modules
to repeat.

### 3.2 Upstream (reverse data channel)

- QPSK, **TDMA with contention-based access**; slot assignments delivered in
  the downstream MAC overhead — a real MAC layer with ranging/calibration.
- Rates: **1.544 Mb/s** (Grade B), 256 kb/s, or 3.088 Mb/s.
- Window 8–26.5 MHz (2.0 MHz channel); one modulator pairs with up to eight
  demodulators; all demodulators in an upstream group share slot assignments.
- MAC functionality per SCTE 55-2 §2.3: initialization/provisioning/sign-on,
  connection establish/release, link management.

### 3.3 Contrast: what makes 55-2 different from our 55-1 target

| Aspect | 55-1 Mode A (GI) | 55-2 Mode B (S-A) |
| --- | --- | --- |
| Symbol rate | 1.024 Msym/s | 772 ksym/s |
| Payload | MPEG-TS sections | **ATM cells** (AAL5/IP/UDP/DSM-CC) |
| Framing | TS packets | SL-ESF superframes (T1 heritage) |
| FEC | (per 55-1, O7) | RS(55,53) + Forney I=5/M=11 |
| Return | proprietary STARVUE II | DAVIC TDMA MAC with ranging |
| Carrier acquisition | box hunts 10 fixed carriers (DCT2000 manual) | box provisioned via MAC control channel |

CableLabs' Remote-OOB spec (CM-SP-R-OOB) documents both in one place and
confirms the coexistence model: 55-1 downstream 70–130 MHz / upstream
5–42 MHz; 55-2 downstream 70–130 MHz / upstream 8–26.5 MHz.

## 4. Boxes — the Explorer / DHCT family

| Era | Models | Notes |
| --- | --- | --- |
| Compact | Explorer 940 | digital-only, small form factor |
| First digital | 2000 / 2100 / 2200 | 2200 datasheet: 32-bit 166 MHz CPU, DAVIC reverse transmitter, 640×480 graphics @ 65k colors, 64/256-QAM Annex B, AC-3 + Musicam, internal security µP + smart-card slot, network code download |
| HD | 3100 / 3170HD / 3250HD | first HD (2003 era) |
| CableCARD | 4200 / 4240C / 4250C / 4250HDC | M-Card separable security (post-July-2007 FCC mandate); FDC 1.54 Mb/s QPSK, RDC 1.54 Mb/s QPSK; some with DOCSIS option |
| DVR | 8000 / 8100 / 8240 / 8300 / 8300HD / 8240C | 8000HD (2004) = first HD DVR; dual-tuner; the TWC "Navigator" platform |
| Cisco era | 9865HDC et al. | Charter/Spectrum Navigator |

Platform software: **PowerTV OS** — from PowerTV, Inc. (Cupertino; S-A
invested 1994, its software shipped on the Explorer 2000, S-A acquired
PowerTV outright March 2001). HTML client engine, native APIs, network
download. Later Cisco-era boxes moved toward Linux/Java stacks with
OCAP/tru2way Xlets.

## 5. Conditional access — PowerKEY

- Per S-A's own datasheet: secures digital services "using an **RSA
  encryption algorithm** that mathematically matches pairs of keys" — a
  public-key pairing between headend and security element, with an internal
  security microprocessor and (later) smart-card / M-Card renewal.
- EMM economy documented in the DNCS glossary: CAA/EA staging EMMs
  (`caaServer`), security-element EMMs (`camAm`), 20/30-day refresh
  (`camAuditor`), BFS-staged fast refresh, GBAMs for PPV/time-of-day, ISK
  EMMs for VOD sessions (`camEx`), per-segment ECMs (`camPsm`).
- TED secures purchase transactions; Netcrypt does bulk/edge encryption.
- **Open research note:** no published analog of Motorola's FPK "free
  public key" mode is known for PowerKEY. If an Explorer revival ever
  happens, the auth-bypass question must be answered fresh — do not assume
  the FPK trick ports across ecosystems. (Contrast: our GI kill criteria
  rest entirely on FPK.)

## 6. Guide lineage — Passport → SARA "Navigator"

- **Passport** — S-A's earlier IPG generation (late-90s Explorer era).
  A port, **Passport Echo**, ran on Motorola DCT2000 hardware (deployed by
  Insight Communications) — proof the look Marshal remembers crossed vendor
  lines. *(Lineage per community/background knowledge; **visually
  corroborated 2026-11-22** by Marshal's Cincinnati Bell screenshot — a
  Passport Echo grid running on DCT-class hardware, dated Mon 12/29/2008
  in-frame, complete with the Motorola DRC remote's A/B/C color keys in its
  command bar. Period documentation still on the acquire list.)*
- **SARA** — Scientific-Atlanta Resident Application: the box-resident
  guide/middleware on Explorer DHCTs. **"Navigator"** is the operator-facing
  name of the guide UI on SARA — the "blue on blue" TWC interface on
  8300/8300HD that Marshal's reference footage shows, continuing into the
  Cisco/Charter era on 9865HDC.
- Guide data delivery: DSM-CC object carousel over the 55-2 OOB — lineup
  objects plus `airtime*.grp.z` program-grid modules (see §3.1); in-band
  carousel also used (host/OCAP config VCI points at the in-band QAM).
- Later: OCAP/tru2way Xlet-based guides on Cisco-era hardware.

**For R12 theming (our project):** the Navigator/Passport visual language
comes from this ecosystem, but the substitute grid runs on *our* GI boxes —
theme is skin-deep; the wire stays 55-1. References (3) and (4) in the
Reproduction Plan (spunker88 Navigator, themilwaukeeboy 6/30/2000 Passport)
remain the visual baseline.

## 7. Community art and document status

Much better documented in the open than the GI side:

- **oobscte55** (sarahsforge.dev, MIT, Python) — complete 55-2 receive
  pipeline from raw IQ to EPG lineup, decoded against a live Charter/Spectrum
  plant. **This is a ready-made conformance oracle:** for a 55-2 transmitter
  build, every layer (QPSK → derandomize → SL-ESF sync → deinterleave →
  RS(55,53) → ATM → AAL5/IP/UDP → DSM-CC → BIOP) has a reference decoder.
  The GI side has no equivalent — our box-as-oracle loop exists precisely
  because nothing like this exists for 55-1 message semantics.
- **SCTE 55-2 2008** — purchasable spec; the ANSI store preview exposes the
  full TOC (downstream QPSK §2.1, upstream QPSK/TDMA/contention §2.2, MAC
  §2.3). 2019 (R2024) revision current.
- **CableLabs CM-SP-R-OOB-I06-170524** — Remote OOB spec; free; documents
  the 55-2 downstream/upstream packet formats, controller/modulator/demod
  split, randomizer polynomials, idle-cell behavior ("missing DAVIC frames
  will cause all DHCTs to drop off the network and be forced to do network
  reentry"), and the 55-1/55-2 coexistence tables.
- **catv.wiki** — S-A document trove: Explorer datasheets (2200, 4250…),
  DNCS page.
- Cisco Community "General DBDS Information" — headend/hub unit roster and
  the DNCS process glossary quoted in §2.

## 8. If we ever build it — mini-DNCS sketch

Parallel to the Reproduction Plan, at envelope-back depth:

1. **PHY TX** — GNU Radio: differential QPSK, SRRC α=0.30, 772 ksym/s into a
   2.0 MHz channel on a chosen carrier in 70–130 MHz. Implement randomizer,
   SL-ESF framing, Forney interleave, RS(55,53). **Validate with oobscte55
   as the RX oracle** — loopback conformance from day one, no box needed
   until the message layer.
2. **Stack** — ATM cell assembler → AAL5 → IPv4/UDP → DSM-CC carousel
   builder (BIOP objects). Heavier than our TS-section world; budget
   accordingly.
3. **MAC** — downstream slot assignments + upstream TDMA demod (SDR RX,
   8–26.5 MHz) for two-way; or run broadcast-only initially, as we do
   on the GI side.
4. **Control semantics** — reimplement the DNCS backlog (§2.1) as daemons:
   staging EMMs, GBAMs, carousel refresh. Box bring-up semantics unknown
   until an Explorer install manual is acquired (Explorer-side observable
   contract = the O2-analog census).
5. **Guide** — SARA resident app is the residency question (donor dump or
   synthesize DSM-CC objects that the resident app parses — same
   strategy as W8, different object format).
6. **CA** — PowerKEY RSA pairing with no known FPK analog = the R10-analog
   risk. Unassessed.

Feasibility verdict vs the GI build: PHY is *easier* to validate (open
decoder + public spec previews), the application stack is *heavier*
(ATM/DSM-CC vs MPEG-TS sections), and the CA question is *less* explored
(no FPK precedent). Boxes are equally cheap e-waste. Not recommended as a
second front while mini-headend is pre-M1; entirely viable afterward with
the same bench (SDR + capture rig already covers both windows).

## 9. Sources

- Cisco Community, "General DBDS Information" (2012) — DBDS architecture, DNCS process glossary.
- catv.wiki — "Digital Network Control System"; "Explorer 2200 Digital Interactive Set-Top" datasheet (S-A, March 2002, PN 752062 Rev A).
- sarahsforge.dev, "oobscte55" (MIT) — 55-2 decode pipeline + signal parameter table.
- ANSI/SCTE 55-2 2008 preview (ANSI Webstore) — TOC of downstream/upstream/MAC sections.
- SCTE 55-2 2019 (R2024) via SCTE account — current revision.
- CableLabs CM-SP-R-OOB-I06-170524 — Remote OOB spec: packet formats, frequency windows, coexistence.
- manuals.plus — Explorer 4240C/4250C user guide; Explorer 4250C/4250HDC (FDC/RDC 1.54 Mb/s QPSK).
- Wikipedia, "Scientific Atlanta" — market share, Explorer 8000HD timeline.
- company-histories.com — S-A/PowerTV 1994 investment; PitchBook — PowerTV acquisition 2001-03; CNET (2000) — PowerTV OS on Explorer 2000.
- AVS Forum "Time Warner Cable Navigator" thread; jimcofer.com "Time Warner: Navigator sucks!" (2008) — Navigator/SARA period behavior ("blue on blue").
- Explorer 2200/3200 user guide (brctv.com); Vidéotron Explorer/Pace terminal guide — DHCT front panels.
