#!/usr/bin/env python3
"""Check the prose against docs/facts.yaml (r6 F6-13, wired by r5 F5-10).

Facts live in docs/facts.yaml; where to look, which contexts are
legitimate, and the allowlist live in ops/docs-facts.toml (ARCH-5).

The prose is read one *unit* at a time: a Markdown paragraph, list item or
table row, or a run of Rust `//!` / `///` doc-comment lines (ARCH-2). A unit
fails a rule when it contradicts a fact; every rule has a context escape and
every finding can be allowlisted (`[[allow]]`: rule + file + a substring of
the unit + why, all required).

Rules, in full:

  oob-rate        A unit mentions the not-our-rate figure
                  (`not_our_rate.scte_55_2_mode_b_mb_s`, 1.544) without a
                  55-2 context marker (`[oob_rate] context_ok`, e.g. "55-2",
                  "Mode B", "Host-POD"). Oob's OOB stream is
                  `oob_stream.stream_rate_mb_s`.
  bare-tag        A unit about releases/tags quotes a hunt carrier as a
                  bare tag (`75.25` in backticks) instead of `rel/75.25`.
  tag-order       A unit listing two or more `rel/<carrier>` tags lists
                  them out of box scroll order (`release_tags`).
  brand-skin      A unit names `naming.superseded_brand` without a
                  supersession marker (`[brand_skin] context_ok`).
  disposition     A unit says a finding is pending/awaiting disposition
                  (`[disposition] phrases`) and names an F-ID or round
                  (F4-1, r4) that `review_dispositions` records as accepted,
                  without saying so (`[disposition] context_ok`).

Files matching `historical` (the review records, which quote the
superseded prose they found) are not scanned. A `scan` or `historical`
pattern that matches no file is an error (a typo must not silently stop
the scan), and so is a malformed or stale allowlist entry.

facts.yaml is read with a small YAML-subset loader (block maps and lists,
flow [..]/{..}, quoted and plain scalars, comments) because the check is
stdlib-only (AGENTS.md §10); the subset is documented in the facts.yaml
header. Anything outside it is a loud error, never a silent misread.

Usage: python3 ops/docs-facts.py
"""

from __future__ import annotations

import fnmatch
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FACTS = ROOT / "docs" / "facts.yaml"
CONFIG = ROOT / "ops" / "docs-facts.toml"
RULES = ("oob-rate", "bare-tag", "tag-order", "brand-skin", "disposition")


# --------------------------------------------------------------------------
# Minimal YAML-subset loader
# --------------------------------------------------------------------------

class YamlError(Exception):
    pass


def _strip_comment(s: str) -> str:
    quote = None
    for i, c in enumerate(s):
        if quote:
            if c == quote:
                quote = None
        elif c in "\"'":
            quote = c
        elif c == "#" and (i == 0 or s[i - 1] in " \t"):
            return s[:i].rstrip()
    return s.rstrip()


def _split_top(s: str) -> list[str]:
    parts, depth, quote, cur = [], 0, None, ""
    for c in s:
        if quote:
            quote = None if c == quote else quote
        elif c in "\"'":
            quote = c
        elif c in "[{":
            depth += 1
        elif c in "]}":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(cur)
            cur = ""
            continue
        cur += c
    if cur.strip():
        parts.append(cur)
    return [p.strip() for p in parts]


_KEY = re.compile(r"""^("[^"]*"|'[^']*'|[^\s"'#\-\[{][^:]*?|-[^\s:][^:]*?)\s*:(?:\s+|$)(.*)$""")


