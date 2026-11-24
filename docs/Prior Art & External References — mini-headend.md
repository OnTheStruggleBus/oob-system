# Prior Art & External References — mini-headend

2026-11-22. Internet-wide scan for prior attempts at any part of this project: 55-1 OOB generation, DAC/message-layer reimplementation, DCT enrollment without a cable company, KLS/DLS code download, FPK/MediaCipher EMM work, resident-guide feeding, and adjacent hobby scenes. Sources are linked inline; assessments are ours.

## 1. Bottom line

Nobody has publicly tried the whole thing, and nobody has publicly done any single one of the hard parts for the GI/Motorola (Mode A) ecosystem. The public record clusters into six buckets: one genuinely close technical relative (for the other ecosystem), one canonical CA reverse-engineering effort, vendor-side corroboration of our architecture map, box-side hobby documentation, adjacent SDR/broadcast scenes, and the surrounding preservation world. §5 lists the gap that remains — which is, precisely, this project.

## 2. Direct technical prior art

### 2.1 oobscte55 — the closest living relative (SCTE 55-2, receive side)

<https://sarahsforge.dev/projects/oobscte55> — a cleaned-up, documented toolchain that decodes a SCTE 55-2 (DVS-178 / DAVIC "Mode B") OOB forward data channel from a raw SDR++ IQ recording all the way to the EPG channel lineup: QPSK demod → differential decode → derandomize → SL-ESF sync → deinterleave → RS(55,53) → ATM cells → AAL5/IP/UDP → DSM-CC carousel → BIOP objects → lineup + strings. The signal was a mystery WAV off a Charter/Spectrum (Scientific-Atlanta/Cisco) plant.

Relevance to Oob: proof that the OOB channel is tractable with hobbyist SDR and ordinary compute, and a module-by-module pipeline template for our PHY work and for oob-msg's receive-side verification harness. It is the other ecosystem — Mode B (ATM/DSM-CC) where we are Mode A (GI proprietary message layer) — so it is a methodology donor, not a protocol source. Also a research-desk contact candidate (§6).

### 2.2 Chris Gerlinsky — "How Do I Crack Satellite and Cable Pay TV?" (33c3, 2016-12-27)

<https://media.ccc.de/v/33c3-8127-how_do_i_crack_satellite_and_cable_pay_tv> — the canonical reverse engineering of the DigiCipher II conditional access: from circuit board to chemical decapsulation, optical ROM extraction, glitching, and reversing custom hardware cryptographic features of a system that stayed secure 15+ years in the field. Wikipedia's DigiCipher 2 article cites this talk for "reverse engineered in 2016" (<https://en.wikipedia.org/wiki/DigiCipher_2>).

Relevance: DCII/MediaCipher is the family our SEM's FPK module and the DCT security silicon implement. This talk is the deep-end escalation path for D0/O13 and the best map of what the security hardware actually does — and a reminder of why we use the period ACP in the SEM rather than attacking the CA ourselves. Required viewing before the sacrificial-box teardown.

### 2.3 CableLabs R-OOB specification (CM-SP-R-OOB-I01-150615, 2015) — vendor-side corroboration

<https://account.cablelabs.com/server/alfresco/553724f3-caa8-41b8-94e1-bafc12e73888> — the Remote Out-of-Band spec's Appendix I describes today's 55-1 OOB delivery and confirms our architecture map from the industry side: the OM creates the OOB signal; the component set is LFG (guide data), DAC (EAS), RADD (MPEG data for video operation), NC (interactive applications and STB message responses), OM; the OOB carries EMM, Network (virtual channel map / NIT), code-download images, PAT/PMT, EAS, guide data, and interactive data; downstream sits at 70–130 MHz, "typically 75.25 MHz or 104.25 MHz"; upstream is 5–42 MHz with 1 × 3.2 MHz + 3 × 192 kHz channels.

Relevance: independent confirmation of the Reproduction Plan's component inventory, the oob-mux stream list, our hunt-table anchors, and the STARVUE upstream window — free, authoritative, and citable in oob-msg's protocol document.

### 2.4 Standards and period silicon

ANSI/SCTE 55-1 2019 (R2024) — the Mode A standard itself, current and purchasable from SCTE; the ANSI preview shows the normative TOC (OOB transmission format, coding scheme, QPSK mapping, modulator RF output, receiver input power, return-path modem): <https://webstore.ansi.org/preview-pages/SCTE/preview_ANSI+SCTE+55-1+2019.pdf>. Purchase decision logged as optional (§6) — the DCT2000 manual, R-OOB (§2.3), and the box-as-oracle loop may suffice.

