# Remote Service Delivery Specification — Interconnect

**Stretch goal S2 · post-M6 · registered 2026-11-22**

- **Status:** Proposed (R0 design) — no schedule weight until M6 exit
- **Parent docs:** `mini-headend Project Overview`, `Oob Reproduction Plan`
- **One-line:** let DCT2000-class boxes at *remote sites* run off the
  mini-headend across the public internet, behaving exactly as if they were
  on the bench plant.

---

## 1. The idea, and why it is faithful

Period cable plants were never one building. A master headend fed
**hub sites** over microwave (AML), fiber, or satellite (HITS literally
existed to distribute to headends), and each hub re-radiated RF into its
local coax. Interconnects tied systems together so guide data, ads, and
premium feeds stayed in sync.

Interconnect (S2) is the same architecture with the internet as the
long-haul: **ship content and control over IP; regenerate RF at the edge.**
The box at a remote site cannot tell whether its OOB carrier was modulated
in the next rack or regenerated from a TCP stream two states away — QPSK is
QPSK at the tap.

### Hard constraint that shapes everything

The boxes speak **only RF** — QPSK OOB downstream, QAM in-band, STARVUE II
upstream. There is no box-side IP anything. Therefore every remote site
needs local RF generation, and the design question is only *what crosses
the WAN*: raw IQ (wasteful — tens of Mbps per carrier of dead air),
stuffed transport streams (workable), or **message PDUs that the hub
re-frames locally** (right answer — see §4).

## 2. Goals / non-goals

**Goals**

- A remote site kit ("hub") that a reasonably technical friend can rack:
  plug in, join the VPN, serve boxes.
- Full OOB control of remote boxes: connect, VCM, region, authorizations,
  time, and (eventually) code download — indistinguishable from local.
- Clear-tier video to the hub at home-uplink-friendly bitrates.
- FPK encrypted tier to remote boxes **without any crypto hardware at the
  hub** (see §5 — this falls out of the FPK design for free).
- Multi-site semantics done the period way: per-hub lineups and region
  tags, one global unit database.

**Non-goals (v1)**

- No Full Encryption tier remotely (matches the main plan — that chain is
  defunct anyway).
- No upstream/return path at hubs in v1 (S1 finishes first; hubs get it in
  phase R4).
- No SLAs. This is hobbyist interconnect, not an ISP.

## 3. Architecture

```javascript
        MASTER HEADEND (Marshal's lab)                      REMOTE HUB
┌──────────────────────────────────┐          ┌──────────────────────────┐
│ oob-dac ── oob-mux ── oob-msg    │          │ hub agent (Docker)       │
│    │           │                 │          │   │                      │
│    │           │ msg PDUs/TCP    │ WireGuard│   ▼                      │
│    │           ├────────────────►│◄────────►│ oob-regen ──► flowgraph  │
│    │           │  (mTLS in WG)   │ tunnel   │      │                   │
│    │           │                 │          │      ▼ QPSK @ hunt freq  │
│    │           │ TS/SRT          │          │   HackRF/Pluto           │
│ playout ──► SEM│════════════════►│          │      │                   │
│ (ffmpeg/tsp)   │ (clear + FPK    │          │      ▼                   │
│                │  scrambled TS)  │          │   combiner ──► tap ──►   │
│                └────────────────►│──► tsp ──►│   QAM mod ──► plant     │
│   ▲            │  (via SEM       │          │      ▲                   │
│   │ upstream   │   GigE-out)     │          │      └─ local TS loop    │
│   └────────────┤◄────────────────│◄─────────│ (R4: STARVUE demod,      │
│                │                 │          │      STARFONE over VoIP) │
└──────────────────────────────────┘          └──────────────────────────┘
```

The hub is deliberately the *same shape* as the master plant at miniature
scale: an OOB TX, a QAM source, a combiner, a tap, and discipline about
dummy loads. The F3-7 RF chain rules apply at hubs unchanged — closed
coax, terminators, no exceptions (hub carriers sit in the FM band too).

### Hub kit v1 (BOM sketch)

