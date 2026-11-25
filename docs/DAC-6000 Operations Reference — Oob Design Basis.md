# DAC-6000 / SEM V8 Operations Reference — Oob Design Basis

*Renamed 2026-11-22 with the naming adoption: the DAC reproduction daemon
is `oob-dac` (formerly "oob-dac").*

- **Date:** 2026-11-21
- **Status:** Working reference, mined from vendor manuals
- **Sources:**
  - *Basic DAC 6000 (V3.1) Operation, Rev 1.2* — Motorola training manual, 648 pp (2006), Scribd doc 360786477
  - *Motorola SEM V8 SmartStream Encryptor Modulator, Software Version 7.1.x* — 295 pp (2015), Scribd doc 833540186
- **Purpose:** everything in these manuals that shapes oob-dac's data model, API, message catalog, and milestone plan. Page references use the manuals' internal numbering.

---

## 1. System model (DAC V3.1 era, §1)

| Component | Role (as documented) |
|---|---|
| **DAC 6000** | Addresses and controls headend equipment and terminals; authorizes services/features; collects purchases from two-way terminals; monitors the terminal population; supports virtual channels. Runs on HP ProLiant (ML570 G3), SUSE Linux + Sybase 12.5, OSF/Motif GUI, TCP/IP + SNMP. |
| **RADD 6000** | "Remote Addressable DANIS/DLS" — executes real-time message operations: **polling and downloads** to terminals. Hosts the **DLS (DownLoad Server)** that builds download streams. |
| **KLS 1000** | KeyList Server — "verifies terminals and authenticates purchases… provides data and keys to encrypt and decrypt." 1U box. Group Configuration Request (**GCR**) for the primary KLS comes from **Motorola's Encryption Control Center (ECC) in San Diego** (often via modem dial-out). Tracks **UKL (Unit KeyList)** files; activity states include `Validation Failure`, `Incompatible UKL`. |
| **OM 1000** | Converts control-data stream to RF OOB, **71–129 MHz**; stream also carries guide data and downloadable objects. (OM 2000 is the later unit.) |
| **RPD 1000/2000** | Return Path Demodulator — receives/demodulates RF upstream from terminals, forwards to NC 1500. |
| **NC 1500** | Network Controller — IP gateway between application and OAM&P networks; interactive (VOD) support. |
| **SEM V8** | SmartStream Encryptor Modulator — remultiplexes, **encrypts (MediaCipher, up to 128 services), and QAM-modulates**. ACP encryption modules: **MediaCipher (DES and CSA scrambling)** or MC+SimulCrypt (CSA CW generation under an SCS). EMMs normally supplied by the DAC acting as **EMMS (EMM Server)** via SimulCrypt EMMG↔MUX TCP/UDP links. |
| **IRT 1000/2000** | Satellite receiver/decryptors at the headend; each contains a **TSODA** decryptor element (2 services, ≤6 PIDs) that the DAC authorizes per service. |
| **Business System** | Subscriber DB/billing; drives the DAC over serial/TCP via **WireLink** (BSI) commands. |
| **CPMS** | Copy Protection Management System — CableCARD copy-protection validation messages. |

**Takeaway for oob-dac:** our single daemon covers DAC + RADD + NC + DLS + EMMS
roles. KLS is the only role with an external dependency (Motorola ECC) that
cannot be reimplemented — see §3.

## 2. Encryption modes — the R1 breakthrough (§7, pp. 7-9/7-31; SEM §2, pp. 16–17)

DAC-documented per-service/per-program encryption modes:

| Mode | EMM infra needed | Per-box DAC authorization needed | Notes |
|---|---|---|---|
| **Full Encryption** (default) | Yes (KLS → EMMs) | **Yes** | Rotating working keys, per-unit keylists. |
| **Fixed_WK** (Fixed Working Key) | No rotating keys | **No** | Learning exercise §7: "The DCT needs to be authorized by the DAC 6000 to decrypt video when using this encryption mode" — single correct answer is FULL ENCRYPTION. Fixed modes and Unencrypted need no authorization. |
| **Fixed_PK / FPK** (Fixed Program Key) | **None** | **No** | SEM manual: "The SEM can encrypt programs using Fixed Program Key (FPK) encryption **without the need for an EMMS**… without the need for any other external devices." All programs in CTE mode share one program tier. |
| **Unencrypted** | No | No | Our M2 clear tier. |
| **Pre-encrypted** | (upstream) | — | Pass-through of already-scrambled content. |