SCTE 28 Host-POD (public mirror: <https://law.resource.org/pub/us/cfr/ibr/005/scte.28.2007.pdf>) — the retail-box interface; corroborates the 70–130 MHz FDC tuning range, the 1.544/3.088 Mb/s forward and 5–42 MHz return rates, and the host/POD split (the operator keeps MAC control of upstream — why our boxes' transmitters stay headend-gated).

NXP TDA10026HN (<https://www.nxp.com/products/TDA10026HN>) — period silicon: "a QPSK demodulator with FEC, compliant to SCTE55-1 and SCTE55-2." The class of front-end chip our boxes' OOB receivers are built around; useful if D0 board analysis wants a datasheet anchor.

## 3. Box-side hobby documentation

- cromwell-intl.com — Motorola DCT 2000 Diagnostics (<https://cromwell-intl.com/technical/dct-2000.html>, updated 2025-08): a careful walkthrough of the diagnostic screens on a live Comcast box — Platform 0x0011 / Family 0x00b / Model 0x00f2, OOB SNR and carrier readout, unit-address format, firmware version with last-updated timestamp, and IP-assignment observations (176.0.0.0/8, later 10.0.0.0/8). Confirms the d-screen content our capture rig will scrape and that hobbyists are still poking these boxes today.
- DCT2500 Installation/User Guide (mirror: <https://www.hcc.net/wp-content/uploads/2012/02/DCT2500_User_Guide.pdf>) — documents d 10 Application Code Modules (module name, version, status — LOADING/ENABLED/DOWNLD/…, ApplID) and d 11 Memory Status: the exact screens oob-cdl must make change state, with the state machine vocabulary written down by the manufacturer.
- JP1 remote-control hacking (AVSForum, 2001: <https://www.avsforum.com/threads/jp1-hack-needed-for-gi-dct1000-2000-cable-box-problem.36416/>) — period-era DCT "hacking" meant IR remote codes, not headend work.
- Forum hearsay — "Motorola DCT 2XXX series cable boxes have been hacked and cloned" (digitalhome.ca, unattributed, no artifacts): treat as noise; nothing public backs it.
- The demand exists — e.g. r/Comcast_Xfinity, "Is it still possible to activate a legacy cable box such as a Motorola DCT…" (<https://www.reddit.com/r/Comcast_Xfinity/comments/194ca5y/>). Every such thread ends at "only the cable company can." No one answered with a headend.

### 3.1 DCT 2000 FAQ (usbjtag.com) — community tier, structural facts only

<https://www.usbjtag.com/dct2000/faq.php> — a user-contributed FAQ on DCT-family set-tops (about Dec 2016, no named author). **Trust level: community-tier** (ADR-001 §4 discovery order: community captures are the last and lowest-authority tier). Nothing below is `verified`; each item needs a box capture under `lab/captures/` before it can inform `oob-msg` or the box-type catalog (AGENTS.md §2). We take only structural and diagnostic facts.

| Fact | Source claim | Trust | Where it lands |
| --- | --- | --- | --- |
| Diagnostic-menu entry | With the box on, power it off (front button or remote), then press SELECT (front) or OK (remote) within about 3 s. One diagnostic field noted: Memory Config, "EEPROM Ver.NO." | community, unverified | **Open item:** the Capture Rig Specification does not yet document how to enter the diagnostics (its §3.1 and §4 cover capture and the screen payloads only). This sequence is a candidate for Capture Rig §7 (open items); confirm on the sacrificial unit and compare against the cromwell-intl write-up (§3.2) |
| E-codes | E00 no error; E01 not connected; E08 POST error; E09 NVRAM corrupted; E11 invalid unit address. E02–E07 and E10 carry no text on the page | community; corroborates our own manual-sourced E-code remedies | Enrollment path (Reproduction Plan, r4 F4-4): E09 → factory-init, E01 → connect, E11 → unit-creation. Corroboration only; the manual stays the authority |
| Config storage by board revision | PH6 and older: 24LC01 (128-byte) config EEPROM. PH7/8: config held in flash at $404000. Checksum selection logic reported (data block $23ceec, checksum $23cf6c, address reference $0022269A). Revisions named PH6, PH7/8, PH7/8/9 | **unverified** (all of these values are on the FAQ page; community-tier and not box-confirmed) | Hint for the E09 / dead-NVRAM factory-init path (r4 F4-4), firmware-import tooling (#33), and platform variation in the box-type catalog (#30). No design change |
| SPI-log serial capture | SPILog.exe over COM1 writes SPI0E000log.txt, SPI07000log.txt and auth.bin; SPICommand.exe (v0.3) replays | community technique reference only | **Not adopted.** Third-party tool; auth.bin and replay touch the auth tier. Our serial capture is the RS-232 scrape in the Capture Rig Specification §3.2 |

**Out of scope, deliberately.** The FAQ's main purpose is authorizing premium channels and PPV on a box, and it advises connecting or re-enabling a box on a local cable system. That is conditional-access defeat and service theft on a real plant. It is prohibited here under AGENTS.md §7 (closed plant) and §11 (never touch live cable plant, licensed services or anyone's content). None of those steps, values or tool usages are reproduced or adopted. Oob reproduces a closed lab plant with self-authored content; the E-code, diagnostic-entry and board-structure facts above are the only things taken. Relevance to O10 and O13: none directly. This is not a patent or wire-format source, so it moves no `oob-msg` status.

### 3.2 cromwell-intl.com "Motorola DCT 2000 Diagnostics" — community tier, one unit

<https://cromwell-intl.com/technical/dct-2000.html> (Bob Cromwell; the one-line mention in §3 is expanded here). A personal, secondary write-up of **one unit on a live cable service**. **Trust level: community-tier**; nothing below is `verified`. Each fact is labelled **page** (the author's own screen observation) or **quoted** (quoted from a third-party page, inferred or speculative). Entries need a box capture under `lab/captures/` before they inform `oob-msg`, the PID plan or the box-type catalog (AGENTS.md §2).

| Topic | Claim | Label | Where it lands |
| --- | --- | --- | --- |
| Platform | MC68331 CPU, with a second DSP/possibly-crypto CPU (the page quotes a third-party web page with "This page claims"; not the author's own testing); VxWorks OS (author citing a Wind River release, not reproduced); modem named STARVUE II (author's inference from the "STARVUE II DIAGNOSTICS" screen header) | quoted | W7 return-path context only; no design change |
| Diagnostic entry | Tap Power, then press Select repeatedly (author says it takes experimentation) | page | Corroborates §3.1's entry sequence (power off, then SELECT or OK within about 3 s); the two differ in detail. Entry is an open item for the Capture Rig Specification (see §3.1); confirm on the sacrificial unit |
| The 19 screens | 01 General Status, 02 OOB Status, 03 In Band Status, 04 Audio/Video Status, 05 Unit Address, 06 Firmware Version, 07 Current Channel Status, 08 Renewable Security, 09 Upstream Modem, 10 App Code Modules, 11 Memory Config, 12 Interactive Info, 13 MAC Frequency Table, 14 Message Types, 15 In Band PAT, 16 In Band PMT, 17 Task Status, 18 IB MCA Status, 19 Keyboard/LED | page (menu order as listed) | Names the real screens behind the d-screen map in Capture Rig Specification §4; check its d-number assignments against this order on our boxes |
| Box-type sample (General Status) | Platform ID 0x0011, Family ID 0x00b, Model ID 0x00f2, Tuner V860DL; author reads these as distinguishing models in identical cases (interpretation) | page / quoted | One community-observed sample for the box-type catalog (#30); unverified |
| Message Types screen | OOB PIDs 0001, 1C0A, 0777, 150c, 0FB0 (and 0000) with counts; in-band PIDs 0000, 0037, 0984, 0983 with Lost and DRAM columns; message-ID table (hex IDs, each with a hex count; a dash means no count shown) laid out as three side-by-side Msg Id / Cnt column pairs with irregular alignment. IDs as the page shows them: 0, 1, 2, 80, 81, 82, 83, 85, 86, 8C, 8D, 8E, 8F, 90, 91, 92, 95, 9A, A0, A2, B0, C0, C1, C2, C3, C4, C5, D8, E0, E3, FD (the page omits 84, 87–8B, A1 and B1, so these are not contiguous ranges). The message IDs 0xC0–0xC5 are box message IDs, unrelated to traffic classes C0–C5 (F6-3). Counter wrap widths are the author's guess | page (values); quoted (interpretation) | **Hypothesized leads only.** Candidates for the oob-msg catalog (#19) and the PID plan (#17), to be cross-checked against SCTE 55-1 and the box-as-oracle loop (Reproduction Plan §3). Not a protocol spec; no status promotion |
| In Band PAT / PMT | Menu items exist; the page does not describe their contents | page (existence only) | Box shows its own PSI: cross-reference #17 and the d 15/d 16 validation in AGENTS.md §8 |
| App Code Modules | 00082000, Tv_Guide, osc2000s, MACGclnt; "programs" is the author's guess | page / quoted | Guide residency and CDL module names for W8 and the d 10 state machine (§3); unverified |
| Memory Config | EEPROM version, NVMEM, DRAM, FLASH, ROM pools in platform and application columns; a system-heap page table | page | Names fields the capture rig can scrape; example values are one unit's and carry no design weight |
| Unit Address | Format nnn-nnnnn-nnnnn-nnn, base 10; author sees no relation to the IP address | page | Format hint for unit-creation data; **no address is recorded here** |
| MAC Frequency Table | 08672000 and 06480000 with power readings; author does not know the meaning | page (values); meaning unknown | Read loosely. 867.2 MHz would exceed our 860 MHz ceiling, so treat as unverified and possibly a different unit encoding |

**Observations only, no keys or bypass.** The Renewable Security screen showed little on the author's unit, and the "crypto core" idea comes from that quoted third-party page and is speculation. We record them as observations; nothing about keys, subscription data or defeating security is taken or adopted.

**Out of scope, deliberately.** The page's live-network actions are not adopted: running diagnostics against a live cable service and trying to scan, ping or traceroute the box's address on the cable drop. They are live-plant actions, prohibited under AGENTS.md §7 (closed plant) and §11 (never touch live cable plant, licensed services or anyone's content). Oob does this only on a closed bench with no live cable. The page's "Root password" footer link is unrelated to the DCT 2000 content and was not followed.

### 3.3 drew1440.com "Motorola Cable" — secondary historical index, lowest authority

<https://drew1440.com/motorola/> — a personal WordPress blog page on GI/Motorola DCT set-tops, compiled mostly from archived 1999–2006 GI, Motorola, Pace and Sony product pages, with the author's editorial asides. **Trust level: community-tier, secondary, and the lowest authority of §3.1–§3.3.** Unlike §3.1 and §3.2 it carries **no** usable hardware, flash, JTAG, serial, d-screen, E-code, PID or protocol detail, and no headend, DAC, OM or SEM content: it is a historical index only, to be checked against primary material. Nothing below is `verified`; the page itself is internally inconsistent in places (for example on whether the DCT5000 shipped).

| Topic | Claim | Label | Where it lands |
| --- | --- | --- | --- |
| Model lineage | DCT1000/1200 (1997); DCT2000 (1999, "Motorola 68xxx" CPU, "2Mb of VRAM" as the page writes it; the unit is as written, bits versus bytes not confirmed); DCT2500 (Broadcom single chip: CPU, MPEG-2 decoder, transport); cost-reduced DCT700, DCT1700, DCT1800; HD and DVR line DCT5100/5200, DCT6200/6208, DCT6400/6412/6416. DCT5000 announced then cancelled ("vapourware", the author's word; the page also says one version reached operators) | archived product pages; the cancellation is editorial and speculative | Context for the box-type catalog (#30) and the census framing; the 68k basis agrees with §3.2's MC68331 claim (also unverified) |
| Modulation | J.83 Annex B, 64/256-QAM; DCT1000 64-QAM only, DCT1200 adds 256-QAM; other models listed as 64/256 generally | archived product pages | Consistent with AGENTS.md §8 (J.83 Annex B, 64/256-QAM) |
| CA lineage (historical, descriptive only) | DigiCipher, later MediaCipher; descrambles earlier GI/Jerrold scrambling | archived product pages | Context for the encrypted tier (M3/G3). Descriptive only: no keys and no bypass |
| Return path | STARFONE (built-in dial-up modem); STARLINK (two-way data, "similar to DAVIC") | archived product pages | W7 context (STARVUE/STARFONE return path) |
| Middleware and guide stacks named | Prevue (IPG on DCT1000-series); Pioneer Passport (later IPG, DCT1000 and DCT2000); OpenTV (ported to the DCT2000); Liberate (deployed on the DCT2000 at Insight); Wink (Charter announced it, the page states no outcome; Time Warner announced it on the DCT1000, not rolled out; the page also notes Wink support in some DirecTV receivers); Worldgate and Microsoft TV (announced or tested, not deployed); Rovi iGuide appears as a heading with no content | archived pages and the author's notes | Guide residency (W8) and the R12 Navigator/Passport fallback; the stack a given fleet box runs still comes from the census |

**Scope.** The page is historical and descriptive: no CA-defeat steps, no live-plant instructions and no credential links, so beyond the standard closed-plant stance (AGENTS.md §7, §11) there is nothing to exclude.

### 3.4 usbjtag.com DCT700 device page — vendor tier, unverified

<https://www.usbjtag.com/jtagdevices/dct700.php> — a vendor device page for a USB BDM/JTAG tool (last updated 2016-12-31), describing the Motorola/GI DCT700 as similar to the DCT2500 design but digital only. **Trust level: vendor-stated, community-tier, lower authority than §3.2; every value is unverified** and must be confirmed on our own unit before anything relies on it (AGENTS.md §2).

| Topic | Claim | Label | Where it lands |
| --- | --- | --- | --- |
| Platform | Debug is EJTAG (MIPS), 14-pin with a reset pin, non-DMA. Read with §3.3's note that the DCT2500 uses a Broadcom single chip, this marks the DCT700 and DCT2500 as a **Broadcom MIPS** family, in contrast to the 68k/BDM DCT2000 (§3.2, §3.3). The page states the CPU as "Motorola 17500QK", which matches no part we know; likely a mislabel, since EJTAG implies a MIPS SoC | vendor-stated; inference ours; CPU name unverified | Box-type catalog (#30): platform variation across the family. No design change |
| Flash and memory map | One Intel 28F320J3 (4 MB). RAM 16 MB at 0x80000000, firmware 4 MB at 0x90000000, NVRAM 256 KB at 0x98000000 | vendor-stated, unverified | Firmware-import tooling (#33) and D0 planning, only as hints to check against our own unit |
| Flash partitions | boot 0x90000000 (0x20000); plat 0x90020000 (0x160000); app1 0x90180000 (0x80000); app2 0x90200000, app3 0x90220000, app4 0x90240000 (0x20000 each); rsvd 0x90260000 (0x1A0000) | vendor-stated, unverified | Same as above; partition names are a lead for the d 10 module vocabulary (§3), not a spec |
| Recovery workflow | The tool offers NVRAM backup and restore, full firmware dump and programming, platform-only programming, and a rescue path for a box that does not boot (boot partition first, programmed slowly, then the platform) | vendor-stated, community reference | Informs the **D0 flash-recovery** requirement (r3 F3-3, r4 F4-3) and #33. Described at a high level only; no runnable steps and no tool init values are reproduced |

**Destructive operations stay Oob's doctrine.** In Oob, flash write-back and rescue are destructive operations (AGENTS.md §7): sacrificial unit first, a verified D0 recovery path (two routes) before any fleet box, and Marshal's typed confirmation. This entry is reference for building that recovery capability on the closed bench, not an endorsement to flash boxes.

**Scope.** The page has no CA-defeat, PID or OOB content. Its note about removing the CATV cable to program reliably concerns programming reliability, not live-plant use; Oob's closed-plant stance (§7, §11) means no live plant regardless. Social and credential links were not followed.

## 4. Adjacent scenes (technique donors, not attempts)

- gr-qam (Clayton Smith; merged into GNU Radio 3.7.10: <https://github.com/argilo/gr-qam>) — SCTE 07 64/256-QAM transmitter, HackRF-based, webcam-to-QAM example. Precedent for the Interconnect hub-fallback "GNU Radio QAM on a second SDR" (Appendix A of the Hardware Acquisition List).
- HackRF analog TV transmission (PAL via rad1o, 2017: <https://www.rtl-sdr.com/transmitting-analog-tv-broadcasts-hackrf/>) — S3 analog-tier reference material.
- bcm2-utils / cable-modem-as-SDR (<https://stdw.github.io/cm-sdr/>, 2015) — Broadcom cable-modem firmware RE turned into a spectrum sampler; the adjacent DOCSIS device-hacking lineage.
- Synacktiv, "Hack the channel" (2025) — <https://www.synacktiv.com/en/publications/hack-the-channel-a-deep-dive-into-dvb-receiver-security> — modern DVB receiver security RE (flash extraction, partition tables); current craft reference for D0.
- The 4DTV hobbyist scene (e.g. <https://www.satelliteguys.us/xen/threads/4dtvs-still-good-for-anything.375813/>) — people still running DSR-920/922 DCII receivers years after the 2016 shutdown, mostly as precision dish movers. Same NVRAM failure mode as our fleet (dead battery → unit ID lost → factory revert) and an established "Map Master" reprogramming-tool economy around it — precedent that NVRAM rehab tooling (r4 F4-4) is the normal answer in this world.
- Sega Channel preservation (Video Game History Foundation, 2025-12: <https://gamehistory.org/segachannel/>) — 100+ ROMs and system data recovered from a former VP's personal archive and a community member's tape backups. The other great cable-data-artifact rescue — but a data-recovery project, not a headend reproduction.
- The homelab "DIY cable channels" scene — ErsatzTV + Blonder Tongue HDE-8C-QAM + Raspberry Pi clusters ("Tales From The Homelab: DIY Cable TV System"), Thor HDMI modulator headends, analog hotel-style headend how-tos. These build channel playout for ordinary TVs: no OOB, no DAC, no enrollment, no guide residency. Validates oob-playout's content side and nothing else.

## 5. The gap — what nobody has done publicly

- Generating a SCTE 55-1 Mode A OOB carrier at all (the one public OOB toolchain, §2.1, is receive-only and Mode B).
- Reimplementing the DAC message layer (factory-init, unit-creation, connect) outside GI hardware.
- Enrolling/authorizing a DCT without a cable company.
- KLS/DLS code download — .obj/.dat carousel generation.
- FPK/MediaCipher EMM generation (everyone who touched the CA, §2.2, attacked it; nobody re-hosted it).
- Resident-guide feeding (i-Guide listings over OOB to a real box).

As far as the public record shows, Oob is the first attempt at any of these — which raises the value of the "every experiment is a commit" rule: our protocol document will be the public artifact.

## 6. Actions logged

- Research desk (oob-expert): track oobscte55 — pipeline architecture, SDR++ capture conventions, and any future TX work port directly to our PHY and verification harness; the Mode B internals (derandomizer, SL-ESF, RS(55,53), ATM/DSM-CC) do not. Monitor the repo; no fork needed.
- D0/O13: Gerlinsky's talk (and slides) are required viewing before the sacrificial-box teardown; decap/optical-ROM/glitching is the deepest escalation rung and one we expect never to need.
- G5 consideration: the oobscte55 author is a natural correspondent if the far-future S-A second front (Sci-Atlanta Ecosystem spec) ever activates — logged, no outreach now.
- Standards purchase: SCTE 55-1 2019 (R2024) — optional buy, decision deferred to M0.5; escalate to required only if PHY bring-up stalls on coding/framing unknowns the box can't teach us.
- Research desk: DCT 2000 FAQ (§3.1) logged as community-tier, structural facts only; the d-screen entry sequence goes to the capture-rig bring-up as an item to confirm on the box, and the board-revision storage hints go to #30 and #33 as unverified. CA-defeat content is out of scope (§7, §11).
- Research desk: cromwell-intl write-up (§3.2) logged as community-tier. Its Message Types PIDs and message IDs are hypothesized leads for #19 and #17, to be cross-checked per Reproduction Plan §3; the 19-screen list is to be checked against the Capture Rig d-screen map. Live-network actions are out of scope (§7, §11).
- Research desk: drew1440 index (§3.3) logged as secondary and lowest authority; model lineage and middleware names are context for #30 and W8, to be verified against primary material.
- Research desk: usbjtag DCT700 page (§3.4) logged as vendor-stated and unverified; it hints at a Broadcom MIPS family and informs D0 recovery planning and #33. Flash write-back stays a destructive operation under §7.
- Citations into oob-msg: CableLabs R-OOB Appendix I (§2.3) is citable corroboration for the stream inventory and RF plan when the protocol document grows its references section.

Cross-references: Oob Reproduction Plan (methodology), Milestones & Stage Gates (G0.5 research-desk duties), Hardware Acquisition List (standards/textbook purchases are not on it — see item 4 above), Sci-Atlanta Ecosystem Specification (the 55-2 world §2.1 lives in).
