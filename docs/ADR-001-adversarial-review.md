# ADR-001 rev 1 — Adversarial Review

- **Date:** 2026-11-21
- **Reviewer:** adversarial pass, requested by Marshal
- **Method:** line-by-line challenge of every claim in ADR-001 rev 1, plus an independent deep-research round (SCTE/CableLabs primary documents, DCT2000 installation manual, DAC-6000 provisioning documentation, OM-2000 product manual, patent and prior-art searches).
- **Verdict:** 15 findings — 2 critical, 8 major, 5 minor. The architecture survives review; two factual errors and one under-analyzed critical risk need correction. Recommended dispositions: 13 × ACCEPT, 2 × ACCEPT-WITH-MODIFICATION.

---

## F1 — CRITICAL — Option B (surplus OM-2000) is mischaracterized

**Rev 1 claim:** the OM-2000's "input side expects DAC/RADD-era source links," so a software DAC is still required and the OM only replaces the modulator stage.

**Evidence against:** the OM-2000 product manual states it "can be configured to receive data over Ethernet using UDP/IP, or as serial data over RS-530 or RS-232 communications links." Its Ethernet output side also emits a null-stripped MPEG-TS (per CM-SP-R-OOB), and the downstream QPSK modulator is required to insert nulls itself.

**Impact:** Option B is *more* viable than rev 1 says. A surplus OM accepts a plain UDP/IP TS feed from rust-dac and produces spec-grade RF. That changes it from "reference unit only" to a legitimate plug-compatible fallback for the modulator stage. The core conclusion (software DAC is unavoidable; SDR first) still stands, but the reasoning was wrong and the fallback story is stronger than documented.

**Disposition:** ACCEPT — rewrite §3 Option B; upgrade OM-2000 to "conformance reference AND fallback modulator."

## F2 — CRITICAL — R1 (EMM crypto) is framed around the wrong key-establishment model

**Rev 1 claim:** without the boxes' per-unit keys from a DAC database, authorization is impossible; mitigation is BDM key extraction.

**Evidence against:** the DCT2000 manual's auth-reason codes and d 08 diagnostic describe the actual security model — per-unit *seed* material ("20 – bad seed checksum"), working-key epoch messages, program/category re-keys, and a renewable security element (TvPC) with group key (GK) and unit keys. Separately, the DAC-6000's provisioning interface includes an explicit **INIT_SETTOP** operation and an INIT_FLAG — i.e., headend-driven (re)initialization of set-tops is a *documented, standard* flow. Real headends routinely initialize field boxes; the install-time key-establishment exchange is the mechanism by which a DAC comes to know a box's keys in the first place.

**Impact:** the primary path for R1 should not be "extract keys via BDM" but "understand and reproduce the documented install-time initialization/key-establishment flow so boxes are initialized into a *known-key state* under our DAC." BDM extraction demotes to fallback. This is materially more tractable and better grounded — but it is still unproven, so R1 remains the gating risk.

**Disposition:** ACCEPT — rewrite §7 R1; reorder mitigations; add research item "install-time INIT/key-establishment flow."

## F3 — MAJOR — OOB carrier frequency set is incomplete

**Rev 1 claim:** OOB at 75.25 MHz, "alternate 104.2 MHz."

**Evidence against:** the DCT2000 specification lists the data carrier as **75.250 MHz or 72.75 MHz or 104.2 MHz**, QPSK, 1.5 MHz bandwidth, −10 to +5 dBmV. Fleet heterogeneity is real: e.g., DCT2244 listings document a 72.75 MHz OOB variant. The d 02 diagnostic supports operator selection of the OOB frequency on agile boxes.

**Impact:** assuming 75.25 MHz fleet-wide will strand any 72.75 MHz-configured unit. The box census (O2) must record each unit's configured OOB frequency, and rust-dac must support re-pointing boxes (or we run the carrier where the fleet majority already listens).

**Disposition:** ACCEPT.

## F4 — MAJOR — PHY parameters were asserted without the normative spec

**Rev 1 claim:** QPSK, 2.048 Mbps, "root-raised-cosine per 55-1" — but symbol rate, rolloff, differential encoding, randomization, and framing were never specified, and the project does not yet possess ANSI/SCTE 55-1 (2009), the normative reference named by CableLabs.