**Implications:**
1. **Fixed_PK removes the EMM/keylist problem entirely.** No EMMS, no KLS, no
   per-unit keys, no DAC authorization — the box decrypts because the program key
   is a fixed constant known to DCT firmware.
2. The remaining unknown collapses to a **single universal secret**: the Fixed
   Program Key value (and the DCII scrambling flavor — DES vs CSA — used by
   DCT2000-class boxes). One extraction serves the whole fleet. Candidate
   sources: box firmware analysis (DCT2500 Broadcom path), SEM documentation,
   or the surplus-hardware lab.
3. Scrambling is ours to implement in the playout chain (DCII DES/CSA) once the
   constant is known — tsduck and ffmpeg both have CSA/DES-CSA building blocks.
4. **Full Encryption per-service addressability** remains the only mode that
   truly needs the KLS/ECC chain (§3) — now explicitly a stretch goal, not a
   gate.

## 3. Key management architecture (§5, pp. 5-88…5-94)

- **KLS 1000** holds encryption elements grouped by **Group Number**; defined by
  Name/IP/Group/Tag (physical tag on the unit).
- **GCR (Group Configuration Request):** Motorola provides the GCR for the
  primary KLS at install; additional ones come from the **ECC in San Diego**.
  The DAC can dial out by modem to request GCR updates.
- **UKL (Unit KeyList):** 9-digit File ID per customer ID; "View Unit KeyList"
  shows the state of assigned keys; KLS activity state `Incompatible UKL` exists
  for mismatches.
- **MCSC mating:** the DAC's "Configure MediaCipher Smart Cards" window is used
  "to mate an MCSC with a host set-top or remate a different MCSC"; remating at
  the box is physical (remove/insert card).

**Implication:** a surplus DAC 6000 without a valid UKL set from Motorola's ECC
can likely still run Fixed_PK / Fixed_WK services (no EMMs needed) but **not**
Full Encryption. This reshapes Option D: the oracle's real value is message
ground truth and mating ops, not necessarily Full-Encryption EMMs.

## 4. Terminal lifecycle and unit database (§9, pp. 9-5…9-18)

**Add a Terminal — unit DB schema:** Terminal Type (model + comm type), **Unit
Address** (unique 16-character decimal), **Serial Number** (12-char
alphanumeric), optional Account Number, Business System Name, Downstream Plant
Name, Upstream Plant Name (if two-way), on-plant flag.

**Terminal communication types** (Define Terminal, Terminal Type field;
page not yet cited): the field gives the model number and one of five
communication types: **One-Way, Fone-Way, Two-Way, Ethernet or DOCSIS**.
A **Fone-Way** terminal reaches the headend through the **Modem Bank**
named on its record. The Purchase History report lists archived collected
purchase information for Fone-Way and Two-Way terminals (terminal ID,
service index, purchase time, poll date). *Inference:* a **One-Way**
terminal has no return path, so purchase collection does not apply to it
(it is outside the Purchase History scope), and neither do polls (Plan
§3.2: Last Response Date is "poll-based; inactive for one-way units").

**Terminal operations (all documented DAC commands):**

