#!/usr/bin/env python3
"""CI check: every committed agent hook is wired, enabled, and actually blocks.

Run by the `agent-guard` CI job (AGENTS.md §14). For each tool's hook file
it confirms the exact keys (an unknown key could disable a hook), the
event and matcher, a minimum timeout in that tool's unit, that no
hook-disabling switch is present, and that every launcher is the same
canonical string. Then it runs each launcher as the tool would, with that
tool's payload shape, twice: in this checkout, and in a throwaway repo
whose checkout lacks the guard (the git fallback path). Each time a merge
must exit 2 and a harmless command must exit 0.

    python3 ops/check-agent-hooks.py
"""

from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import sys

if sys.version_info < (3, 11):
    sys.exit(f"{__file__}: needs Python 3.11+ for tomllib (this is "
             f"{sys.version.split()[0]}); the project pins 3.12 — e.g. `uv run python3 ...`.")

import tomllib  # noqa: E402

REPO = pathlib.Path(__file__).resolve().parent.parent


def load_json(rel: str) -> dict:
    return json.loads((REPO / rel).read_text())


def exact(obj: dict, keys: set[str], where: str) -> None:
    """Fail on missing or unknown keys: an unknown key could disable a hook."""
    assert set(obj) == keys, f"{where}: keys {sorted(obj)} != expected {sorted(keys)}"


def at_least(value: object, minimum: float, where: str) -> None:
    assert isinstance(value, (int, float)) and value >= minimum, \
        f"{where}: timeout {value!r} below {minimum} (launcher needs time to start)"


def claude() -> str:
    d = load_json(".claude/settings.json")
    exact(d, {"$schema", "permissions", "hooks"}, ".claude/settings.json")
    exact(d["hooks"], {"PreToolUse"}, "claude hooks")
    [entry] = d["hooks"]["PreToolUse"]
    exact(entry, {"matcher", "hooks"}, "claude entry")
    assert entry["matcher"] == "Bash"
    [hook] = entry["hooks"]
    exact(hook, {"type", "command", "timeout"}, "claude hook")
    assert hook["type"] == "command"
    at_least(hook["timeout"], 5, "claude (seconds)")
    return hook["command"]


def cursor() -> str:
    d = load_json(".cursor/hooks.json")
    exact(d, {"version", "hooks"}, ".cursor/hooks.json")
    exact(d["hooks"], {"beforeShellExecution"}, "cursor hooks")
    [hook] = d["hooks"]["beforeShellExecution"]
    exact(hook, {"command", "timeout", "failClosed"}, "cursor hook")
    assert hook["failClosed"] is True, "Cursor hook must keep failClosed: true"
    at_least(hook["timeout"], 5, "cursor (seconds)")
    return hook["command"]


def gemini() -> str:
    d = load_json(".gemini/settings.json")
    exact(d, {"hooks"}, ".gemini/settings.json")
    exact(d["hooks"], {"BeforeTool"}, "gemini hooks")
    [entry] = d["hooks"]["BeforeTool"]
    exact(entry, {"matcher", "hooks"}, "gemini entry")
    assert entry["matcher"] == "run_shell_command"
    [hook] = entry["hooks"]
    exact(hook, {"type", "name", "command", "timeout"}, "gemini hook")
    assert hook["type"] == "command"
    at_least(hook["timeout"], 5000, "gemini (milliseconds)")
    return hook["command"]


def codex() -> str:
    d = load_json(".codex/hooks.json")
    exact(d, {"hooks"}, ".codex/hooks.json")
    exact(d["hooks"], {"PreToolUse"}, "codex hooks")
    [entry] = d["hooks"]["PreToolUse"]
    exact(entry, {"matcher", "hooks"}, "codex entry")
    assert entry["matcher"] == "Bash"
    [hook] = entry["hooks"]
    exact(hook, {"type", "command", "timeout"}, "codex hook")
    assert hook["type"] == "command"
    assert "--format codex" in hook["command"], "Codex launcher must pass --format codex"
    at_least(hook["timeout"], 5, "codex (seconds)")
    return hook["command"]