def _scalar(s: str):
    s = s.strip()
    if not s:
        return None
    if s[0] in "|>&*!":
        raise YamlError(f"unsupported YAML syntax: {s!r}")
    if s[0] == '"':
        return json.loads(s)
    if s[0] == "'":
        if not s.endswith("'"):
            raise YamlError(f"unterminated string: {s!r}")
        return s[1:-1].replace("''", "'")
    if s[0] == "[":
        if not s.endswith("]"):
            raise YamlError(f"unterminated flow list: {s!r}")
        return [_scalar(p) for p in _split_top(s[1:-1])]
    if s[0] == "{":
        if not s.endswith("}"):
            raise YamlError(f"unterminated flow map: {s!r}")
        out = {}
        for p in _split_top(s[1:-1]):
            m = _KEY.match(p)
            if not m:
                raise YamlError(f"bad flow-map entry: {p!r}")
            out[_scalar(m.group(1))] = _scalar(m.group(2))
        return out
    if s in ("true", "false"):
        return s == "true"
    if s in ("null", "~"):
        return None
    if re.fullmatch(r"-?\d+", s):
        return int(s)
    if re.fullmatch(r"-?\d+\.\d+", s):
        return float(s)
    return s


def load_yaml(text: str):
    lines: list[list] = []
    for n, raw in enumerate(text.splitlines(), 1):
        if "\t" in raw[: len(raw) - len(raw.lstrip())]:
            raise YamlError(f"line {n}: tab indentation")
        body = _strip_comment(raw)
        if not body.strip():
            continue
        if body.strip() == "---":
            raise YamlError(f"line {n}: multi-document YAML unsupported")
        lines.append([len(body) - len(body.lstrip()), body.strip(), n])
    pos = 0

    def block(ind: int):
        if lines[pos][1].startswith("- ") or lines[pos][1] == "-":
            return seq(ind)
        return mapping(ind)

    def mapping(ind: int) -> dict:
        nonlocal pos
        out: dict = {}
        while pos < len(lines) and lines[pos][0] == ind and not lines[pos][1].startswith("-"):
            _, body, n = lines[pos]
            m = _KEY.match(body)
            if not m:
                raise YamlError(f"line {n}: expected 'key: value', got {body!r}")
            key, rest = _scalar(m.group(1)), m.group(2)
            pos += 1
            if rest:
                out[key] = _scalar(rest)
            elif pos < len(lines) and (lines[pos][0] > ind or (
                    lines[pos][0] == ind and lines[pos][1].startswith("-"))):
                out[key] = block(lines[pos][0])
            else:
                out[key] = None
        if pos < len(lines) and lines[pos][0] > ind:
            raise YamlError(f"line {lines[pos][2]}: unexpected indentation")
        return out

    def seq(ind: int) -> list:
        nonlocal pos
        out: list = []
        while pos < len(lines) and lines[pos][0] == ind and (
                lines[pos][1].startswith("- ") or lines[pos][1] == "-"):
            _, body, n = lines[pos]
            item = body[1:].lstrip()
            if not item:
                pos += 1
                out.append(block(lines[pos][0]) if pos < len(lines) and lines[pos][0] > ind else None)
            elif _KEY.match(item) and not item.startswith(("[", "{", '"', "'")):
                # "- key: value" opens a mapping whose keys align with `key`.
                lines[pos] = [ind + len(body) - len(item), item, n]
                out.append(mapping(lines[pos][0]))
            else:
                out.append(_scalar(item))
                pos += 1
        return out

    if not lines:
        return {}
    result = block(lines[0][0])
    if pos != len(lines):
        raise YamlError(f"line {lines[pos][2]}: could not parse")
    return result


# --------------------------------------------------------------------------
# Prose units
# --------------------------------------------------------------------------

_ITEM = re.compile(r"^\s*([-*+]|\d+[.)])\s")


def units(text: str, offset: int = 0):
    """Yield (first_line_number, unit_text): paragraphs, list items, table rows.

    Fenced code blocks are skipped: they are code, not claims.
    """
    buf: list[str] = []
    start = 0
    fenced = False

    def flush():
        nonlocal buf
        if buf:
            yield start + offset, " ".join(s.strip() for s in buf)
        buf = []

    for n, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("```"):
            yield from flush()
            fenced = not fenced
            continue
        if fenced:
            continue
        if not line.strip() or line.lstrip().startswith("#"):
            yield from flush()
            if line.strip():
                yield n + offset, line.strip()
            continue
        if line.lstrip().startswith("|") or _ITEM.match(line):
            yield from flush()
            if line.lstrip().startswith("|"):
                yield n + offset, line.strip()
                continue
        if not buf:
            start = n
        buf.append(line)
    yield from flush()