| Operation | Semantics |
|---|---|
| **Initialize** | Sends configuration sequence; clears NVRAM *except* downloaded objects and view-history stack; clears passwords. |
| **Cold Initialize** | Clears NVRAM *including download code objects* and static RAM; resets user processor; "returns a terminal to an **out-of-box state**" then sends the configuration sequence. (On ASTB: erases flash except boot code + base platform.) |
| **Factory Reset** | ASTB only; flash erased except boot code; boot code can process only boot-code-control and download messages. |
| **Refresh** | Updates terminal info. |
| **Disconnect** | Prevents the terminal from displaying any video. |
| **Clear PIN** | Clears passwords. |
| **Configure Transmission Parameters** | Map Name, Amplitude Level, Upstream/Downstream Plant, **Modem Bank, RPD channel and frequency**, interactive DS/US IDs, impulse credit limits. |
| **Authorize Services / Packages / IPPV** | Per-service, per-package, and impulse authorization; **all IPPV services share one tier**. |
| **Clear Orphan Purchases** | Per-set-top purchase cleanup. |

**R3 fallout:** `E 09`/`E 11` recovery semantics are now documented — Cold
Initialize is precisely the "out-of-box" recovery operation, and the
bring-up contract (§5 of the ADR) maps to Initialize → configure sequence.

## 5. Provisioning API — WireLink/BSI command set (§4, p. 4-19)

The Business System Interface ("WireLink", 600/700 series) is the DAC's external
provisioning protocol — adopt these verbs and codes as oob-dac's admin API:

```
001 Synchronization / Return Status
620 Collect Event Purchases          621 Return Message      622 Received Event Purchases
630 Download (Add) Digital Events    631 Download (Modify)   633 Download (Delete)
660 Add New Set-Top                  662 Change Set-Top Service
663 Delete Digital Set-Top           664 Initialize Digital Set-Top
665 Clear Parental Lock              666 Change Set-Top Activation
680 Change Set-Top Features          684 Collect Set-Top Services/Purchases
685 Return Message Query Services/Features/Programs
688 Change Set-Top Channel Map       694 PPV Authorization Update
720 Upload IPPV Purchases            721/722 Return/Receive
724 Upload Single-Terminal IPPV      725/726 Return/Receive
760 Terminal Request                 770 Read Terminal Settings   771 Return Message
794 Authorize Programs
```

Multiple WireLink ports are supported (e.g., one for standard transactions, one
high-priority PPV authorization, one IPPV uploads). Expanded result codes exist
as a port attribute.

## 6. Code download model (§12, pp. 12-2…12-80)

**Object model (Core terminals = DCT2000-class):** a downloadable object ships as
`.obj` (code/data) + `.dat` (Data Description File) containing: object name,
application ID + version, object type (code/data), object version,
classification (**basic platform / application / loader**), load address, object
size, svar size, constructor offset, destructor offset, text description. The
**DLS builds the download data stream** from these.

**Control messages** (Enable / Disable / Purge / Delete) — "distributed… in the
**EMM data stream**": Enable starts an object (one version at a time), Disable
stops it, Purge removes all but the specified version, Delete removes it.

**Tune-to-download messages:** Conditional Tune Download Channel (Core), ASTB
Tune Download, and **Bootcode Control** (broadcast; instructs the bootloader to
download the base platform).

**Workflow:** create object modules → add code suites → download code suites to
RADD(s) → define decoder conditionals (which terminal types get what) → assign
software objects → load assignments to RADD → monitor/control (check download
status, monitor carousel, control).

**M4 fallout:** the carousel we must build is the DLS function with a documented
object container; control flows ride the EMM stream we already have to emit;
box-side observability is d 10. Risk drops from "undocumented" to "object
format + segment framing to confirm against a live box."

## 7. Return path and polling (§1, §9)

- STARFONE boxes' purchases reach the DAC via **headend modem banks** ("Modem
  Bank" is a per-terminal transmission parameter); polling itself is a RADD
  real-time operation.
- Two-way RF terminals get **RPD channel and frequency** assignments per
  terminal; RPD 1000/2000 demods feed the NC 1500.
- The DAC "monitors the terminal population" — keepalive/status reporting is a
  core loop worth emulating for our own observability.

## 8. Operations and housekeeping

- **Time zone management** is automated from DAC release V2.90+ (set-top time
  zone handled without scripts).
- **EPG guide providers:** TV Guide or **local servers** — the local-server
  option is our guide path (LFG-equivalent).
