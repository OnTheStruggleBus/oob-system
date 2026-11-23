# Capture Rig Specification — mini-headend

**The E1 enabling track: how a sacrificial DCT2000-class box is read by a PC with no human in the loop**

- **Date:** 2026-11-23
- **Status:** Proposed (E1 design). Consolidates and extends the capture-rig
  kit in the Hardware Acquisition List (P0-3). Items marked *open* need a
  bench check before this spec is treated as settled.
- **Parent docs:** `mini-headend Project Overview` (§8 document map),
  `Project Milestones & Stage Gates` (§4 E1 row, §6.3 capture-rig tools),
  `Hardware Acquisition List` (P0-3 capture rig; rev 5 radio note),
  `Management Console Specification` (§4.3 Discovery Lab),
  `ADR-002` (traffic class C5 for lab injection)
- **Direction:** Marshal, 2026-11-23 (E1 scope, signal-source map and open
  items as recorded below).

---

## 1. Purpose

E1 is an enabling track. Its **done** condition feeds **G1 entry**: a
capture rig that produces files into `lab/captures/` for three sources at
once — the front-panel LED, the on-screen d-screens, and the DCT2500 RS-232
console — with OCR accuracy spot-checked against manual d-screen readings.

The rig exists so that **each experiment iteration needs zero human
interaction**: a template goes out, the box's response is recorded from
every surface it exposes, a verdict is written, and one commit records the
run. Without it, the connect-message search depends on a person reading the
front panel after every injection (Hardware Acquisition List P0-3, r3 F3-2).

E1 adds no RF path. The OOB carrier is still produced by the flowgraph into
a terminated, dummy-load-first plant (AGENTS.md §7). The capture rig only
reads the box.

## 2. Signal-source map on the box

| # | Source | What it shows | Electrical tap? | Direct-to-PC path |
| --- | --- | --- | --- | --- |
| (a) | **Front-panel 7-segment LED** | E-codes such as `E 00`, `E 09`, `E 11` | **None.** The only surface with no electrical tap | USB camera → OpenCV + Tesseract (§3.3) |
| (b) | **On-screen d-screens** on the composite video out | Diagnostic screens `d 01`, `d 02`, `d 10`, `d 14`, `d 15`, `d 16` (payload map in §4) | Video output of the box | RCA composite → USB capture card (§3.1) |
| (c) | **RS-232 diagnostic console** (DCT2500) | Console text; the highest-fidelity text source | Serial port | RS-232 → USB-serial adapter (§3.2) |

Notes on the map:

- The d-screens are rendered on video out. The brief's "composite/RF video
  out" is served here by composite. An RF-only path would need a tuner in
  front of the capture card, which is outside the P0-3 kit (open item CR-3).
- The RS-232 console exists on DCT2500s. The Hardware Acquisition List
  records that the fleet includes DCT2500s. The per-box-type connector and
  RAC inventory is open (CR-2), so this spec does not claim the console on
  any other box type.

## 3. Direct-to-PC capture paths

### 3.1 Composite to USB capture card (the d-screens, no camera)

The d-screens are read **direct to the PC via RCA**. A composite-to-USB
capture card is the path. No camera is involved.

- Driver: v4l2 (Milestones §6.3). One frame is grabbed on each screen change
  and on each harness timeout.
- Each frame is stored raw, with a hash, next to the experiment's other
  captures (§6). Text is extracted with the OCR pipeline in §5.
- The Hardware Acquisition List P0-3 entry "Composite/HDMI capture card"
  is the kit item. HDMI is accepted only where the video output is HDMI.

### 3.2 RS-232 to USB-serial (the console, highest fidelity)

The DCT2500 diagnostic console is scraped over **RS-232 to a USB-serial
adapter**. A null-modem cable or adapter is used where the port requires.

- Driver: picocom with expect scripts (Milestones §6.3, the fleet's
  instrumented tier per r3 F3-8).
- The raw byte stream is logged with timestamps. This is the reference
  against which the camera and the capture card are checked when both exist.
- Where the console is present it is the first choice for diagnostic text.
  Where it is absent, §3.1 and §3.3 carry the load.

### 3.3 Front-panel LED to USB camera (the only surface without a tap)

The front-panel 7-segment LED is read by a **USB camera** running OpenCV and
Tesseract. This is the **only** source of the three with no electrical tap,
so the camera is the one dependency the rig cannot avoid unless open item
CR-1 resolves (the serial console mirrors the LED).

- The camera is a fixed-focus, macro-capable unit aimed at the sacrificial
  box's front panel (Hardware Acquisition List P0-3).
