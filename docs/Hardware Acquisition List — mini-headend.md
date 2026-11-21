# Hardware Acquisition List — mini-headend

*2026-11-22, rev 2. Consolidates the Reproduction Plan §8 buy list, the
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
disposition alongside r4.

**Already in the estate (no purchase):** Motorola SEM V8 P/N
507199-001-00 (ACP confirmed — QAM + FPK encryption), ADALM-Pluto (OOB
TX candidate and upstream monitor), the DCT fleet (DCT2000 + DCT2500 +
DVR models), Debian 13 host, analog plant (IAD2432 + Asterisk + mgetty
NAS) for STARFONE.

---

## P0 — Buy now (unblocks G0.5 and G1)

### P0-1. OOB radio — Pluto first ($0), HackRF *clone* as metered fallback

**Cost-optimized plan (rev 2):**

1. **ADALM-Pluto (in estate, $0).** Apply the AD9364-identity range
   hack (required at *every* hunt carrier, r4 F4-1), then meter actual
   delivered level at the chosen carrier through the full F3-7 chain
   into a dummy load. If the budget closes to the −10…+5 dBmV window at
   the box input with the mandatory bandpass filter in-line (the filter
   is already in P0-2 and cleans the Pluto's sub-150 MHz spurs), the
   Pluto is the OOB TX and no radio purchase happens at all.
2. **Fallback: HackRF One *clone* (~$105–150), not a genuine unit
   (~$340).** rtl-sdr.com's clone-vs-original bench comparison found the
   $339.95 genuine vs $105 clone gap hard to justify; for a
   constant-envelope 1.024 Msym/s QPSK carrier into a filter +
   attenuator, clone phase noise is not the bottleneck. Buy a bare
   clone board (no PortaPack) from a listing with returns; verify
   output on the meter before it enters the chain. Covers the whole
   hunt table (75.25–116.25 MHz) in spec, no hack; doubles as the
   STARVUE upstream receiver later if the Pluto stays on TX.
3. **Decision point:** G0.5 exit records which radio committed, with
   the level log as evidence (unchanged from r4 F4-1's metering
   requirement — only the preferred order changes).

- **Unblocks:** G0.5 exit (OOB TX committed), G1 entry.

### P0-2. F3-7 RF chain (r3 F3-7)

The chain between SDR and plant, bought as one kit:

| Item | Spec / notes |
| --- | --- |
| Bandpass filter | For the chosen OOB carrier (104.2/110.25 class, decided at census); also suppresses Pluto spurs (r4 F4-8: FM-band adjacency makes this mandatory, not optional) |
| Small gain block / PA | Sized after metering radio output through the chain — buy only if the level budget doesn't close (r4 F4-1 delta) |
| Variable/step attenuators | 0–30 dB class, to land ≈0 dBmV at the box input (window −10…+5 dBmV) |
| 2-way combiner | Merges OOB carrier with SEM QAM output |
| Directional tap / coupler | Plant feed + measurement port |
| 75 Ω dummy loads / terminators | Several — dummy-load-first is doctrine (closed-plant rule) |
| RG6 + F-connectors + tools | Compression fittings preferred; shielded passives throughout |
| 2-way splitter | Census bench (feed one box at a time off the tap) |

- **Unblocks:** G0.5 exit (level log through the chain into a dummy load).

### P0-3. Capture rig (E1, r3 F3-2 — "highest-leverage purchase in the plan")

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
  sufficient.

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
   bench (r4 F4-1) — evaluate before buying anything.

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
  default buy). A TCXO-upgraded radio is the first remedy.

### P3-2. Extra capture-rig units

- Second camera/capture card when more than one box runs experiments
  concurrently, or for Interconnect hub telemetry (post-G6 pattern).

### P3-3. O1 surplus watch — zero planning weight

- OM-2000 (conformance reference), DAC-6000 (R11 escalation only), RADD,
  TvPC/MCSC cards (side quest), spare **SEM V8 507199-002** (ACP-less;
  clear-tier redundancy, r4 F4-2) or another -001 (full redundancy).
  Logged by the research desk; buy on sight, plan on nothing.

---

## Appendix A — Interconnect hub kit (post-G6, no schedule weight)

Per the Interconnect spec §3, per hub: SBC (RPi 5 / x86 mini-PC, 8 GB
class), HackRF-class radio *or* a recycled Pluto (the master's retired
OOB radio is the natural hand-me-down), QAM source in preference order
(surplus SEM 507199-002 → HDMI→QAM gadget → GNU Radio QAM on a second
SDR), mini F3-7 chain (combiner, tap, attenuator, dummy loads, RG6),
optional RTL-SDR + capture rig for R4 return path, optional smart PDU
for remote box power-cycling. R1 is a loopback bench kit — much of it
borrows from the master bench.

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
