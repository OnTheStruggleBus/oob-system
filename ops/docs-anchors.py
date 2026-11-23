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

Python 3.12, stdlib only (AGENTS.md §10).

Usage: python3 ops/docs-anchors.py [target/book/html]
"""

from __future__ import annotations

import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

SKIP_SOURCES = {"print.html", "404.html", "toc.html"}


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
    for e in errors:
        print(f"docs-anchors: {e}", file=sys.stderr)
    if errors:
        print(f"docs-anchors: {len(errors)} broken internal link(s)/anchor(s)",
              file=sys.stderr)
        return 1
    print(f"docs-anchors: {checked} internal links and anchors OK "
          f"across {len(pages)} pages")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
