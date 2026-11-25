# Encoding & Transport Specification — mini-headend

*Status: proposed, 2026-11-24 (strategy session, Marshal). This specification
sets the encode and transport profile for the clear-tier playout chain
(W4) and for the encrypted tier that reuses it (W5). It does not disposition
any gate. Gate placement is in §11 and is for Marshal to decide.*

**Trust labels used here.** Every statement carries one of these:

- **documented (public standard)** — taken from a published standard
  (ISO/IEC 13818-1 and -2, ANSI/SCTE 35, CEA-608, J.83 Annex B). The text is
  not yet in the repository; acquiring it is an open item (§12).
- **documented (repo)** — taken from a document already in this repository,
  cited by section.
- **lab default** — a choice this project makes. It is not a claim about
  any box or any headend.
- **hypothesized** — a claim about box behavior that the box has not yet
  confirmed. Only a capture under `lab/captures/` can move it on
  (AGENTS.md §2).

Nothing in this document is **verified**. No box-visible behavior is claimed.

## 1. Scope

The chain from a self-authored source file to the QAM modulator's input.
Every setting in it is an encode-profile or service row in oob-dac's store,
edited in the Console (Programming & Ad Management Specification §2), never in
a file:

1. **Ingest and transcode** — decode the source, then encode it to the
   profile in §2 (ffmpeg, `oob-playout`).
2. **Program stream** — elementary streams multiplexed into MPEG-2 transport
   streams with PAT, PMT, the DCII SI tables and the PID plan (`oob-mux`).
3. **Splice signaling** — SCTE 35 cues placed at the GOP boundaries that §5
   requires (`oob-playout`, driven by the schedule DB; see the Programming &
   Ad Management Specification).
