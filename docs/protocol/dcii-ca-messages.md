# DCII CA message families: ECM40, ECM41, EMM95 and the ACP oracle

The O4 "DCII conditional-access message families" row, as a protocol-doc
chapter. It documents the **structure and semantics** of the GI/Motorola
DigiCipher II (DCII) conditional-access messages the headend must emit, so
a developer can build encoders in `oob-msg` and schedulers in `oob-dac`
against real field layouts rather than guesses.

**Scope: structure and semantics only — no values.** This chapter carries
field names, roles, ordering and lifetimes. It carries **no key material,
no FPK constant, no key values (seed, category, program or working), no
Unit Addresses, and no captured hex that embeds any of those** (AGENTS.md
§5, §11). Encrypted-key fields are named as opaque N-byte blobs and go no
further.

**Nothing on this page is box-verified.** The oob-msg taxonomy is
`documented` (manual-sourced) / `hypothesized` (constructed, unverified) /
`verified` (box-accepted) (AGENTS.md §2). This chapter's sole source is a
community-tier research deck, not a manual, so every family below is
`hypothesized` — never `documented` and **never** `verified`. There is no
capture from *our* box behind any of it. Promotion
to `verified` needs a d 15 / d 16 (or upstream-sniffer) capture from an Oob
box committed under `lab/captures/` and referenced from the message family
in `oob-msg`; the box is the referee (AGENTS.md §2). The crate is the
protocol document; this chapter renders its annotations.

## Source and provenance

The structural content here is synthesized from **corpus item 12** — the
Gerlinsky / @akacastor conference research deck on DCII/ACP
reverse-engineering — cited, not reproduced. That source is public
primary-research, multi-method (real MPEG-TS captures decoded with
dvbsnoop, ACP SPI-bus logs, an optical ROM dump of the ACP firmware with
IDA disassembly, and a 65c02 simulation), with constants cross-checked
against ROM checksums. Its vintage is ~2015–2016 research on 1998-era
GIC silicon; DCII is the same CA family the DCT2000-class fleet uses, so
the **family and structure transfer**, while the deck's field *values* are
capture-era data from a different operator's plant and are illustrative of
layout only.

Under the ADR-001 §4 discovery order (SCTE 55-1 spec → structure-guided
construction from the DAC §11 semantic payloads → patents → firmware-
assisted parsing → community captures; see AGENTS.md §2), this source is the
**firmware-assisted parsing tier, published through community channels**:
its optical ROM extraction and IDA disassembly are firmware-assisted
parsing, and the deck reached the public through conference and community
channels. That tier is not a
manual-sourced authority, so the structure it sources and the encoders it
seeds are `hypothesized`, not `documented`; it cannot establish box
acceptance. The ROM-sourced
constants in the deck carry the author's own caveat (~0.04 %/bit optical
extraction error, not uniformly checksum-verified), so they are
strong-but-not-absolute.

**Discovery-order log: skipped tiers are unchecked, not disproved.** This
chapter's layouts come from the firmware-assisted tier above, which is later
in the ADR-001 §4 order than three tiers that have **not been checked**:

- SCTE 55-1 spec (tier 1): not in the repo; acquisition is open item O7.
  Whether it specifies any CA payload structure is unchecked.
- DAC §11 structure-guided construction (tier 2): DAC §11 is semantic (what a
  Define Operation means). A bit-level cross-check of ECM/EMM/ACP layout
  against it has not been run.
- Patents, O10 (tier 3): the patent search has not been run, so no patent
  has been checked.

Because these legs are unchecked, this chapter does not claim the higher
tiers lack these layouts. Item 12 is the only source in hand for them, which
is why every family here is `hypothesized`.