_RS_DOC = re.compile(r"^\s*//[/!](?!/)\s?(.*)$")


def rust_units(text: str):
    """Yield (line, unit) for each run of consecutive `//!` / `///` lines."""
    buf: list[str] = []
    start = 0
    for n, line in enumerate(text.splitlines(), 1):
        m = _RS_DOC.match(line)
        if m:
            if not buf:
                start = n
            buf.append(m.group(1))
            continue
        if buf:
            yield from units("\n".join(buf), offset=start - 1)
            buf = []
    if buf:
        yield from units("\n".join(buf), offset=start - 1)


# --------------------------------------------------------------------------
# Config validation
# --------------------------------------------------------------------------

class ConfigError(Exception):
    pass


def _expand(patterns: list[str], what: str) -> list[Path]:
    out: list[Path] = []
    for pat in patterns:
        hits = [p for p in ROOT.glob(pat) if p.is_file()]
        if not hits:
            raise ConfigError(f"docs-facts.toml: {what} pattern {pat!r} matches no files")
        out.extend(hits)
    return out


def _validate_allow(allow: list) -> None:
    for i, a in enumerate(allow, 1):
        if not isinstance(a, dict):
            raise ConfigError(f"docs-facts.toml: [[allow]] #{i} is not a table")
        for key in ("rule", "file", "contains", "why"):
            v = a.get(key)
            if not isinstance(v, str) or not v.strip():
                raise ConfigError(f"docs-facts.toml: [[allow]] #{i} needs a non-empty `{key}`")
        if a["rule"] not in RULES:
            raise ConfigError(f"docs-facts.toml: [[allow]] #{i} has unknown rule "
                              f"{a['rule']!r} (one of {', '.join(RULES)})")
        if not (ROOT / a["file"]).is_file():
            raise ConfigError(f"docs-facts.toml: [[allow]] #{i} names missing file {a['file']!r}")
        extra = set(a) - {"rule", "file", "contains", "why"}
        if extra:
            raise ConfigError(f"docs-facts.toml: [[allow]] #{i} has unknown keys {sorted(extra)}")


def _accepted_rounds(facts: dict) -> dict[str, str]:
    """Map accepted round ('r4') -> its F-ID prefix ('F4-')."""
    out = {}
    for rnd, d in (facts.get("review_dispositions") or {}).items():
        if isinstance(d, dict) and d.get("state") == "accepted":
            m = re.match(r"(F\d+-)", str(d.get("findings", "")))
            if not m:
                raise ConfigError(f"facts.yaml: review_dispositions.{rnd}.findings unreadable")
            out[str(rnd)] = m.group(1)
    return out


# --------------------------------------------------------------------------
# Rules
# --------------------------------------------------------------------------