def copilot() -> str:
    d = load_json(".github/hooks/agent-guard.json")
    exact(d, {"version", "hooks"}, ".github/hooks/agent-guard.json")
    exact(d["hooks"], {"preToolUse"}, "copilot hooks")
    [hook] = d["hooks"]["preToolUse"]
    exact(hook, {"type", "matcher", "bash", "timeoutSec"}, "copilot hook")
    assert hook["type"] == "command" and hook["matcher"] == "bash"
    at_least(hook["timeoutSec"], 5, "copilot (seconds)")
    return hook["bash"]


def kimi() -> str:
    d = tomllib.loads((REPO / "ops" / "agent-guard.kimi.toml").read_text())
    exact(d, {"hooks"}, "ops/agent-guard.kimi.toml")
    [hook] = d["hooks"]
    exact(hook, {"event", "matcher", "command", "timeout"}, "kimi hook (Kimi allows only these)")
    assert hook["event"] == "PreToolUse" and hook["matcher"] == "Bash"
    at_least(hook["timeout"], 5, "kimi (seconds)")
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


def run(launcher: str, payload: dict, cwd: pathlib.Path = REPO) -> int:
    env = {k: v for k, v in os.environ.items() if k != "CLAUDE_PROJECT_DIR"}
    return subprocess.run(["sh", "-c", launcher], input=json.dumps(payload), text=True,
                          cwd=cwd, env=env, capture_output=True, timeout=30).returncode


def fallback_repo(tmp: pathlib.Path) -> pathlib.Path:
    """A throwaway repo whose checkout lacks the guard but origin/main has it.

    This is the branch-switch case (QA-16): the launcher must fetch the guard
    from git rather than run unguarded or block everything.
    """
    repo = tmp / "fallback"
    (repo / "ops").mkdir(parents=True)
    shutil.copy2(REPO / "ops" / "agent-guard.py", repo / "ops" / "agent-guard.py")
    git = ["git", "-C", str(repo), "-c", "user.name=agent-guard-ci", "-c", "user.email=ci@localhost",
           "-c", "commit.gpgsign=false"]
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(git + ["add", "ops/agent-guard.py"], check=True)
    subprocess.run(git + ["commit", "-q", "-m", "guard"], check=True)
    subprocess.run(git + ["update-ref", "refs/remotes/origin/main", "HEAD"], check=True)
    (repo / "ops" / "agent-guard.py").unlink()  # checkout without the guard
    return repo


def main() -> int:
    failures: list[str] = []
    for rel in [".claude/settings.json", ".cursor/hooks.json", ".gemini/settings.json",
                ".codex/hooks.json", ".github/hooks/agent-guard.json", "ops/agent-guard.kimi.toml"]:
        if "disableallhooks" in (REPO / rel).read_text().lower():
            failures.append(f"{rel}: contains disableAllHooks")
    launchers: dict[str, str] = {}
    tmpdir = tempfile.TemporaryDirectory()
    fb = fallback_repo(pathlib.Path(tmpdir.name))
    for name, (reader, build) in CASES.items():
        try:
            launcher = reader()
        except (AssertionError, KeyError, ValueError) as exc:
            failures.append(f"{name}: hook not wired as expected ({exc!r})")
            continue
        launchers[name] = launcher
        for label, cwd in (("checkout", REPO), ("fallback", fb)):
            b, a = run(launcher, build(BLOCK), cwd), run(launcher, build(ALLOW), cwd)
            if b != 2:
                failures.append(f"{name} ({label}): '{BLOCK}' exited {b}, expected 2 (blocked)")
            if a != 0:
                failures.append(f"{name} ({label}): '{ALLOW}' exited {a}, expected 0 (allowed)")
    canonical = {n: l.replace(" --format codex", "") for n, l in launchers.items()}
    if len(set(canonical.values())) > 1:
        failures.append("launchers differ between tools: " + ", ".join(sorted(canonical)))
    for f in failures:
        print(f"::error::{f}")
    print(f"check-agent-hooks: {len(CASES)} tools, {len(failures)} problem(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