| Item | Role | Notes |
| --- | --- | --- |
| SBC (RPi 5 or x86 mini-PC) | hub agent host | runs our containers; 8 GB class |
| HackRF One clone, ADALM-Pluto, or LibreSDR | OOB TX | Supported set; primary chosen at the G0.5 bench by the F4-1 level log (Marshal, 2026-11-23). Pluto needs the range hack at hunt frequencies; Pluto acceptable as kit-recycling |
| QAM source (pick one, in order of preference) | in-band RF | (a) surplus SEM V8 507199-**002** (ACP-less — clear tier is all a hub needs locally; F4-2 already has these on the O1 watch); (b) HDMI→QAM gadget (pragmatic, period-wrong); (c) GNU Radio QAM on a second SDR (CPU-heavy, fidelity risk) |
| Mini RF chain | plant | 2-way combiner, tap, attenuator, 75 Ω dummy loads, RG6 |
| Optional (R4) | return path | RX-capable SDR for STARVUE + USB cam/OCR telemetry reuse of the F3-2 capture rig |
| Optional | ops | smart PDU for remote box power-cycling |

## 4. What crosses the WAN (per flow)

### 4.1 OOB control — ship messages, not symbols

The constant-rate requirement lives at the RF edge, not on the internet.
So:

1. Master `oob-mux` emits **message PDUs** (addressed, sequenced,
   timestamped) over a persistent TCP connection inside the WireGuard
   tunnel.
2. The hub's **oob-regen** service performs the 55-1 framing and
   **null-stuffing locally at exactly 2.048 Mb/s** (1.024 Msym/s QPSK),
   feeding the SDR
   flowgraph at a hard constant rate. WAN jitter is absorbed by a PDU
   buffer; if the buffer starves, regen stuffs nulls — exactly what a real
   OM does when the DAC is quiet. Boxes see an unbroken carrier either way.
3. Ordering is preserved by sequence numbers; loss is re-requested for
   non-idempotent messages (download control), tolerated for cyclic ones
   (connect/auth repeats are the norm anyway).

**R1 shortcut:** ship the fully-stuffed OOB transport stream over TCP
(~2 Mb/s constant) and let the hub flowgraph consume it from a deep buffer.
Simpler — one less service — at the cost of shipping stuffing across the
WAN and losing per-hub addressing. Fine for the first single-hub test;
regenerator is the target architecture.

### 4.2 In-band video — SRT

Playout TS crosses as **SRT** (ARQ, ~1–2 s latency budget) to the hub,
where `tsp` re-clocks and hands it to the QAM source. PCR/continuity
re-stamping at the hub with `tsp` filters as needed. Bandwidth is the
honest constraint: MPEG-2 MP@ML at 3–6 Mb/s per service means a home
uplink carries a *curated* lineup (2–8 services), not a full 1999-basic
tier. The hub subscribes to a channel set; `lineup.yaml` declares per-hub
mappings.

### 4.3 Time, region, and guide data

All ride the OOB flow — Time messages originate at the master (boxes at
hubs keep master time; period-correct, hubs never had their own clocks),
Region messages carry the hub's `epg_region` tag if we ever want
per-hub listings variants. Guide data is cyclic carousel content; WAN
outages just freeze the cycle (see §6).

### 4.4 Return path (R4, after S1)

STARVUE II demod happens at the hub (RX SDR), bursts ship back as PDUs to
the master's STARVUE parser. STARFONE rides SIP to the master's modem
bank. Both are low-rate and latency-tolerant. v1 hubs run one-way, which
is exactly how many period one-way plants operated — purchases via the
operator's phone line were always the fallback.

### 4.5 Management plane

Hub agent: WireGuard mesh enrollment, container updates (watchtower-style
or pull-on-command), metrics/logs back to the master's monitoring,
watchdog that keeps the OOB regen alive (a hub whose OOB dies strands its
boxes at `E 01` — the single worst hub failure), and a remote
power-circuit for the bench boxes via smart PDU. Hub keys and configs live
in the git-crypt tier like everything else sensitive.

## 5. The FPK tier crosses for free — and why

FPK (Fixed Program Key) is the same key in every DCT2000 ever made; the
SEM's ACP scrambles once, at the master. A scrambled transport stream is
therefore **decryptable by any box anywhere** — including boxes behind a
hub two time zones away. So the FPK tier at a hub is just: ship the SEM's
scrambled output over SRT, modulate, done. No hub-side ACP, no key
material in transit (the *ciphertext* is the cargo; keys never leave the
master SEM), no change to the threat model.