**Promotion gate (what a passed cross-check earns).** A passed cross-check
against SCTE 55-1, DAC §11 or the O10 patents strengthens provenance and
confidence in a family's layout and in the `0x4749` `CA_system_id` candidate
(tracked on #56). It does **not** by itself lift a family above
`hypothesized`. Per AGENTS.md §2, `documented` is manual-sourced and needs
the manual (DAC §11 or SCTE 55-1) in hand; `verified` is box-accepted and
needs a box capture (d 15 / d 16, promotion path on #80). Until one of those
holds, every family stays `hypothesized`.

## 1. CA constants and where they live

| Constant | Role | Status |
| --- | --- | --- |
| DCII `CA_system_id` candidate `0x4749` ("GI" ASCII) | Identifies the GI/Motorola CA system inside the `CA_descriptor`; dvbsnoop labels it "General Instrument" | `hypothesized` (sourced candidate; see note below) |
| PSI/SI table-id set | PAT `0x00`, CAT `0x01`, PMT `0x02`, ECM40 `0x40`, ECM41 `0x41`, EMM95 `0x95` | `hypothesized` |
| `CA_descriptor` (PMT) | Tag `0x09`: `CA_system_id(16)`, `111`+`CA_PID(13)`, optional private bytes (empty by default; see below) — points a program/stream at its ECM PID | `hypothesized` |
| CAT on PID 1 | Table `0x01`; indexes the EMM streams and their provider IDs | `hypothesized` |
| EMM carriage on cable | EMMs are delivered **out-of-band** on cable (in-band on satellite) — the Oob OOB stream carries them | `hypothesized` |

The one cited GI `CA_descriptor` decode (corpus item 12, the 33c3 talk
`33c3-8127`; the slide locator is not yet recorded in the repo and should be
added on re-check) had `descriptor_length = 4`, exactly `CA_system_id` +
`CA_PID`, i.e. **no private bytes**. Private-byte carriage is an Oob
construction option, `hypothesized`, empty by default (aligning with
[dcii-si.md](dcii-si.md)). Do not encode private bytes as evidenced GI
structure. Whether a box would reject private bytes at `d 16` is untested
here; treat it as an unverified risk, not an observed behavior.

**`CA_system_id` note — do not flip here.** The DCII PSI/SI chapter
([dcii-si.md](dcii-si.md)) and the `oob_mux::psi` constant currently carry
`0x0700` as a `hypothesized` placeholder ("DVB registry as commonly
reported; no value in hand"). Corpus item 12 provides a **sourced
candidate, `0x4749`**, from a clean dvbsnoop decode of a real DCII cable
capture — stronger provenance than the placeholder. Per the ADR-001 §4
order, `0x4749` is expected to supersede `0x0700`, but that change is a
**separate, gated dev PR** owned by the `CA_system_id` sourcing issue
(#56) and still requires an Oob-box d 15 / d 16 capture to reach
`verified`. This chapter records the candidate and its provenance; it does
**not** change the `0x0700` value in `dcii-si.md` or in `oob_mux::psi`.

## 2. Key hierarchy and cadence — roles and lifetimes only

DCII uses a four-level key chain, each level delivered to or derived by the
box from the level above. **The key values are secrets and never enter the
repo (§5); only the roles, lifetimes and the delivery/rotation cadence are
design inputs.**

| Role | Lifetime | Delivered / derived by | Status |
| --- | --- | --- | --- |
| Seed keys (×3) | Box lifespan | Injected at personalization (factory) — **inaccessible** | `hypothesized` |
| Category key | ~1 month (epoch) | Carried by **EMM95**, encrypted under the seed keys | `hypothesized` |
| Program key | 1 day or 1 PPV event | Carried by **ECM41**, encrypted under the category key | `hypothesized` |
| Working key | 133 ms – 1 s | Derived from the **ECM40** incrementing frame count, under the program key | `hypothesized` |

- **Epochs and key-select.** Category and program keys roll by epoch with a
  **current + next** key-select carried in the messages, so a box can hold
  both and transition without losing service.
- **Working-key cadence.** ECM40s arrive in sequential triplets about
  133 ms apart; the working key is a function of the ECM40 frame count.
  ECM40 and ECM41 are emitted as a **pair** per channel.

### Trust-root boundary (what Oob can and cannot reproduce)

The headend's job is **key delivery and rotation**: emitting
EMM95 / ECM41 / ECM40 with the correct epochs, cadence, per-UA addressing,
and — for the real encrypted tier — the encrypted key blobs the SEM
produces. The headend does **not** perform secure-element key
**injection**: personalization and seed-key injection are factory-only and
inaccessible (the personalization ROM path is disabled after manufacture).
Oob can therefore reproduce message structure, cadence, addressing and
epoch rotation; it cannot personalize a box, originate seed keys, or hold
any key value.

## 3. Message families

Field roles and ordering, enough to build an encoder. **No raw captured
hex, key payloads or Unit Addresses are reproduced**; encrypted-key fields
are named as opaque blobs.

### ECM40 — Working-Key ECM — table `0x40` — `hypothesized` structure / `hypothesized` encoder

Field roles, in capture order: `table_id 0x40`; section/length framing;
**Service ID** (per channel); **frame count** (incrementing — drives the
working key); **hardware crypto-mode selector**; trailing **checksum**
byte. Emitted in sequential triplets ~133 ms apart and paired with ECM41.

### ECM41 — Program-Key ECM — table `0x41` — `hypothesized` structure / `hypothesized` encoder

Field roles: `table_id 0x41`; framing; **8-byte initialization vector**;
**Provider ID**; **hardware crypto select**; **Service ID**; **category
epoch**; **bitmap** (one bit flags PPV); **program epoch**; **count of
acceptable tiers** followed by that many **4-byte tier entries**;
**validity start**; **valid period** (about 24 h); and an **8-byte
encrypted program-key blob** (opaque; secret — not reproduced).

### EMM95 — Entitlement / Category EMM — table `0x95` — `hypothesized` structure / `hypothesized` encoder

Multi-part (observed in four parts). **Individually addressed** to one box
by its **40-bit Unit Address** — there are **no group keys; every EMM is
per-UA**. Field roles per part: `table_id 0x95`; length; **Unit Address**;
**EMM part number**; **bitmap** of the data items present;
**key-select (current + next)**; **hardware crypto select**;
**geographic-location** field; **category epoch**; **Provider ID**; an
**8-byte encrypted category-key blob (current + next)** (opaque; secret —
not reproduced); and **tier-bank** data. It carries the box's access
rights and the encrypted category key.

### ACP local SPI access-rights protocol — `hypothesized` structure — **NOT headend-emitted**

This is the command/response bus between the box controller (SPI master)
and the ACP secure element (SPI slave): the host sends `80 <cmd> …`, the
ACP replies `55 <cmd> …`. The reply's type byte distinguishes tuner 0
(`00`) / tuner 1 (`80`), a prompted response, and an **unprompted `0x87`
response on an access-rights change** (for example a channel change). The
bus exposes the category epoch, Provider ID, Unit Address, key-selects and
subscription tiers, with an encryption-mode indicator (fixed-key vs full)
in the tier/response data.

**Oob neither builds nor emits this bus** — it is internal to the box's
secure element (a box-internal SPI). It is documented here only as the
box-side **oracle**: the real-world analog of the d 15 / d 16 diagnostics
that QA uses to confirm an Oob-emitted family was accepted. Treating ACP as
a message family to emit would be a scope error.

## 4. Crate ownership and PID / insertion-budget implications

| Family / artifact | Owning crate | Responsibility |
| --- | --- | --- |
| ECM40, ECM41, EMM95 definitions (layouts, status labels, evidence backrefs) | **oob-msg** (W3) | The O4 protocol-doc-as-code: each family at `hypothesized`, promoted to `verified` only on a capture (F6-6 / F6-7) |
| EMM / ECM **emission paths** — per-UA EMM cycling, category-epoch management, ECM-pair cadence | **oob-dac** | Drives *when* and *to whom* families are emitted; the KLS/ukl lifecycle and category epochs |
| PSI/SI assembly — `CA_descriptor` in PMT, CAT on PID 1 indexing the EMM streams, PID plan | **oob-mux** (W2) | Already builds PAT/PMT/CAT ([dcii-si.md](dcii-si.md)); this chapter sources the `CA_system_id` candidate (§1) and the CAT→EMM indexing role |
| Encrypted-tier scrambling **rotation cadence** → SEM | **oob-playout → SEM (GigE)** | For the Fixed-PK tier (O3): the cadence is a headend concern; the FPK constant itself stays in the git-crypt tier / O3, never here |

**PID-plan and insertion-bandwidth implications** (AGENTS.md §8, F6-3):

- EMMs ride the **EMM PID**; the **EMM Provider ID is `0x0000` / `0x0001`**
  for an Oob install (the §8 guardrail). The deck's capture carries a
  foreign plant's provider ID, which is not ours and is not reproduced.
- **EMM95 is strictly per-UA (no group keys).** EMM throughput therefore
  scales with fleet size × (category-epoch rotation + access-rights
  changes), a real load on the **~1000 Kbps insertion budget shared across
  traffic classes C0–C5**: per-UA EMM cycling competes with the ECM pairs
  and the PSI carousels. oob-dac's class budgeting (F6-3) must account for
  it, with an explicit budget line when the encrypted tier is scheduled
  (post-G1).
- ECM40 / ECM41 pairs are per-channel and high-rate (working key ≤ 1 s);
  their carousel cadence is a C-class budget input, not a free insertion.

## 5. Explicit non-goals (out of reach or secret)

These are **not** buildable headend inputs and must not be documented as
constructible here — they are either box-silicon secrets or factory-only:

1. **DES hardware-customization mask values** (the key/data XOR taps). The
   deck shows the *attack* that recovers them; the mask values are box
   secrets (§5), and they are not something the headend emits.
2. **Working-key derivation internals** — the exact frame-count →
   working-key DES-custom transform.
3. **The personalization process and seed-key injection** — factory-only
   and inaccessible.
4. **Any actual key value** (seed, category, program, working) and any
   Unit Address — RAM-only, secret, never in the repo (§5).

A capability that needs any of the above is bounded by the DCII secrets
tier, not by effort.

## References

- Corpus item 12 (Gerlinsky / @akacastor DCII/ACP reverse-engineering
  slides) — cited, not reproduced; provenance and vintage in "Source and
  provenance" above.
- Architect design note: issue #76 (the DCII construction spec this chapter
  renders).
- [DCII PSI/SI](dcii-si.md) — the PAT/PMT/CAT assemblers and the current
  `0x0700` `CA_system_id` placeholder this chapter's `0x4749` candidate is
  expected to supersede (pending issue #56 and a box capture).
- refs F4-6 (DCII-flavored PSI/SI), O10 (patent / box-as-oracle pass),
  O3 (FPK constant and scrambling flavor).