**Impact:** a GNU Radio flowgraph built on vibes risks a carrier the box's demod rejects despite "carrier lock" heuristics (the manual warns the lock indicator "can falsely indicate locked"). Symbol rate is at least derivable (2.048 Mbps ÷ 2 bits/symbol = 1.024 Msym/s; with typical rolloff ≈ 1.3–1.5 MHz occupied, consistent with the manual's 1.5 MHz), but coding/framing details must come from the spec or from capture analysis.

**Disposition:** ACCEPT — add milestone M0.5 (acquire SCTE 55-1; PHY conformance test against a real box's d 02 data-activity indicators, not just carrier lock).

## F5 — MAJOR — 55-1 upstream model was wrong-ish

**Rev 1 claim:** RF upstream is an "Aloha-ish MAC," phased late.

**Evidence:** CM-SP-R-OOB states the SCTE 55-1 system "does not include such upstream scheduling capabilities (and timing/jitter constraints), however requires multiple upstream frequencies to operate" — i.e., *unscheduled* contention access on multiple upstream frequencies (one 3.2 MHz channel or three 192 kHz channels in 5–42 MHz), unlike 55-2's scheduled TDMA. The STARVUE II diagnostic confirms per-box upstream frequency (e.g., 9.0 MHz) and transmit level in dBmV.

**Impact:** mostly a wording fix, but it lowers M6 difficulty (no slot scheduler to build; the hard part is demodulating contention bursts) and adds a config requirement (upstream frequency table — the d 14 MAC FREQUENCY TABLE exists for exactly this).

**Disposition:** ACCEPT.

## F6 — MAJOR — No-PCR/asynchronous OOB is a missed simplification

**Evidence:** CM-SP-R-OOB: "There are no PCR time stamps in any of the SCTE-55-1 streams; all MPEG streams coming out of the OM are asynchronous," and the modulator may insert NULL packets wherever necessary.

**Impact:** rust-dac's TS mux needs no PCR generation and no precise inter-packet timing; pacing can be loose, which de-risks the GNU Radio chain significantly. Rev 1 never said this and implied tighter mux discipline than required.

**Disposition:** ACCEPT — state it explicitly; it shrinks M1.

## F7 — MAJOR — STARFONE return path: feasibility confirmed, protocol risk understated

**Evidence:** the STARFONE diagnostic shows modem types up to 14400 baud, connect baud (e.g., 300), data format, two headend-provisioned phone numbers, a parameters-valid flag, and hang-up codes including "Communication protocol fault."

**Impact:** M5's core assumption — we can provision the dial number from the headend and point boxes at our own NAS — is confirmed by the manual. But answering the call with mgetty is the easy half; the GI polling/transaction protocol spoken over that modem link is undocumented, and the box will hang up with protocol-fault codes until we speak it. This is a second reverse-engineering surface that rev 1 treated as near-free.

**Disposition:** ACCEPT-WITH-MODIFICATION — M5 stays, but add sub-risk and an explicit protocol-RE task; consider sniffing the box's outbound call on a simulated line to bootstrap the protocol.

## F8 — MAJOR — "CDL" is a named, semi-documented thing; rev 1 treated it as pure unknown

**Evidence:** CableLabs nomenclature expands CDL as **Common Download**; SCTE 28 documents the code-download *locator* mechanism (Code Version Tables pointing at a frequency/PID data carousel) for the POD era; the DCT2000 d 10 diagnostic documents box-visible behavior (segment zero → packet countdown; module states DOWNLD/ENABLED/DISABLD).

**Impact:** the code-download problem decomposes into (a) a documented *discovery* layer (CVT-style tables) and (b) a proprietary GI *segment* layer. Not a green field — two document leads exist that rev 1 missed.

**Disposition:** ACCEPT — add both to the research plan and references.

## F9 — MAJOR — No content/lineup consistency strategy

**Rev 1 gap:** HDMI→QAM modulators appear with no playout design, and nothing enforces consistency between the DAC's virtual channel map and the actual RF lineup. The DCT2000 has no useful in-band self-discovery for this purpose — the DAC channel map is authoritative, so a mismatch strands channels.

**Impact:** operational bug factory. Fix: one `lineup.yaml` in the repo as the single source of truth, generating both the rust-dac channel map and the modulator playout config; CI check that the two agree.

**Disposition:** ACCEPT.

## F10 — MAJOR — Compliance section is thinner than the project's own standards

**Gaps:** (a) Part 15 is mentioned, but retransmission hygiene is not — all in-band content must be self-generated or rights-free, not re-modulated commercial channels; (b) EAS emulation must be unmistakably labeled as test traffic (SCTE 18 tones are designed to command attention); (c) any recovered key material becomes a sensitive secret (git-crypt discipline is already planned — extend it to box dumps, which may contain unit keys).

**Disposition:** ACCEPT.

## F11 — MINOR — DAC operational model documented elsewhere; use it as our API shape

**Evidence:** the Oracle ASAP DAC-6000 cartridge documents the DAC's real operational verbs: ADD_SETTOP (unit address, serial, VCM handle, EPG region, timezone, credit in GI Units, turn-on/turn-off channels, upstream/downstream plant handles), INIT_SETTOP, MODIFY_SETTOP_FEAT (country/currency/output channel), CLEAR_ALL_PACKAGES/SERVICES/PROGRAMS.

**Impact:** rust-dac's admin API should mirror these verbs and its unit database these fields — free, battle-tested data model instead of inventing one.

**Disposition:** ACCEPT.

## F12 — MINOR — Time service is missing from the message catalog

**Evidence:** the front panel displays time of day; DAC provisioning includes a per-box TIMEZONE handle. Headend time distribution is a real DAC message our catalog omits.

**Disposition:** ACCEPT — add to catalog; trivial to implement, annoying to discover late.

## F13 — MINOR — Firmware-version heterogeneity affects diagnostics and behavior

**Evidence:** d 13 (interactive info, incl. IP address/UPM) and d 14 (MAC frequency table) exist only on firmware 06.x; d 06 reports firmware version; d 01 reports Platform/Family/Model IDs.

**Impact:** census (O2) must capture firmware rev per unit; assumptions about available diagnostics and interactive features are per-rev.

**Disposition:** ACCEPT — fold into O2.

## F14 — MINOR — Level plan needs explicit numbers, not "box-friendly"

**Evidence:** box input ranges: data carrier −10 to +5 dBmV, average digital in-band −10 to +5 dBmV; R-OOB practice runs the FDC at 0 to −7 dBc relative to 256-QAM carriers.

**Impact:** rev 1's "0 ± 5 dBmV at the tap" is workable but should be a designed budget: in-band QAM at ≈ 0 dBmV, OOB at ≈ −5 dBc relative (−5 dBmV), both comfortably mid-range, with per-tap attenuation math in the runbook.

**Disposition:** ACCEPT.

## F15 — MINOR — No kill criteria

**Gap:** "full parity" has a known unknown at its center (R1) and no defined abandonment rule.

**Fix:** define it now — if the install-time key-establishment research (F2) fails after a bounded effort, the project definition-of-done falls back to M2 (clear-tier fleet) + M5 (STARFONE), with encrypted authorization and code download declared out of scope rather than silently slipping.

**Disposition:** ACCEPT-WITH-MODIFICATION (wording of the fallback is Marshal's call).

---

## Research log (what was checked, what was found, what wasn't)

**Found:**
- OM-2000 input interfaces (UDP/IP, RS-530, RS-232) — product manual (tvcinc.com mirror).
- DCT2000 full diagnostic suite, error codes, unit-address format (40-bit), STARFONE/STARVUE II parameters, TvPC security model, auth-reason table, d 10 download behavior — DCT2000 Installation Manual.
- DAC-6000 provisioning verbs and unit data model — Oracle ASAP Motorola DAC-6000 Cartridge Guide.
- 55-1 system architecture, OM as multiplexer, no-PCR/asynchronous streams, upstream model, ARPD upstream interface — CM-SP-R-OOB-I03-160512 (CableLabs).
- CDL = Common Download; CVT locator mechanism — SCTE 28.

**Not found (honest dead ends):**
- No public hobbyist DAC/headend emulator for the GI platform. No GitHub project, no forum build log. This project appears to be first-of-kind in the open — raises both the value and the risk.
- No public documentation of the GI DAC→box message wire format, the code-download segment format, or the STARFONE polling protocol.
- Patent searches for the GI download/authorization mechanisms returned nothing directly usable in this round; a targeted patents.google.com pass (assignee: General Instrument, 1993–1999, set-top download/security) is queued as an open item.
- Period OOB captures: still unsourced (O1 stands).