**Verify at bench (V1):** that the SEM can emit encrypted programs on a
GigE/ASI *output* (not only to its own QAMs) so the scrambled TS is
available for WAN shipment. If it can't, fallbacks in order: (a) hub gets
its own ACP-equipped 507199-001 (expensive, opportunistic); (b) hub runs
clear tier only until the software DES/CSA path exists (backlog, no date; Plan W5 backlog note) —
the one scenario where the demoted FPK-extraction work regains schedule
relevance. Flagged here so nobody is surprised later.

## 6. Failure and degradation model (period-authentic on purpose)

| Failure | Box-visible behavior | Faithful? |
| --- | --- | --- |
| WAN down, hub alive | OOB regen stuffs nulls from its buffer, then sends locally-cached cyclic messages (connect/auth/time) with last-known config; boxes stay authorized (Control Channel Timeout set long); guide data ages gracefully | Yes — this is a microwave fade at a real hub |
| Hub OOB dies | Boxes drift to `E 01` after timeout | Yes — and the hub watchdog exists precisely to never let this happen |
| WAN lossy | SRT retransmits; video freezes before it corrupts (MPEG-2 over packet loss, the classic look) | Yes |
| Master down, WAN up | Hubs degrade to cached-config mode; nothing new authorizes until master returns | Yes — DAC outages were like this |

Degradation is a feature to get *right*, not paper over: a real hub in a
storm behaved exactly this way, and the boxes' diagnostics were built to
report it.

## 7. Security and content posture

- All hub traffic inside WireGuard; hub identities are device keys in
  git-crypt; no box secrets ever traverse the WAN (there are no box
  secrets — FPK's nature is the point, and it never leaves the SEM).
- Content remains self-authored on the remote leg. Nothing off-air is
  retransmitted across the internet — the interconnect carries our own
  loops and listings only, full stop.
- Remote sites remain closed plants: hub kits ship with terminators and
  the same dummy-load-first rules. A hub that radiates into the FM band
  is a hub that gets its VPN keys revoked.

## 8. Phasing

| Phase | Content | Exit |
| --- | --- | --- |
| **R0** | This document; hub config schema (`site.yaml` per hub: ID, region tag, lineup mapping, radio roles) | Reviewed alongside r3/r4 dispositions |
| **R1** | Loopback hub: a hub kit on the same bench, WAN simulated with netem (delay/loss/jitter); OOB via stuffed-TS shortcut; one box authorizes through the tunnel | Box at `E 00` behind a 100 ms/1%-loss link |
| **R2** | First real remote site, clear tier, one box; oob-regen replaces the shortcut | Remote box tunes the lineup; connect survives a forced WAN flap |
| **R3** | FPK tier remotely (ships master's scrambled TS; gate = V1 bench check) | Remote box decrypts; d 01 auth reason correct |
| **R4** | Return path: STARVUE demod at hub; STARFONE over SIP | Remote poll response logged at master |
| **R5** | Multi-hub: second site, per-hub lineup + region tag, EAS targeted per hub | Two hubs, one unit DB, distinct VCMs |

**Dependencies:** R1 needs M1 + the F3-2 capture rig pattern (telemetry
for a box you can't see); R2 needs M2; R3 needs M3 + V1; R4 needs M5.
Everything here is post-M6 scheduling by standing rule — the stretch
register, not the critical path.

## 9. What this buys the project

- **A second oracle.** Two independent plants running the same headend
  build is the cheapest conformance test there is — divergent box
  behavior between sites localizes bugs instantly.
- **Community on-ramp.** Other collectors' DCTs come back to life by
  shipping them a hub kit, which is also how the protocol document gets
  exercised against box variants the fleet doesn't own.
- **Scale realism.** Per-hub lineups, region tags, and site-targeted EAS
  exercise corners of the DAC semantics (VCM, Region, EAS addressing)
  that a single bench plant never touches.
- **It completes the illusion.** A cable system you can only watch in the
  room where it's racked is a demo. A cable system with hubs is a
  *network*.