def check(facts: dict, cfg: dict) -> list[str]:
    bad_rate = facts["not_our_rate"]["scte_55_2_mode_b_mb_s"]
    our_rate = facts["oob_stream"]["stream_rate_mb_s"]
    problems: list[str] = []
    if bad_rate == our_rate:
        problems.append("facts.yaml: oob_stream.stream_rate_mb_s equals not_our_rate")
    rate_re = re.compile(rf"(?<![\d.]){re.escape(str(bad_rate))}(?!\d)")
    carriers = [str(c) for c in facts["release_tags"]["carriers_in_box_scroll_order"]]
    bare_re = re.compile(r"`(" + "|".join(re.escape(c) for c in carriers) + r")`")
    rel_re = re.compile(r"rel/(" + "|".join(re.escape(c) for c in carriers) + r")(?![\d])")
    tagish = re.compile(r"\b(releases?|tag(s|ged|ging)?)\b", re.I)
    brand = str(facts["naming"]["superseded_brand"])
    rate_ok = [s.lower() for s in cfg["oob_rate"]["context_ok"]]
    brand_ok = [s.lower() for s in cfg["brand_skin"]["context_ok"]]
    disp_phrases = [s.lower() for s in cfg["disposition"]["phrases"]]
    disp_ok = [s.lower() for s in cfg["disposition"]["context_ok"]]
    accepted = _accepted_rounds(facts)
    ref_re = re.compile(
        r"\b(?:(" + "|".join(re.escape(p) for p in accepted.values()) + r")\d+"
        r"|(" + "|".join(re.escape(r) for r in accepted) + r"))\b"
    ) if accepted else None
    allow = cfg.get("allow") or []
    _validate_allow(allow)
    used_allow: set[int] = set()

    files = _expand(cfg["scan"], "scan")
    historical = cfg.get("historical") or []
    _expand(historical, "historical")
    for path in sorted(set(files)):
        rel = path.relative_to(ROOT).as_posix()
        if any(fnmatch.fnmatch(rel, h) for h in historical):
            continue
        text = path.read_text(encoding="utf-8")
        for line, unit in (rust_units(text) if path.suffix == ".rs" else units(text)):
            low = unit.lower()
            hits: list[tuple[str, str]] = []
            if rate_re.search(unit) and not any(k in low for k in rate_ok):
                hits.append(("oob-rate", f"{bad_rate} Mb/s outside a 55-2 context "
                             f"(Oob's OOB stream is {our_rate} Mb/s)"))
            if tagish.search(unit):
                for m in bare_re.finditer(unit):
                    hits.append(("bare-tag", f"release tag `{m.group(1)}` should be "
                                 f"`rel/{m.group(1)}`"))
            order = [carriers.index(c) for c in rel_re.findall(unit)]
            if len(order) >= 2 and order != sorted(order):
                hits.append(("tag-order", "rel/<carrier> tags out of box scroll order"))
            if brand.lower() in low and not any(k in low for k in brand_ok):
                hits.append(("brand-skin", f"names {brand} as if it were the brand skin "
                             "(superseded by the per-deployment config slot)"))
            if (ref_re and any(p in low for p in disp_phrases)
                    and not any(k in low for k in disp_ok)):
                ref = ref_re.search(unit)
                if ref:
                    hits.append(("disposition", f"calls {ref.group(0)} undispositioned, but "
                                 "facts.yaml review_dispositions records its round as accepted"))
            for rule, msg in hits:
                allowed = False
                for i, a in enumerate(allow):
                    if a["rule"] == rule and a["file"] == rel and a["contains"] in unit:
                        used_allow.add(i)
                        allowed = True
                if not allowed:
                    problems.append(f"{rel}:{line}: [{rule}] {msg}\n    > {unit[:160]}")
    for i, a in enumerate(allow):
        if i not in used_allow:
            problems.append(f"docs-facts.toml: stale [[allow]] entry #{i + 1} "
                            f"({a['rule']} in {a['file']}): matches nothing; remove it")
    return problems


def main() -> int:
    try:
        facts = load_yaml(FACTS.read_text(encoding="utf-8"))
        cfg = tomllib.loads(CONFIG.read_text(encoding="utf-8"))
        problems = check(facts, cfg)
    except (YamlError, ConfigError, tomllib.TOMLDecodeError,
            KeyError, TypeError, ValueError) as e:
        print(f"docs-facts: bad facts.yaml or docs-facts.toml: {type(e).__name__}: {e}",
              file=sys.stderr)
        return 1
    for p in problems:
        print(f"docs-facts: {p}", file=sys.stderr)
    if problems:
        print(f"docs-facts: {len(problems)} contradiction(s) with docs/facts.yaml. "
              "Fix the prose (or the fact, in the same commit), or allowlist a "
              "true false positive in ops/docs-facts.toml [[allow]] with a `why`.",
              file=sys.stderr)
        return 1
    print(f"docs-facts: prose agrees with docs/facts.yaml ({len(cfg['scan'])} scan patterns)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