4. **Transport and pacing** — constant-bit-rate transport packing,
   null-stuffing and PCR-accurate delivery to the SEM over UDP/GigE (tsp, per
   the Plan's W4 row).
5. **Verification** — the lab-side analyzer checks in §10.

Out of scope: H.264, AAC, HD, progressive video, adaptive bitrate, and any
encoding of real broadcaster content. The Plan's codec pin (§8) forbids the
first two for the fleet, and §2 explains why.

## 2. Video profile

| Parameter | Value | Label |
| --- | --- | --- |
| Codec | MPEG-2 video, Main Profile at Main Level (MP@ML) | documented (repo, Plan §8 codec pin; ISO/IEC 13818-2) |
| Raster | 720 × 480 luma, 4:2:0 chroma | documented (repo, Plan §8) |
| Frame rate and scan | 29.97 frames/s, interlaced (59.94 fields/s, "480i") | documented (repo, Plan §8) |
| Field order | Top field first for 480i | lab default (the period NTSC convention; confirm against the source material) |
| Aspect | 4:3 by default. 16:9 content is letterboxed inside 4:3 | lab default. The fleet's 16:9 handling is **hypothesized** until the census (§12) |
| Bitrate control | Constant bitrate per service, with VBV set so the stream stays inside the DCII transport budget (§4) | documented (repo, Plan §8: CBR transport) |
| Maximum bitrate | 15 Mb/s (MP@ML ceiling); the lab ceiling is far below it (§4) | documented (public standard) |
| VBV buffer | 1,835,008 bits at the MP@ML maximum; the lab sets a smaller VBV that matches its CBR target | documented (public standard for the maximum); the lab value is a lab default |

**Why 480i, not progressive.** The fleet decodes nothing newer. The repo
pins MPEG-2 MP@ML 480i for that reason (Plan §8; census in §12). Encoding
progressive, or a higher level, is a box-compatibility failure, and a
failure of that kind looks like a tuning or PSI fault (Plan §8). That is
the reason the profile is pinned, not a matter of taste.

## 3. Audio profile

| Parameter | Value | Label |
| --- | --- | --- |
| Primary codec | MPEG-1 Layer II (MP2), 48 kHz, stereo | documented (repo, Plan §8 audio pin, F5-7) |
| Secondary codec | AC-3 (Dolby Digital), 48 kHz, only where the census confirms decoder support | documented (repo, Plan §8). The fleet's support is **hypothesized** until d-screen census |
| MP2 bitrate | 192 kb/s stereo | lab default (the fleet's MP2 range is set in the census) |
| AC-3 bitrate | 192–384 kb/s; 384 kb/s is the usual 5.1 rate | documented (repo, F3-6 range); the 5.1 rate is a public-standard practice |
| Second audio (SAP-style) | Not carried in v1 | lab default. Whether period headends carried one is hypothesized |

Audio is timed against video through the PTS on both elementary streams.
The lab target is that a splice (§5) moves picture and sound together. The
target is a lab default. Measured lip-sync is a lab-side check (§10), not a
box claim.

## 4. Transport budget

The budget is set by the modulation, not by the encoder.

| Constellation | Usable payload (J.83 Annex B) | Label |
| --- | --- | --- |
| 256-QAM | ≈ 38.8 Mb/s | documented (repo, Plan §8 and F3-6) |
| 64-QAM | ≈ 26.97 Mb/s | documented (public standard, J.83 Annex B) |

Per-service budget on 256-QAM, lab default:

| Component | Rate | Notes |
| --- | --- | --- |
| Video (CBR target) | 3.5 Mb/s | The repo's per-service range is 3–6 Mb/s (Remote spec, the bitrate paragraph). 3.5 Mb/s is the lab target. |
| Audio (MP2) | 0.192 Mb/s | §3 |
| PSI, SI, SCTE 35 PID | ≈ 0.05 Mb/s per service | lab estimate. Measured by the analyzer (§10) |
| Per-service total | ≈ 3.75 Mb/s | sum of the above |

At 3.75 Mb/s per service, 256-QAM carries about **10 services** (10 × 3.75 =
37.5 Mb/s) with about 1.3 Mb/s of headroom for null packets and the table
PIDs. At 64-QAM, about **7 services** fit (7 × 3.75 = 26.25 Mb/s). Barker
and EPG channels run at a lower CBR target (Plan §8), so the counts above
are approximate. The count is a lab planning figure. It is not a box claim.

**The OOB stream is a different thing.** The out-of-band channel is QPSK at
1.024 Msym/s (2.048 Mb/s), asynchronous and null-stuffed, with about 1000 kb/s
for DAC traffic (ADR-002; repo facts registry). It carries no PCR, and its
budget is not the program-TS budget above. The two must not be merged in any
document.

## 5. Splice structure and GOP

This is the detail that decides whether an ad splice is clean.

- **Closed GOP.** Every splice point starts a closed GOP, so the first
  frame after the splice does not depend on pictures from before it. Period
  headend encoders are described with closed-GOP operation. The reason given
  here (decode after a splice) is a lab engineering choice, not a sourced
  claim. One documented example is the Sci-Atlanta spec §2 encoder D9032,
  described with closed-GOP statistical
  multiplexing.
- **GOP cadence.** N = 15, M = 3, which is the usual 480i broadcast pattern
  (I B B P B B P B B P B B P B B). **Lab default.** The cadence is a choice
  for the splice points, not a fleet requirement.
- **Cue placement.** An SCTE 35 cue is placed on the frame that opens the
  GOP at the splice in-point. A splice at any other frame is expected to
  produce a decoder artifact (**hypothesized** for the fleet decoder). The lab
  treats it as a failed splice (§10).
- **No B-frame leakage.** The first GOP after a splice starts with an I-frame
  and carries no references to the previous program.

## 6. Timing: PCR, PTS and PSI repetition

| Item | Requirement | Label |
| --- | --- | --- |
| PCR interval | At most 100 ms between PCR packets on the PCR PID | documented (public standard, ISO/IEC 13818-1) |
| PCR accuracy | Within ±500 ns of the nominal clock | documented (public standard, ISO/IEC 13818-1) |
| PAT and PMT repetition | Lab target: at most 500 ms. The 100 ms figure is the PCR interval, not a PAT or PMT rule | lab default, to confirm against ISO/IEC 13818-1 when acquired. The 500 ms target follows public DVB measurement practice (ETSI TR 101 290), not a repo document |
| PTS | Present on every access unit; repeated at least every 700 ms | documented (public standard, ISO/IEC 13818-1), to confirm against the text |
| Lab pacing target | PCR jitter inside the ±500 ns budget at the SEM input | lab default. Measured on the analyzer (§10) |

The program TS has PCR. The OOB stream does not (ADR-001 §1, Context). Anyone who copies the "no PCR" fact from the OOB context into the program
context is making an error.

## 7. Program stream tables

- **PAT and PMT** — standard MPEG-2. The PMT lists each elementary stream and
  its `stream_type`.
- **DCII SI** — not DVB SI. The master guide table on PID 8187, virtual
  channel numbers, and the CA descriptors that the box reads (documented
  repo, Plan §8, F4-6/F5-6). The table layouts are **hypothesized** until
  they are confirmed through d 14 and d 15/d 16 on a box (§12).
- **SCTE 35 PID** — carried in the PMT with `stream_type` 0x86 (documented
  public standard, ANSI/SCTE 35). The box's reaction to this stream type is
  **hypothesized**. The lab expects the box to skip it; a capture must show that.
- **Null packets** — PID 0x1FFF. Used to fill the CBR budget (§4).
- **Continuity counters** — increments by one on each packet that carries payload, on each PID, with the
  public-standard exceptions (ISO/IEC 13818-1, for adaptation-only packets and
  discontinuity indicators). The analyzer checks them (§10).

## 8. Captions and user data

- **Closed captions.** NTSC 480i captions are CEA-608 (line 21, field 1 and
  field 2), carried as `cc_data` inside MPEG-2 picture user data under the
  GA94 identifier (documented public standard, CEA-608 and the A/53 practice).
- **CEA-708** (digital captions) is not carried in v1. DCT2000-class boxes
  are not expected to decode it. **Hypothesized.**
- **AFD** (active format description) user data is optional. It is not
  carried in v1 unless the census shows the fleet uses it (§12).
- **Ratings** (TV Parental Guidelines and similar) travel in the listings,
  not in the program stream. The listings path is the W8 guide chapter
  (Plan §4.1), and the program stream does not carry rating metadata in v1.

## 9. Asset transcode pipeline (lab)

The **Asset** entity in the schedule DB (Plan, W4 amendment) is the output of
one transcode. The pipeline is:

1. **Source.** Self-authored or rights-free material only (ADR-001 content
   rule). No broadcaster content.
2. **Probe.** Record the source's frame rate, scan, field order and audio
   layout, and reject anything outside §2–§3 before encoding.
3. **Encode.** ffmpeg MPEG-2 encode with the §2–§5 settings. The settings
   come from an **encode profile** row in oob-dac's store, created and edited
   in the Console and never in a file (Programming & Ad Management
   Specification §2). The shipped profiles are seed rows, labelled as lab
   defaults. Every asset records the profile revision it was built with.
4. **Package.** The elementary streams go to `oob-mux` for PSI and PID
   assignment (§7).
5. **Record.** The asset row stores the codec parameters, the measured
   bitrate, the duration, and the splice-point list. The listings synthesizer
   reads the metadata from this row (Plan, W4 amendment, entity **Asset**).

**Validation on save.** oob-dac validates every encode profile when an
operator saves it. It refuses any value the fleet cannot decode, with the
reason shown in the Console: a codec other than MPEG-2 video and MP2 or AC-3
audio, a level other than MP@ML, a progressive scan, or a rate above the §4
budget. These refusals are server-side and apply to every client, including
the API (ADR-003 §5). The CI lint (AGENTS.md §8: ffmpeg lint in CI) checks the
shipped seed profiles against the same rules.

## 10. Verification (lab-side, not box evidence)

These checks run on the lab host. They do not claim anything about a box. A
box-visible claim still needs a capture (AGENTS.md §2).

| Check | Pass condition | Tool |
| --- | --- | --- |
| Profile conformance | MP@ML, 480i, 4:2:0, MP2 or AC-3 only | ffprobe / the CI lint (§9) |
| GOP cadence at splices | Every splice frame is a closed-GOP I-frame | tsduck analyzer on the splice list |
| PCR interval | ≤ 100 ms on every PCR PID | tsanalyze (tsduck) |
| PCR accuracy | Within ±500 ns at the SEM input | tsp pacing logs; lab-side measurement only |
| PAT and PMT repetition | ≤ 500 ms lab target (§6) | tsanalyze |
| Continuity counters | No unexplained gaps | tsanalyze |
| Bitrate per service | Within the §4 budget | tsanalyze PID bitrate |
| Lip sync across a splice | Picture and sound within the lab target | lab measurement; the target is a lab default |

A failed check blocks the asset or the mux configuration. A passed check is
evidence about the stream, not about the box.

## 11. Gate placement (PROPOSED — Marshal dispositions)

- **W4 (M2, clear tier).** The §2–§7 profile is the W4 codec pin. The decode
  smoke test on the oldest fleet box in the M2 exit already covers it.
- **Splice cues (§5, §7).** Proposed for W4, because a clear-tier channel with
  breaks needs them.
- **Captions (§8).** Not in any gate exit. Proposed as a backlog item until a
  box capture shows that a fleet box displays them.
- **Encrypted tier (W5, M3).** The encrypted services use the same profile
  (§2–§7). The scrambler sits after the mux and does not change the profile.

No gate is passed, claimed or dispositioned here.

## 12. Open items

- **Acquire the standards text** — ISO/IEC 13818-1 and -2 (PCR, PAT/PMT
  repetition, the MP@ML limits), ANSI/SCTE 35 (stream type 0x86, cue syntax),
  CEA-608 — as an open item, handled like O7. Until then every "documented
  (public standard)" label above is a claim from memory and must be checked
  against the text.
- **Census.** The census of the fleet decides the AC-3 and 16:9 questions,
  the AFD question, and whether any box displays CEA-708. Each is
  **hypothesized** until then.
- **Box-side test list.** Captures needed before any box-visible claim:
  (a) a spliced stream decodes with no visible artifact at the splice,
  captured on the E1 rig; (b) the PMT with a 0x86 stream does not stop
  tuning on the oldest box; (c) CEA-608 captions display, if they are carried.

## 13. Cross-references

- Plan §8 (codec pin, DCII SI, ~1000 kb/s insertion budget) and the W4 row.
- Plan W4 amendment (the schedule DB; the **Asset** entity).
- Programming & Ad Management Specification (splice cues come from the
  schedule DB and the avail model).
- Remote Service Delivery Specification (the 3–6 Mb/s per-service range, and
  the hub's transport over SRT).
- Sci-Atlanta Ecosystem Specification §2 (the encoder and DCM line as a
  documented period reference).
- ADR-001 (the OOB stream rate and the no-PCR fact for the OOB context).
- ADR-002 (the insertion budget and the pacer).
