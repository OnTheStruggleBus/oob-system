#!/usr/bin/env python3
"""Check every internal link *and anchor* in the built mdBook HTML (r5 F5-10).

mdbook-linkcheck 0.7.7 fails the build on links to missing files but does
not validate `#fragments`. This closes that gap against the real output:
it reads the HTML mdBook generated, collects every element `id`, and checks
that each relative `href="page.html#frag"` (or `#frag`) points at an
existing file and an existing id. Because the ids come from mdBook's own
heading slugger, there is no slug rule to keep in sync.

External links (anything with a scheme, or protocol-relative `//`) are
deliberately skipped: no network in a required check. `print.html`
(mdBook's all-in-one page, which rewrites anchors) and `404.html` are
skipped as sources; they are generated from the same chapters.

Second pass (QA-3): Markdown that lives outside the book -- the root
README.md and AGENTS.md, and the role files in .github/agents/ and
.claude/agents/ -- is checked at the source. Every relative link must name
an existing file (percent-decoded, resolved against the linking file), and
a `#fragment` into a Markdown file must match one of that file's headings
under GitHub's slug rule (lowercase; drop everything but letters, digits,
`_`, `-` and spaces; spaces become `-`; repeats get `-1`, `-2`, ...).

Python 3.12, stdlib only (AGENTS.md §10).

Usage: python3 ops/docs-anchors.py [target/book/html]
"""

from __future__ import annotations

import re
import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

SKIP_SOURCES = {"print.html", "404.html", "toc.html"}
ROOT = Path(__file__).resolve().parent.parent
# Markdown outside the book, checked at the source. Each pattern must match.
SOURCE_GLOBS = ["README.md", "AGENTS.md", ".github/agents/*.md", ".claude/agents/*.md"]
_LINK = re.compile(r"\]\(((?:[^()\s]|\([^()\s]*\))+)(?:\s+\"[^\"]*\")?\)")
_CODE_SPAN = re.compile(r"`[^`]*`")


def github_slugs(md: str) -> set[str]:
    slugs: set[str] = set()
    seen: dict[str, int] = {}
    fenced = False
    for line in md.splitlines():
        if line.lstrip().startswith("```"):
            fenced = not fenced
            continue
        m = None if fenced else re.match(r"^#{1,6}\s+(.*?)\s*#*\s*$", line)
        if not m:
            continue
        text = re.sub(r"[`*_]|\[([^\]]*)\]\([^)]*\)", lambda x: x.group(1) or "", m.group(1))
        slug = re.sub(r"[^\w\- ]", "", text.lower()).replace(" ", "-")
        n = seen.get(slug, 0)
        seen[slug] = n + 1
        slugs.add(slug if n == 0 else f"{slug}-{n}")
    return slugs


def check_sources() -> tuple[int, list[str]]:
    errors: list[str] = []
    checked = 0
    files: list[Path] = []
    for pat in SOURCE_GLOBS:
        hits = sorted(p for p in ROOT.glob(pat) if p.is_file())
        if not hits:
            errors.append(f"source pattern {pat!r} matches no files")
        files.extend(hits)
    for src in files:
        text = src.read_text(encoding="utf-8")
        fenced = False
        for n, line in enumerate(text.splitlines(), 1):
            if line.lstrip().startswith("```"):
                fenced = not fenced
                continue
            if fenced:
                continue
            for m in _LINK.finditer(_CODE_SPAN.sub("", line)):
                href = m.group(1).strip("<>")
                parts = urlsplit(href)
                if parts.scheme or href.startswith("//"):
                    continue
                checked += 1
                where = f"{src.relative_to(ROOT)}:{n}"
                target = src if not parts.path else (src.parent / unquote(parts.path)).resolve()
                if not target.exists():
                    errors.append(f"{where}: link to missing file {href!r}")
                    continue
                frag = unquote(parts.fragment)
                if frag and target.suffix == ".md":
                    if frag not in github_slugs(target.read_text(encoding="utf-8")):
                        errors.append(f"{where}: anchor #{frag} matches no heading in "
                                      f"{target.relative_to(ROOT)}")
    return checked, errors


class Page(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.ids: set[str] = set()
        self.hrefs: list[tuple[int, str]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        a = dict(attrs)
        if a.get("id"):
            self.ids.add(a["id"])
        if tag == "a" and a.get("name"):
            self.ids.add(a["name"])
        if tag == "a" and a.get("href"):
            self.hrefs.append((self.getpos()[0], a["href"]))


def parse(path: Path) -> Page:
    p = Page()
    p.feed(path.read_text(encoding="utf-8"))
    return p


def main(argv: list[str]) -> int:
    root = Path(argv[1] if len(argv) > 1 else "target/book/html").resolve()
    if not (root / "index.html").is_file():
        print(f"docs-anchors: no built book at {root} (run `mdbook build` first)",
              file=sys.stderr)
        return 1
    pages = {p.resolve(): parse(p) for p in root.rglob("*.html")}
    errors: list[str] = []
    checked = 0
    for src, page in sorted(pages.items()):
        if src.name in SKIP_SOURCES:
            continue
        for line, href in page.hrefs:
            parts = urlsplit(href)
            if parts.scheme or href.startswith("//"):
                continue  # external: not checked by design
            target = src if not parts.path else (src.parent / unquote(parts.path)).resolve()
            checked += 1
            where = f"{src.relative_to(root)}:{line}"
            if target.is_dir():
                target = target / "index.html"
            if not target.exists():
                errors.append(f"{where}: link to missing file {href!r}")
                continue
            frag = unquote(parts.fragment)
            if frag and target.suffix == ".html":
                tpage = pages.get(target)
                if tpage is None or frag not in tpage.ids:
                    errors.append(f"{where}: anchor #{frag} not found in "
                                  f"{target.relative_to(root)} ({href!r})")
    src_checked, src_errors = check_sources()
    errors.extend(src_errors)
    for e in errors:
        print(f"docs-anchors: {e}", file=sys.stderr)
    if errors:
        print(f"docs-anchors: {len(errors)} broken internal link(s)/anchor(s)",
              file=sys.stderr)
        return 1
    print(f"docs-anchors: {checked} internal links and anchors OK "
          f"across {len(pages)} book pages; {src_checked} relative links OK "
          f"in root and role Markdown")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
