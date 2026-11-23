# Hardware Acquisition List — mini-headend

*2026-11-23, rev 5. Consolidates the Reproduction Plan §8 buy list, the
r3/r4 review deltas (F3-2 capture rig, F3-3/F4-3 D0 tooling, F4-1 radio
roles, F4-4 batteries, F4-7 networking), and the Milestones & Stage Gates
§6 tool stack into one prioritized list. Priority follows the gate plan:
buy in P-order and each purchase unblocks a named gate or track. Surplus
finds stay opportunism (O1) — they appear here with zero planning weight.*

**Rev 2 changes (Marshal's cost direction, 2026-11-22):** the two-SDR
line items (~$430+ as listed) are restructured into a cost-optimized
radio plan — Pluto-first TX at $0, RTL-SDR-class upstream RX at ~$40,
HackRF *clone* (~$105–150) demoted to metered fallback. See P0-1 and
P2-1. Note: this revises the radio-role assignment implied by
undispositioned finding r4 F4-1 (HackRF primary); the metering F4-1
requires happens either way, and this plan is offered for Marshal's
disposition alongside r4. (r4 was since accepted, 2026-11-22.)

**Rev 3 changes (Marshal's direction, 2026-11-22):** **LibreSDR** folded
in as an *eventual option* — source listing logged (AliExpress item
3256807990830574). Profile and caveats in P3-4; referenced from the
P0-1 fallback set and the hub kit (Appendix A). Not an immediate buy.

**Rev 4 changes (Marshal's question, 2026-11-22):** commercial /
purpose-built hardware survey added as P3-5 — period GI gear (extends
O1), the DekTec DTA-2115B metrology-grade modulator (benchmark
reference), commercial QAM alternatives (SEM-spare/hub class), and
receiver-side verification instruments. All watch-list entries with
zero planning weight.

**Rev 5 changes (Marshal's direction, 2026-11-23):** the OOB TX is a
**supported set** — ADALM-Pluto (range-hacked at the hunt carriers),
HackRF One clone, and LibreSDR (P3-4 is now in the set; its buy status is
unchanged and no purchase is implied). The **primary is chosen at the
G0.5 bench** by the F4-1 level log, not pinned here. This note records the
hardware-option delta; the r4 F4-1 record is unchanged and not
dispositioned by this revision. The capture-rig kit (P0-3) is now specified
in the [Capture Rig Specification](Capture%20Rig%20Specification%20—%20mini-headend.md).
Rev 5 adds no buy and no spend.

**Already in the estate (no purchase):** Motorola SEM V8 P/N
507199-001-00 (ACP confirmed — QAM + FPK encryption), ADALM-Pluto (OOB
TX candidate and upstream monitor), the DCT fleet (DCT2000 + DCT2500 +
DVR models), Debian 13 host, analog plant (IAD2432 + Asterisk + mgetty
NAS) for STARFONE.

---

## P0 — Buy now (unblocks G0.5 and G1)

### P0-1. OOB radio — Pluto first ($0), metered fallbacks

**Cost-optimized plan (rev 2, fallback set extended rev 3):**

1. **ADALM-Pluto (in estate, $0).** Apply the AD9364-identity range
   hack (required at *every* hunt carrier, r4 F4-1), then meter actual
   delivered level at the chosen carrier through the full F3-7 chain
   into a dummy load. If the budget closes to the −10…+5 dBmV window at
   the box input with the mandatory bandpass filter in-line (the filter
   is already in P0-2 and cleans the Pluto's sub-150 MHz spurs), the
   Pluto is the OOB TX and no radio purchase happens at all.
2. **Fallback set (buy one, only if the Pluto meters short):**
   - **HackRF One *clone* (~$105–150), not a genuine unit (~$340).**
     rtl-sdr.com's clone-vs-original bench comparison found the price
     gap hard to justify; for a constant-envelope 1.024 Msym/s QPSK
     carrier into a filter + attenuator, clone phase noise is not the
     bottleneck. Bare clone board (no PortaPack), listing with returns,
     metered before entering the chain. Covers the whole hunt table in
     spec, no hack; doubles as the STARVUE upstream receiver later.
   - **LibreSDR (~$120–180 at the logged AliExpress listing; other
     sellers up to ~$260–320 — verify seller and price).** PlutoSDR-
     firmware-compatible clone (AD9363 + Zynq 7020): **native
     70 MHz–6 GHz** (whole hunt table, no range hack), 0.5 ppm-class
     VCTCXO (answers the drift/GPSDO escalation path), built-in
     Ethernet (network-attached like the SEM — fits the containerized
     lab). Caveat: a bench review observed a TX center (LO) spike in a
     DATV test that a genuine Pluto did not show — right where a
     narrowband carrier lives — so it goes through the same metered
     dummy-load bench rule; full profile in P3-4. Choose this over the
     HackRF clone if the gr-iio flowgraph stack and Ethernet placement
     are worth more than the HackRF's wider community support.
3. **Decision point:** G0.5 exit records which radio committed, with
   the level log as evidence (unchanged from r4 F4-1's metering
   requirement — only the preference order changes).

- **Unblocks:** G0.5 exit (OOB TX committed), G1 entry.

### P0-2. F3-7 RF chain (r3 F3-7)

The chain between SDR and plant, bought as one kit:

| Item | Spec / notes |
| --- | --- |
| Bandpass filter | For the chosen OOB carrier (104.2/110.25 class, decided at census); also suppresses Pluto/LibreSDR spurs (r4 F4-8: FM-band adjacency makes this mandatory, not optional) |
| Small gain block / PA | Sized after metering radio output through the chain — buy only if the level budget doesn't close (r4 F4-1 delta) |
| Variable/step attenuators | 0–30 dB class, to land ≈0 dBmV at the box input (window −10…+5 dBmV) |
| 2-way combiner | Merges OOB carrier with SEM QAM output |
| Directional tap / coupler | Plant feed + measurement port |
| 75 Ω dummy loads / terminators | Several — dummy-load-first is doctrine (closed-plant rule) |
| RG6 + F-connectors + tools | Compression fittings preferred; shielded passives throughout |
| 2-way splitter | Census bench (feed one box at a time off the tap) |

- **Unblocks:** G0.5 exit (level log through the chain into a dummy load).

### P0-3. Capture rig (E1, r3 F3-2 — "highest-leverage purchase in the plan")

*Specified in the [Capture Rig Specification](Capture%20Rig%20Specification%20—%20mini-headend.md) (rev 5 of this list).*

| Item | Spec / notes |
| --- | --- |
| USB camera | Fixed-focus, macro-capable; pointed at the sacrificial box's front panel for 7-seg OCR (OpenCV + Tesseract, whitelist-trained on E-codes/d-screen digits) |
| Composite/HDMI capture card | d-screen OSD grabs — the d-screens render on video out, so this scrapes d 01/d 02/d 10/d 14/d 15 automatically |
| USB RS-232 adapter | DCT2500 console scrape (fleet confirmed to include DCT2500s); null-modem cable/adapter as the port requires |

- **Unblocks:** G1 entry (E1 exit = zero-human-interaction evidence per
  experiment). Without it the connect-message search dies of tedium
  (r3 F3-2).

### P0-4. D0 flash tooling (r3 F3-3, r4 F4-3 — pre-M1, gates three workstreams)

| Item | Spec / notes |
| --- | --- |
| TL866 II Plus (minipro-compatible) | Flash dump/restore route 1 |
| TSOP-48 adapter set | Socket + in-circuit clip; DCT2000 NOR flash package — confirm against the sacrificial box's board rev before ordering the clip variant |
| 68k BDM wiggler | Route 2 (gdb stub); verify BDM header population on the board at census — if absent, route 2 becomes clip-and-solder or an alternate programmer, and E2's "two validated routes" exit needs a re-plan |
| FX2 logic analyzer | sigrok/PulseView bus probing — the RE workbench's fallback when the paper trail runs out (cheap; buy with the rest) |

- **Unblocks:** E2 exit → G4 entry (no download enable without verified
  recovery), W8 donor path, O13 parser RE. Buy with P0 so validation
  finishes before G1 closes (r4 F4-3 promotes D0 to a named pre-M1 task).

### P0-5. NVRAM batteries (r4 F4-4)

- Soldered lithium cells + holders matching the fleet's boards (verify
  cell type at first open-box D0 session); log per-box battery state at
  census. **Opportunistic fix only** — the factory-init message path
  makes dead-battery boxes fully functional with zero soldering; replace
  batteries when a box is open anyway.

---

## P1 — Buy before G2/G3 (clear tier + FPK tier)

### P1-1. Managed switch (r4 F4-7)

- Between playout host and SEM to keep GigE multicast clean (IGMP
  behavior with the SEM's GigE port is untested — the switch is the
  controlled environment for that test). Small 5–8 port managed unit is
  sufficient. (If a LibreSDR ever joins the bench, its native Ethernet
  lands on this switch too.)

### P1-2. Copper 1000BASE-T SFP — *conditional*

- Only if the SEM's GigE SFP cages are unpopulated at bench bring-up
  (E4); use a module from the SEM manual Appendix B approved-vendor
  list. **Verify before buying.**

### P1-3. SEM spares and consumables

- Nothing required; rack ears/power cords only if the unit arrives
  without them. (A spare SEM is O1 surplus opportunism — see P3-3.)

---

## P2 — Buy before G4/G5 (code download drills + return path)

### P2-1. Upstream RX SDR — RTL-SDR class (~$30–45), not a second HackRF

**Cost-optimized plan (rev 2):** the STARVUE window (5–42 MHz) carries
narrow channels (192 kHz / 3.2 MHz plan); 2.4–3.2 Msps capture
bandwidth is ample, so a receive-only dongle does the G5 job.

1. **RTL-SDR Blog V3 (~$45 with antennas)** — direct-sampling for the
   HF end + R860 tuner above ~24 MHz covers the full window; doubles as
   the leakage-sweep spectrum monitor for the closed-plant doctrine.
   Market note (2026-11): the better V4 went end-of-life (R828D stock
   exhausted); remaining V4 units are climbing (~$65) and the V3
   returns as the volume model.
2. **Or wait for the V4L "Lite"** (same V4 architecture incl. the HF
   upconverter; first batch expected ~1–2 months from 2026-11). G5 is a
   post-G4 stretch, so waiting costs nothing; the V4's street price was
   $29.95 dongle-only / $39.95 with antennas.
3. **$0 interim:** the Pluto can monitor the upstream window at the
   bench (r4 F4-1) — evaluate before buying anything. If a LibreSDR
   joins the estate (P3-4) it is also a competent full-duplex upstream
   receiver in this window.

(A second HackRF or an SDRplay RSP1A remains the buy only if upstream
bursts prove to need better dynamic range than a dongle provides —
decide from the first STARVUE capture, not before.)

- **Unblocks:** G5 (stretch) entry.

### P2-2. STARFONE modem-bank fill (G5, stretch)

- The mgetty NAS exists in the estate; fill gaps only: period modems for
  the bank and any IAD2432 line cards short of the box count. Verify
  estate inventory first.

### P2-3. Sacrificial/donor DCT units (W6, W8; F4-5)

- E-waste feedstock purchases *as needed*: additional sacrificial
  DCT2000s for download drills, and donor units **specified by target
  platform** (Platform/Family/Model ID from the census — a DCT2500
  object doesn't run on a DCT2000). Cheap by design; buy when the census
  says what's short, not before.

---

## P3 — Optional / escalation only

### P3-1. GPSDO

- Only if d 02 ever shows carrier drift (F4 escalation path, not a
  default buy). A TCXO-upgraded radio is the first remedy — note the
  LibreSDR (P3-4) already ships a 0.5 ppm-class VCTCXO, which may
  retire this item entirely if one joins the estate.

### P3-2. Extra capture-rig units

- Second camera/capture card when more than one box runs experiments
  concurrently, or for Interconnect hub telemetry (post-G6 pattern).

### P3-3. O1 surplus watch — zero planning weight

- OM-2000 (conformance reference), DAC-6000 (R11 escalation only), RADD,
  TvPC/MCSC cards (side quest), spare **SEM V8 507199-002** (ACP-less;
  clear-tier redundancy, r4 F4-2) or another -001 (full redundancy).
  Logged by the research desk; buy on sight, plan on nothing.

### P3-4. LibreSDR — eventual option (logged 2026-11-22, Marshal's direction)

- **Source listing:** AliExpress item 3256807990830574 (~$120–180 class
  at logging; other sellers list up to ~$260–320 — verify seller,
  returns, and current price before any order).
- **What it is:** PlutoSDR-firmware-compatible clone — AD9363
  transceiver + Zynq 7020, runs the unofficial Pluto firmware port, so
  the project's gr-iio flowgraphs and libiio tooling work unchanged.
- **Why it fits this project:** native **70 MHz–6 GHz** range (the
  whole hunt table with no firmware hack); 0.5 ppm-class VCTCXO +
  external reference option (drift answers P3-1); built-in Gigabit
  Ethernet (network-attached like the SEM — clean fit for the
  containerized deployment and for rack placement away from the host);
  second RF channel accessible for a future TX/RX split.
- **Caveats (bench before trust):** a review observed a TX center (LO)
  spike in DATV testing absent on a genuine Pluto — for our narrowband
  QPSK carrier that spike sits mid-channel, so the P0-1 metering rule
  (dummy load, spectrum + level log) applies in full; seller variance
  and a thinner support trail than genuine Analog hardware; Ethernet
  support varies by software stack (verify gr-iio-over-network on
  arrival).
- **Status:** eventual option, not an immediate buy. Enters through
  one of three doors: (a) the P0-1 fallback set if the Pluto meters
  short and its gr-iio/Ethernet fit is preferred over a HackRF clone;
  (b) a second-radio need where full-duplex + Ethernet beats a dongle;
  (c) the Interconnect hub radio (Appendix A), where its Ethernet
  native placement is the best fit of any radio on this list.

### P3-5. Commercial / purpose-built watch (logged 2026-11-22)

Survey of commercial hardware designed for this job class. **All
zero-planning-weight watch entries** — the SDR path and the SEM already
cover every function below cheaper; these are opportunism, fallback
depth, and benchmarks.

**A. Period GI gear — literally designed for this (extends O1):**

| Unit | Role if found |
| --- | --- |
| OM-1000/2000 | The real OOB modulator — TS in over UDP/IP or RS-530, proper 55-1 carrier out. The ultimate conformance reference for our SDR chain; worth real money to the project if one surfaces cheap |
| RPD-1000/2000 | STARVUE upstream demod — the G5 return path done in period hardware |
| DAC-6000 / RADD-6000 | R11 escalation only (message ground truth if discovery stalls past the M1 timebox) |
| DLS / KLS-1000 / NC-1500 | Completeness; documentation and mating-ops value |

**B. Metrology-grade modulator — DekTec DTA-2115B** (predecessor
DTA-2115, discontinued): all-standard PCIe test modulator, fully agile
**32–2186 MHz** (whole hunt table natively), DVB-S QPSK at
0.088–85 MBd with programmable roll-off, plus an **arbitrary I/Q sample
playback mode** (0.088–85 Msps via StreamXpress/DTAPI) that could take
GNU-Radio-computed 55-1 baseband straight to RF. Lab-grade specs:
−116 dBc phase noise @ 10 kHz, ±0.5 ppm, 10 MHz/1pps sync input, MER
≥42 dB. Price reality: the DTA-2115-GOLD bundle listed at **€6,960**
(no stock). **Verdict: the money-no-object correct instrument and a
terrible purchase against a $0 Pluto** — logged as the signal-quality
benchmark and as a rent/borrow candidate if the SDR chain ever needs an
independent reference transmitter.

**C. Commercial QAM alternatives — SEM-spare / hub class:**

| Unit | Notes |
| --- | --- |
| DekTec DTE-3114 | Standalone **quad QAM-A/B/C** modulator, TS-over-IP in, GigE with IGMP + SNMP, agile 36–1002 MHz — same network-attached shape as the SEM; slots into the r4 F4-2 W4 fallback chain and the hub QAM list |
| Carrier-grade edge QAMs (Vecima, Harmonic NSG class) | MPTS-over-GigE in, many QAMs out; surface on surplus at hobbyist prices occasionally — check Annex B support and licensing state before buying |
| HDMI→QAM gadgets (Blonder Tongue, Thor, Pico) | Pragmatic but period-wrong; already the hub fallback in the Interconnect spec, unchanged |

**D. Receiver-side verification instruments:** cable field meters
(VIAVI/JDSU DSAM class) — QAM MER/BER with constellation views, sweep,
ingress analysis, return QAM generator; some DSAM-era units reportedly
carried forward-data-channel checks for DSG plants — **unverified:
confirm FDC capability against the specific firmware before buying**.
DekTec's professional cable receiver line (54–1002 MHz, J.83 A/B/C,
MER/SNR/BER metrology + Xpect monitoring) is the bench-grade version.
Verdict: convenience instruments for level logs and leakage sweeps; the
RTL-SDR (P2-1) already covers the cheap version of this job.

**E. S-A/Cisco DNCS-era gear:** wrong wire format (55-2 ATM/DSM-CC) —
relevant only to the far-future second front in the S-A ecosystem spec;
not watched.

---

## Appendix A — Interconnect hub kit (post-G6, no schedule weight)

Per the Interconnect spec §3, per hub: SBC (RPi 5 / x86 mini-PC, 8 GB
class), OOB radio in preference order — **LibreSDR** (P3-4; Ethernet-
native placement is ideal for an unattended hub) → the master's retired
Pluto (hand-me-down) → HackRF clone — QAM source in preference order
(surplus SEM 507199-002 → DekTec DTE-3114 or carrier-grade EQAM
[P3-5 C] → HDMI→QAM gadget → GNU Radio QAM on a second SDR), mini F3-7
chain (combiner, tap, attenuator, dummy loads, RG6), optional RTL-SDR +
capture rig for R4 return path, optional smart PDU for remote box
power-cycling. R1 is a loopback bench kit — much of it borrows from the
master bench.

## Appendix B — Estate verification checklist (before any P1/P2 order)

1. SEM bench (E4): SFP cages populated? QAM/UC count matches listing
   (8/4)? Firmware rev + FPK license selectable? First-contact archive
   taken (SEM-EM jar + MIBs → git/git-crypt)?
2. Analog plant: mgetty NAS line count vs fleet; modem inventory.
3. Census (E3): sacrificial unit picked; board rev → flash package →
   P0-4 clip variant confirmed; BDM header present/absent; battery
   states logged; instrumented DCT2500 designated (or absence recorded,
   r3 F3-8).
4. RF bench: chosen primary carrier from census d 02 observations;
   filter ordered for *that* carrier; Pluto level log decides P0-1's
   fallback branch.

*Cross-references: Reproduction Plan §8 (stack + original buy list),
Milestones & Stage Gates §4 (E-tracks) and §6 (tool stack), reviews r3
(F3-2/F3-3/F3-7) and r4 (F4-1/F4-2/F4-3/F4-4/F4-7).*
