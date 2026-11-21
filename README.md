# Oob — an out-of-band headend for DCT-class cable boxes

*An MVPD of one.*

**Oob** is a faithful software reproduction of a decommissioned
GI/Motorola cable television headend — the DAC 6000 controller, OM
1000/2000 out-of-band modulator, KLS/DLS code download system, and their
companion services — run as a closed home-lab RF plant so that real
DCT2000-series cable boxes boot, authorize, tune, download code, and show
their resident program guide again.

The boxes are the referee: every behavior is validated against fleet
hardware diagnostics, not assumptions. The project nickname for the lab
estate is **mini-headend**; the guide stack is **VueGuide+**; the operator
UI is **Oob Console**; the fictional operator brand is **ArchaiTel Digital
Cable**.

## Read this first

Everything human-readable lives in [`docs/`](docs/):

| Start with | What it is |
| --- | --- |
| [`docs/mini-headend Project Overview — Faithful Lab Reproduction.md`](docs/mini-headend%20Project%20Overview%20—%20Faithful%20Lab%20Reproduction.md) | Orientation — ground rules, topology, status |
| [`docs/Project Milestones & Stage Gates — mini-headend.md`](docs/Project%20Milestones%20&%20Stage%20Gates%20—%20mini-headend.md) | The operating contract: gates G0–G6, tool stack |
| [`docs/Oob Reproduction Plan — mini-headend.md`](docs/Oob%20Reproduction%20Plan%20—%20mini-headend.md) | Workstreams W1–W8, discovery methodology, tech stack |
| [`AGENTS.md`](AGENTS.md) | Working rules for AI agents in this repo |

Full document map: Project Overview §8.

## Layout

```
crates/      oob-dac, oob-mux, oob-msg, oob-cdl, oob-playout,
             oob-starfone, oob-guide (Rust workspace)
flowgraphs/  GNU Radio OOB TX / STARVUE RX
lab/         discovery harness + box captures (git-crypt tier)
ops/         lineup.yaml, units.yaml, CI (nightly canary)
docs/        the mdBook source — protocol, runbook, specs, reviews
```

## Status

Gate **G0 passed** (documentation base); G0.5 (PHY ready) is next, with
adversarial reviews r3/r4 awaiting disposition as its first agenda item.
The SEM V8 (QAM + MediaCipher FPK encryption) is period hardware already
in the estate; everything else is software we write, deployed as
rootless Podman containers on a Debian 13 host.

## Intent

Closed plant only — coax, taps, and terminators on a bench. Self-authored
content only. Self-owned hardware only. The output is working history,
plus a public protocol document so the next person who finds a DCT2000 at
a thrift store can bring it back to life too.

License: GPLv3 (see [LICENSE](LICENSE)).
