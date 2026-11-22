#!/usr/bin/env python3
"""Install the Oob agent command guard for tools that only read user-level hooks.

Claude Code, Cursor, Gemini CLI, Codex, and GitHub Copilot pick the guard
up from files committed in this repo (AGENTS.md §14). Kimi Code reads hooks
only from ~/.kimi-code/config.toml, so contributors who use Kimi run:

    uv run --python 3.12 python3 ops/install-agent-guards.py          # install/update
    uv run --python 3.12 python3 ops/install-agent-guards.py --check  # report only

(Python 3.11+ is required for tomllib; the project pins 3.12.)

It appends the [[hooks]] entry from ops/agent-guard.kimi.toml once, keeps a
backup of the original file, and refuses to touch a config that does not
parse. Run it yourself: it edits a file in your home directory.
"""

from __future__ import annotations

import pathlib
import re
import shutil
import sys

if sys.version_info < (3, 11):
    sys.exit(f"{__file__}: needs Python 3.11+ for tomllib (this is "
             f"{sys.version.split()[0]}); the project pins 3.12 (.python-version) — run it as "
             "`uv run --python 3.12 python3 ops/install-agent-guards.py`.")

import tomllib  # noqa: E402

REPO = pathlib.Path(__file__).resolve().parent.parent
SNIPPET = REPO / "ops" / "agent-guard.kimi.toml"
KIMI_CONFIG = pathlib.Path.home() / ".kimi-code" / "config.toml"
MARKER = "ops/agent-guard.py"
# An existing guard entry: from its own leading comment block (the one that
# names the Oob agent command guard), else the [[hooks]] header, through its
# timeout line; unrelated comments above it are kept. Used to replace an outdated entry in place.
OLD_BLOCK = re.compile(
    r"\n*(?:#[^\n]*Oob agent command guard[^\n]*\n(?:#[^\n]*\n|[ \t]*\n)*)?\[\[hooks\]\]\n(?:(?!\[)[^\n]*\n)*?command = '[^\n]*ops/agent-guard\.py[^\n]*'\n"
    r"(?:(?!\[)[^\n]*\n)*?timeout = \d+[ \t]*\n?")


# A leftover guard comment block with no entry under it (QA-29).
ORPHAN_COMMENTS = re.compile(r"\n*#[^\n]*Oob agent command guard[^\n]*\n(?:#[^\n]*\n)*")
MARKER_LINE = "Oob agent command guard"


def snippet_command() -> str:
    return tomllib.loads(SNIPPET.read_text())["hooks"][0]["command"]


def kimi_status() -> str:
    if not KIMI_CONFIG.parent.exists():
        return "absent"
    if not KIMI_CONFIG.exists():
        return "no-config"
    text = KIMI_CONFIG.read_text()
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        return f"unparseable: {exc}"
    for hook in data.get("hooks", []):
        if hook.get("event") == "PreToolUse" and MARKER in hook.get("command", ""):
            current = hook.get("command") == snippet_command()
            clean = text.count(MARKER_LINE) == SNIPPET.read_text().count(MARKER_LINE)
            return "installed" if current and clean else "outdated"
    return "missing"


def install_kimi() -> int:
    status = kimi_status()
    if status == "absent":
        print("Kimi Code: not installed here (~/.kimi-code missing); nothing to do.")
        return 0
    if status == "installed":
        print(f"Kimi Code: guard hook already present in {KIMI_CONFIG}.")
        return 0
    if status.startswith("unparseable"):
        print(f"Kimi Code: {KIMI_CONFIG} does not parse ({status}); not touching it.")
        return 1
    snippet = SNIPPET.read_text()
    tomllib.loads(snippet)  # never write a snippet that doesn't parse
    if KIMI_CONFIG.exists():
        backup = KIMI_CONFIG.with_name("config.toml.bak-before-oob-guard")
        if not backup.exists():
            shutil.copy2(KIMI_CONFIG, backup)
            print(f"Kimi Code: backup written to {backup}")
        existing = KIMI_CONFIG.read_text()
    else:
        existing = ""
    if status == "outdated":
        existing, n = OLD_BLOCK.subn("\n", existing, count=1)
        existing = ORPHAN_COMMENTS.sub("\n", existing)
        if n != 1:
            print(f"Kimi Code: found an outdated guard entry in {KIMI_CONFIG} but could not "
                  "locate it safely; edit it by hand to match ops/agent-guard.kimi.toml.")
            return 1
    new_text = existing.rstrip("\n") + "\n\n" + snippet if existing else snippet
    parsed = tomllib.loads(new_text)  # the result must still parse
    guards = [h for h in parsed.get("hooks", []) if MARKER in h.get("command", "")]
    if (len(guards) != 1 or guards[0]["command"] != snippet_command()
            or new_text.count(MARKER_LINE) != snippet.count(MARKER_LINE)):
        print(f"Kimi Code: refusing to write {KIMI_CONFIG}: result would not hold exactly one current guard entry.")
        return 1
    KIMI_CONFIG.write_text(new_text)
    verb = "updated" if status == "outdated" else "installed"
    print(f"Kimi Code: guard hook {verb} in {KIMI_CONFIG}. Check with /hooks in a Kimi session.")
    return 0


def main(argv: list[str]) -> int:
    if argv[1:2] == ["--check"]:
        print(f"Kimi Code: {kimi_status()}")
        return 0
    return install_kimi()


if __name__ == "__main__":
    sys.exit(main(sys.argv))