- **bsilog:** BSI transaction logger (14 MB rolling + archive) — copy the pattern
  for oob-dac's provisioning audit log.
- Reports include **MCSC Mated/Unmated** lists — the DAC tracks card mating
  state fleet-wide; our unit DB should too.

## 9. oob-dac module mapping

| DAC function (manual) | oob-dac module |
|---|---|
| Define Terminal / unit DB (§9) | `units` store (unit addr, serial, type, plants, features) |
| Initialize / Cold Init / Disconnect / Clear PIN (§9) | `lifecycle` command emitter |
| Authorize Services/Packages/IPPV (§9, §7) | `entitlements` (tiers, packages, credit) |
| Channel maps (§8) | `lineup` (from `lineup.yaml`) |
| WireLink 600/700 (§4) | `admin-api` (same verbs/codes) |
| DLS download stream (§12) | `cdl` carousel builder (.obj/.dat loader) |
| EMM stream incl. download control (§12) | `emm` (Fixed-mode now; Full later) |
| Polling / terminal monitor (§1) | `poller` |
| Operation status (§11, p. 11-4; this doc §10) | `command::lifecycle` (#22): status names are code (Layer 3), transition table `hypothesized` |
| Time zone auto (§14) | `clock` |
| Local EPG server option (§7) | `guide` |
| bsilog (§4) | `audit` log |
| STARFONE modem banks (§1) | `starfone` (mgetty NAS bridge) — stretch |
| RPD channel/frequency config (§9) | `starvue` upstream config — M6 |
| SEM V8 scrambling (DES/CSA, FPK) | **SEM ACP** (FPK mode, DES or CSA) — no `playout` scrambler stage; oob-dac only records the tier matrix (SEM-ACP decision, Plan W5). The software DES/CSA path in oob-mux is backlog only (Plan W5 backlog note) |

## 10. Operation status (Define Operations, §11, p. 11-4)

The manual introduces these as "Status indications are as follows" (p.
11-4). Every status describes the **event on the DAC and RADD 6000**. None
describes a terminal reply. Definitions are paraphrased, with short quotes
marked.

| Status | Meaning (p. 11-4) |
|---|---|
| **New** | Not yet processed, because its start time hasn't arrived. |
| **Request Sent** | Read and initiated by the DAC. |
| **Request Sent Failed** | The operation manager could not send the event to its destination task. |
| **Timeout** | Initiated, but "did not get an acknowledgement". The manual doesn't name the acknowledger; the RADD is the likely reading (inference). |
| **Downloaded** | Initiated and "accepted by the RADD 6000". |
| **Retrieving Status** | The DAC is fetching the event's status from the RADD 6000. (The Retrieve Operation Status control that triggers it is named elsewhere in the manual; seen only in search-index excerpts, page not yet cited.) |
| **Unknown** | The RADD 6000 has no record of the event. |
| **Device Offline** | The RADD 6000 is offline or has lost communication. |
| **Active** | The event is executing. |
| **Completed** | The event completed successfully. |
| **Marked Completed** | The event was active but passed its end time, so the system closed it out as completed. |
| **Failed** | "The operation failed." |
| **Delayed** | A Category Epoch operation is currently active. (Inference: the event is held until that operation ends.) |

**Takeaway for oob-dac (#22):** the lifecycle follows the DAC → RADD
handoff (hypothesis H1 on #22), not terminal replies. Terminal results
(Last Response Date, Failures, Chronic Non-Responding) live on the
terminal record. Neither the manual nor this table gives transition order
or a final status for One-Way operations. Oob's transition table
([#22](https://github.com/OnTheStruggleBus/oob-system/issues/22#issuecomment-7106984109))
is a `hypothesized` reconstruction. It ends a One-Way operation at
**Completed**, or **Marked Completed** if its end time passes while Active.
The reasoning: per the table above, the status depends on the event on the
DAC/RADD, not on a terminal reply. Oob's daemon plays the DAC and RADD
together (§1 takeaway). Research notes: issue #22 (oob-expert).