- The region of interest is calibrated once per box type and stored with
  the box-type notes, not with the box instance.

## 4. d-screen payload map

Each d-screen carries a payload the harness reads. Field layouts are taken
from the manuals and are **hypothesized** until a spot-check of OCR output
against a manual reading confirms them (E1 exit).

| Screen | Payload read | Used for |
| --- | --- | --- |
| `d 01` | Authorization reason | Canary (`AUTH` check, AGENTS.md §6); enrollment steps |
| `d 02` | Carrier and data lock; `PID 1 received` | G1 core-tier exit: data activity and carrier lock |
| `d 10` | Download module set | Code-download experiments (G4) |
| `d 14` | Connect type; PID and message counters | G1 full-tier exit: connect type logged; per-experiment delta |
| `d 15` / `d 16` | PAT and PMT contents; GI CA descriptors | PSI/SI validation (r4 F4-6); d 15 is the nightly canary scrape |

Any screen that shows a serial number or a Unit Address is a
**box-identifying** capture (§6, git-crypt tier).

## 5. OCR pipeline

1. **Input.** Camera frames of the LED digits (§3.3); capture-card frames of
   the d-screen text (§3.1).
2. **Pre-processing (OpenCV).** Crop to the calibrated region of interest,
   deskew, threshold. Calibration is per box type and is stored with the
   box-type notes.
3. **Recognition (Tesseract).** Trained with a **character whitelist** on
   E-codes (`E 00`, `E 09`, `E 11`, and the rest of the manual set) and on
   d-screen digits. Free text is not recognized. A whitelist keeps a
   misread from producing a plausible wrong code.
4. **Output.** One structured record per frame: source, timestamp, frame
   hash, recognized string, and per-field confidence. The record is the
   machine-readable form; the frame is the evidence.
5. **Acceptance.** At E1 exit, OCR output is spot-checked against manual
   d-screen readings. The sample size and method are set at E1 exit (open
   item CR-4). A claim about box state is made only from the raw capture,
   never from the OCR record alone (AGENTS.md §2).

## 6. Harness contract

**Flow.** A message template goes out through the SDR, the box responds, and
the three capture paths record the response. The flow is:

```
message template (oob-msg, status + template hash)
   -> class C5 injection through the command layer (ADR-002 §7)
   -> pacer -> flowgraph -> SDR (chosen at the G0.5 bench)
   -> box
   -> capture paths: (a) LED camera, (b) d-screen capture card, (c) RS-232 console
   -> lab/captures/<experiment>/  (raw frames, serial log, OCR records, manifest)
   -> verdict -> one commit
```

**Files.** Each experiment writes one directory under `lab/captures/`. It
holds the raw frames, the serial log, the OCR records, and a manifest that
names the template hash, the box, and the timestamps.

**Encryption tier.** A capture that shows a serial number, a Unit Address,
or any other box-identifying value goes in the **git-crypt tier** (AGENTS.md
§5). The first box captures land encrypted (Milestones G1 entry). Nothing
identifying is committed in plaintext.

**Commit rule.** One commit per experiment, with the shape *message template
hash → box response (OSD state and scraped diagnostics) → verdict*. The
commit uses the `lab(exp):` prefix (AGENTS.md §6). The protocol document
grows from these commits, not from notes.

**Reset actuator.** Zero interaction also needs a way for the harness to
power-cycle the sacrificial box. The optional smart PDU in the Hardware
Acquisition List is the candidate. Without an actuator the rig still
captures, but a person must reset the box, and the zero-interaction purpose
is not met (open item CR-4 covers the choice).

**Safety.** The rig adds no RF path and no destructive box operation. An
experiment that does involve a destructive operation (cold initialize, code
download, flash write) runs on the sacrificial box only, with typed
confirmation from Marshal (AGENTS.md §7).

## 7. Open items

- **CR-1 (bench-verify).** Does the DCT2500 RS-232 console mirror the
  front-panel LED codes? If it does, the camera dependency in §3.3 can be
  removed for those codes. Check on the sacrificial box before the camera
  is treated as required for every code.
- **CR-2.** Per-box-type connector and RAC inventory: which box types expose
  composite video out, an RS-232 console, and which connector and RAC
  variant they use. The fleet census (E3) feeds this.
- **CR-3.** An RF-only video path for any box without composite out. This is
  not in the P0-3 kit and needs a tuner in front of the capture card.
- **CR-4.** The OCR spot-check method and sample size, and the choice of
  reset actuator for the harness (§6). Both are set at E1 exit.
