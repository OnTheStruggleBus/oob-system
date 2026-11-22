#!/usr/bin/env python3
"""CI check: every committed agent hook is wired, enabled, and actually blocks.

Run by the `agent-guard` CI job (AGENTS.md §14). For each tool's hook file
it confirms the expected event and matcher, that no hook-disabling switch
is present, that every launcher is the same canonical string, and then
runs the launcher exactly as the tool would, with that tool's payload
shape: a merge must exit 2 and a harmless command must exit 0.

    python3 ops/check-agent-hooks.py
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import tomllib

REPO = pathlib.Path(__file__).resolve().parent.parent


def load_json(rel: str) -> dict:
    return json.loads((REPO / rel).read_text())


def claude() -> str:
    d = load_json(".claude/settings.json")
    [entry] = [e for e in d["hooks"]["PreToolUse"] if e.get("matcher") == "Bash"]
    [hook] = entry["hooks"]
    assert hook["type"] == "command"
    return hook["command"]


def cursor() -> str:
    d = load_json(".cursor/hooks.json")
    [hook] = d["hooks"]["beforeShellExecution"]
    assert hook.get("failClosed") is True, "Cursor hook must keep failClosed: true"
    return hook["command"]


def gemini() -> str:
    d = load_json(".gemini/settings.json")
    [entry] = [e for e in d["hooks"]["BeforeTool"] if e.get("matcher") == "run_shell_command"]
    [hook] = entry["hooks"]
    assert hook["type"] == "command"
    return hook["command"]


def codex() -> str:
    d = load_json(".codex/hooks.json")
    [entry] = [e for e in d["hooks"]["PreToolUse"] if e.get("matcher") == "Bash"]
    [hook] = entry["hooks"]
    assert hook["type"] == "command"
    assert "--format codex" in hook["command"], "Codex launcher must pass --format codex"
    return hook["command"]


def copilot() -> str:
    d = load_json(".github/hooks/agent-guard.json")
    [hook] = d["hooks"]["preToolUse"]
    assert hook["type"] == "command" and hook.get("matcher") == "bash"
    return hook["bash"]


def kimi() -> str:
    d = tomllib.loads((REPO / "ops" / "agent-guard.kimi.toml").read_text())
    [hook] = d["hooks"]
    assert set(hook) == {"event", "matcher", "command", "timeout"}, "Kimi allows only these four fields"
    assert hook["event"] == "PreToolUse" and hook["matcher"] == "Bash"
    return hook["command"]


BLOCK, ALLOW = "gh pr merge 1", "git status"
CASES = {
    # name: (reader, payload builder)
    "claude": (claude, lambda c: {"tool_name": "Bash", "tool_input": {"command": c}}),
    "cursor": (cursor, lambda c: {"command": c, "cwd": str(REPO)}),
    "gemini": (gemini, lambda c: {"tool_name": "run_shell_command", "tool_input": {"command": c}}),
    "codex": (codex, lambda c: {"tool_name": "Bash", "tool_input": {"command": c}}),
    "copilot": (copilot, lambda c: {"toolName": "bash", "toolArgs": json.dumps({"command": c})}),
    "kimi": (kimi, lambda c: {"hook_event_name": "PreToolUse", "tool_name": "Bash",
                              "tool_input": {"command": c}}),
}


def run(launcher: str, payload: dict) -> int:
    return subprocess.run(["sh", "-c", launcher], input=json.dumps(payload), text=True,
                          cwd=REPO, capture_output=True, timeout=30).returncode


def main() -> int:
    failures: list[str] = []
    for rel in [".claude/settings.json", ".cursor/hooks.json", ".gemini/settings.json",
                ".codex/hooks.json", ".github/hooks/agent-guard.json", "ops/agent-guard.kimi.toml"]:
        if "disableallhooks" in (REPO / rel).read_text().lower():
            failures.append(f"{rel}: contains disableAllHooks")
    launchers: dict[str, str] = {}
    for name, (reader, build) in CASES.items():
        try:
            launcher = reader()
        except (AssertionError, KeyError, ValueError) as exc:
            failures.append(f"{name}: hook not wired as expected ({exc!r})")
            continue
        launchers[name] = launcher
        b, a = run(launcher, build(BLOCK)), run(launcher, build(ALLOW))
        if b != 2:
            failures.append(f"{name}: '{BLOCK}' exited {b}, expected 2 (blocked)")
        if a != 0:
            failures.append(f"{name}: '{ALLOW}' exited {a}, expected 0 (allowed)")
    canonical = {n: l.replace(" --format codex", "") for n, l in launchers.items()}
    if len(set(canonical.values())) > 1:
        failures.append("launchers differ between tools: " + ", ".join(sorted(canonical)))
    for f in failures:
        print(f"::error::{f}")
    print(f"check-agent-hooks: {len(CASES)} tools, {len(failures)} problem(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
