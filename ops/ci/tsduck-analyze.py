#!/usr/bin/env python3
"""CI gate `tsduck analyze` (#18, r6 F6-15; ADR-002 Option C).

Generates the fixed-length stream with `cargo run --example gen_stream`
(fresh on every run, nothing cached) and has tsduck check it as an
independent oracle for section syntax, CRC32 and continuity. Fails on:

  * a non-zero exit of tsp / tstables, or any tsp error or warning line;
  * any continuity discontinuity or duplicated packet (analyze counters and
    `-P continuity`), any invalid sync byte, any transport_error packet;
  * any section tsduck rejects (invalid CRC32 or syntax; `tstables
    --invalid-sections`);
  * any PID that is present but not referenced by the PAT, a PMT or the CAT
    (the null PID 8191 is the one allowlisted name);
  * a stream that does not carry exactly the PIDs `gen_stream` declares, or
    is missing PAT/CAT/PMT, or has the wrong length (a vacuous pass).

What this does NOT prove: tsduck decodes the generic MPEG-2 structure.
GI/DCII descriptors and DCII table layouts are private to it (it reports
them as unknown or private, and this script never parses descriptors, so
they cannot fail the gate). A green run is NOT evidence that the DCII bits
are right: d 15 / d 16 on a box remain the referee (AGENTS.md §2, r4
F4-6). Nothing here promotes any status above `hypothesized`.

The stream rate has one home, docs/facts.yaml `oob_stream.stream_rate_mb_s`.

Usage:
  ops/ci/tsduck-analyze.py             generate, analyze, exit 0 only if clean
  ops/ci/tsduck-analyze.py --stream F  analyze an existing file instead
  ops/ci/tsduck-analyze.py --self-test generate, then prove the checker
                                       rejects corrupted copies of it
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACKETS = 20_000
PKT = 188
NULL_PID = 8191
# The PIDs gen_stream declares: PAT, CAT, the PMT, four private-section
# ES PIDs (C2..C5), the EMM PID listed in the CAT, and the null PID.
EXPECTED_PIDS = {0, 1, 256, 257, 258, 259, 260, 512, NULL_PID}
# Named, justified tolerances (the discipline of ops/docs-facts.toml): a
# substring of a tsp stderr line -> why it is not a failure. Empty on
# purpose; add an entry with a `why`, never a blanket ignore.
ALLOW_STDERR: dict[str, str] = {}


def stream_rate_bps() -> int:
    text = (ROOT / "docs" / "facts.yaml").read_text()
    m = re.search(r"^\s*stream_rate_mb_s:\s*([0-9.]+)", text, re.M)
    if not m:
        raise SystemExit("docs/facts.yaml: oob_stream.stream_rate_mb_s not found")
    return round(float(m.group(1)) * 1_000_000)


def run(cmd: list[str]) -> tuple[int, str, str]:
    p = subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT)
    return p.returncode, p.stdout, p.stderr


def fields(line: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for part in line.rstrip(":").split(":"):
        if "=" in part:
            k, v = part.split("=", 1)
            out.setdefault(k, v)
    return out


def check(stream: Path, bps: int, packets: int | None) -> list[str]:
    """Returns the problems found; empty means clean."""
    bad: list[str] = []
    rc, out, err = run(
        ["tsp", "--bitrate", str(bps), "-I", "file", str(stream),
         "-P", "continuity", "-P", "analyze", "--normalized", "-O", "drop"]
    )
    if rc != 0:
        bad.append(f"tsp exited {rc}")
    for line in err.splitlines():
        if line.strip() and not any(k in line for k in ALLOW_STDERR):
            bad.append(f"tsp: {line.strip()}")

    pids: dict[int, dict[str, str]] = {}
    ts: dict[str, str] = {}
    unref = ""
    tables: set[int] = set()
    for line in out.splitlines():
        f = fields(line)
        if line.startswith("ts:"):
            ts = f
        elif line.startswith("unreferenced:"):
            unref = f.get("pidlist", "")
        elif line.startswith("pid:") and "pid" in f:
            pids[int(f["pid"])] = f
        elif line.startswith("table:") and "tid" in f:
            tables.add(int(f["tid"]))
    if not ts:
        return bad + ["tsp analyze produced no `ts:` summary line"]

    n = int(ts.get("packets", "0"))
    if packets is not None and n != packets:
        bad.append(f"stream has {n} packets, expected {packets}")
    for key in ("invalidsyncs", "transporterrors"):
        if ts.get(key, "?") != "0":
            bad.append(f"{key}={ts.get(key)}")
    for pid, f in sorted(pids.items()):
        for key in ("discontinuities", "duplicated", "invalidscrambling"):
            if f.get(key, "?") != "0":
                bad.append(f"pid {pid}: {key}={f.get(key)}")
    extra = [p for p in unref.split(",") if p and int(p) != NULL_PID]
    if extra:
        bad.append(f"unreferenced PIDs (not in PAT/PMT/CAT): {','.join(extra)}")
    if set(pids) != EXPECTED_PIDS:
        bad.append(f"PIDs present {sorted(pids)} != declared {sorted(EXPECTED_PIDS)}")
    if not {0, 1, 2} <= tables:
        bad.append(f"PAT/CAT/PMT not all seen (table ids {sorted(tables)})")

    # tsduck drops sections it cannot validate unless asked to dump them.
    cmd = ["tstables", "--all-sections", "--invalid-sections"]
    for p in sorted(pids):
        if p != NULL_PID:
            cmd += ["--pid", str(p)]
    rc, out, err = run(cmd + [str(stream)])
    if rc != 0:
        bad.append(f"tstables exited {rc}")
    for line in (out + err).splitlines():
        if "Invalid section" in line:
            bad.append(f"tstables: {line.strip()}")
    return bad


def generate(dest: Path, bps: int) -> None:
    rc, _, err = run(
        ["cargo", "run", "--quiet", "--locked", "-p", "oob-mux", "--example", "gen_stream",
         "--", "--out", str(dest), "--packets", str(PACKETS), "--rate-bps", str(bps)]
    )
    if rc != 0:
        raise SystemExit(f"gen_stream failed:\n{err}")
    if dest.stat().st_size != PACKETS * PKT:
        raise SystemExit(f"gen_stream wrote {dest.stat().st_size} bytes")


def mutate(data: bytes, kind: str) -> bytes:
    d = bytearray(data)
    seen = 0
    for i in range(0, len(d), PKT):
        pid = ((d[i + 1] & 0x1F) << 8) | d[i + 2]
        pusi = bool(d[i + 1] & 0x40)
        if kind == "crc" and pid == 259 and pusi:
            d[i + 50] ^= 0x01  # a body byte of a 4096-byte section
            return bytes(d)
        if kind == "cc" and pid == 258:
            seen += 1
            if seen == 3:
                d[i + 3] = (d[i + 3] & 0xF0) | ((d[i + 3] + 3) & 0x0F)
                return bytes(d)
        if kind == "unref" and pid == NULL_PID:
            d[i + 1], d[i + 2] = 0x01, 0x23  # a PID nothing declares
            return bytes(d)
        if kind == "sync" and seen == 0 and i > 0:
            d[i] = 0x00
            return bytes(d)
    raise SystemExit(f"self-test: no packet to mutate for {kind}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--stream", type=Path)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    bps = stream_rate_bps()
    with tempfile.TemporaryDirectory() as tmp:
        stream = args.stream
        if stream is None:
            stream = Path(tmp) / "oob-gen.ts"
            generate(stream, bps)
        packets = None if args.stream else PACKETS
        problems = check(stream, bps, packets)
        for p in problems:
            print(f"FAIL: {p}")
        if problems:
            return 1
        print(f"clean: {stream.stat().st_size // PKT} packets at {bps} b/s, "
              f"PIDs {sorted(EXPECTED_PIDS)}, no CC/CRC/syntax/reference findings")
        if args.self_test:
            clean = stream.read_bytes()
            for kind in ("crc", "cc", "unref", "sync"):
                bad = Path(tmp) / f"bad-{kind}.ts"
                bad.write_bytes(mutate(clean, kind))
                found = check(bad, bps, packets)
                if not found:
                    print(f"SELF-TEST FAIL: a corrupted ({kind}) stream passed")
                    return 1
                print(f"self-test ok: {kind} rejected ({found[0][:90]})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
