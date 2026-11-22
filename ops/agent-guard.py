#!/usr/bin/env python3
"""Oob agent command guard (AGENTS.md §14, "Credentials").

Agents run shell commands through the owner's `gh` login, so nothing on
GitHub's side can tell an agent from Marshal. This guard blocks the shell
commands an agent must never run: merging or approving PRs, changing
repository admin settings, reading the raw token, force-pushing, pushing
to `main`, and disabling this guard.

It is best effort, not a sandbox: a determined process can obfuscate a
command past any pattern match. It stops honest mistakes and casual
misbehaviour, which is the gap AGENTS.md §14 names.

Usage
  As a hook (reads the tool's hook JSON on stdin): Claude Code, Kimi Code,
  Cursor, Gemini CLI, GitHub Copilot, and Codex all work. The command is
  taken from `tool_input.command`, top-level `command` (Cursor), or
  `toolArgs` (Copilot camelCase, object or JSON string).
      python3 ops/agent-guard.py               # exit 0 allows; exit 2 blocks
      python3 ops/agent-guard.py --format codex  # also prints Codex's deny JSON
  The committed per-tool hook files are listed in AGENTS.md §14.
  Any other tool with a pre-command hook:
      python3 ops/agent-guard.py --check "<shell command>"
      -> exit 0 allows; exit 2 blocks, reason on stderr.
  Self-test:
      python3 ops/agent-guard.py --self-test

Changing this file, .claude/settings*.json, or the Kimi Code hook in
~/.kimi-code/config.toml is Marshal's job: agents are blocked from
editing them (see the rules below and .claude/settings.json).

Kimi Code: hooks are user-level only (~/.kimi-code/config.toml), and a
hook that crashes or times out is fail-open there. The [[hooks]] entry
allows exactly event/matcher/command/timeout; see AGENTS.md §14.
"""

from __future__ import annotations

import json
import re
import sys

# (reason, regex). Matched case-insensitively against the whole command,
# so `bash -c '...'`, pipes, and `&&` chains are covered too.
PROTECTED = (r"(\.claude/settings(\.local)?\.json|ops/agent-guard\.py|\.kimi-code/config\.toml"
             r"|\.cursor/hooks\.json|\.gemini/settings\.json|\.codex/(hooks\.json|config\.toml)"
             r"|\.github/hooks/)")

RULES: list[tuple[str, str]] = [
    # Every rule stays inside one command: [^|;&\n] never crosses a pipe,
    # a chain, or a new line (QA-19).
    # --- merging and approving -------------------------------------------
    ("merging a PR is Marshal's alone (AGENTS.md §6 rule 1)",
     r"\bgh\s+pr\s+merge\b"),
    ("approving a PR is not allowed for agents (AGENTS.md §11)",
     r"\bgh\s+pr\s+review\b[^|;&\n]*(--approve\b|\s-[a-z]*a[a-z]*\b)"),
    ("merging via the API is Marshal's alone",
     r"\bgh\s+api\b[^|;&\n]*(pulls/[^/\s]+/merge\b|/merges\b)"),
    ("approving via the API is not allowed for agents",
     r"\bgh\s+api\b(?=[^|;&\n]*pulls/[^/\s]+/reviews\b)(?=[^|;&\n]*\bapprove\b)"),
    ("approving via the API is not allowed for agents",
     r"\bgh\s+api\b[^|;&\n]*pulls/[^/\s]+/reviews\b[^|;&\n]*--input\b[\s\S]*\bapprove\b"),
    ("merge/approve/admin GraphQL mutations are not allowed for agents",
     r"\bgh\s+api\s+graphql\b[\s\S]*\b(mergePullRequest|enablePullRequestAutoMerge"
     r"|addPullRequestReview\b[\s\S]*APPROVE|submitPullRequestReview\b[\s\S]*APPROVE"
     r"|(create|update|delete)BranchProtectionRule|(create|update|delete)RepositoryRuleset"
     r"|updateRepository|deleteRepository|transferRepository|archiveRepository)\b"),
    # --- repository admin --------------------------------------------------
    ("repository admin endpoints are Marshal's alone",
     r"\bgh\s+api\b[^|;&\n]*(branches/[^\s/]+/protection|/rulesets\b|/collaborators\b"
     r"|/hooks\b|/keys\b|/actions/(secrets|variables|permissions)\b|/environments\b"
     r"|/transfer\b|/teams\b|/invitations\b)"),
    ("changing repository settings is Marshal's alone",
     r"\bgh\s+api\b(?=[^|;&\n]*(-X|--method)[\s=]*['\"]?(PATCH|DELETE|PUT)\b)"
     r"(?=[^|;&\n]*\brepos/[^/\s'\"]+/[^/\s'\"]+/?(['\"]|\s|$))"),
    ("changing repository settings is Marshal's alone",
     r"\bgh\s+repo\s+(edit|delete|rename|archive|unarchive|transfer|set-default)\b"),
    ("secrets and variables are Marshal's alone",
     r"\bgh\s+(secret|variable)\s+(set|delete|remove)\b"),
    ("rulesets are Marshal's alone",
     r"\bgh\s+ruleset\s+(create|edit|delete)\b"),
    # --- credentials -------------------------------------------------------
    ("reading or changing the gh login is not allowed for agents",
     r"\bgh\s+auth\s+(token|login|logout|refresh|switch|setup-git|git-credential)\b"),
    ("printing the gh token is not allowed for agents",
     r"\bgh\s+auth\s+status\b[^|;&\n]*(--show-token\b|\s-[a-z]*t[a-z]*\b)"),
    ("raw GitHub API calls must go through gh, not curl/wget/http",
     r"\b(curl|wget|http|https|xh)\b[^|;&\n]*\bapi\.github\.com\b"),
    # --- git ----------------------------------------------------------------
    ("force-pushing is blocked (AGENTS.md §6 rule 6)",
     r"\bgit\s+push\b[^|;&\n]*(\s--force(-with-lease|-if-includes)?\b|\s-[a-z]*f[a-z]*\b|\s\+\S)"),
    ("pushing to main is blocked; every change goes through a PR",
     r"\bgit\s+push\b[^|;&\n]*(\s|:)(refs/heads/)?(main|master)(\s|$|[;&|])"),
    # --- the guard itself (QA-20: only real writes to a guarded path) -------
    ("disabling or editing the agent guard is Marshal's alone",
     r"(^|[^0-9&-])>>?\s*['\"]?[^\s'\";&|]*" + PROTECTED),
    ("disabling or editing the agent guard is Marshal's alone",
     r"\b(tee|mv|cp|rm|ln|truncate|chmod|install|dd)\b[^|;&\n]*" + PROTECTED),
    ("disabling or editing the agent guard is Marshal's alone",
     r"\b(sed\s+(-[a-z]*\s+)*-i|perl\s+-[a-z]*[pi])\b[^|;&\n]*" + PROTECTED),
    ("disabling or editing the agent guard is Marshal's alone",
     r"\bdisableAllHooks\b"),
]

COMPILED = [(reason, re.compile(rx, re.IGNORECASE)) for reason, rx in RULES]

# Heredoc bodies are usually text (commit messages, PR bodies) that may
# merely *mention* a blocked command. A body is dropped before matching
# only when its opening line is plainly a text sink; it is kept when the
# opener runs `gh` (QA-17: `gh api --input -`), uses command substitution,
# or runs an interpreter in command position.
HEREDOC = re.compile(r"^(?P<opener>[^\n]*<<-?\s*(?P<q>['\"]?)(?P<tag>\w+)(?P=q)[^\n]*)\n"
                     r"(?P<body>.*?)\n[ \t]*(?P=tag)[ \t]*$",
                     re.DOTALL | re.MULTILINE)
KEEP_BODY = re.compile(
    r"\bgh\s+api\b|\$\(|`"
    r"|(^|[;&|(]|\bthen\b|\bdo\b)\s*(sudo\s+|env\s+(\w+=\S*\s+)*)?"
    r"(bash|sh|zsh|dash|ksh|fish|eval|exec|source|xargs|ssh|python3?|perl|ruby|node|osascript|\.)(\s|$)")


def _strip_text_heredocs(command: str) -> str:
    def repl(m: re.Match) -> str:
        if KEEP_BODY.search(m.group("opener")):
            return m.group(0)
        return m.group("opener") + "\n" + m.group("tag")
    return HEREDOC.sub(repl, command)


def check(command: str) -> str | None:
    """Return the block reason, or None if the command is allowed."""
    scanned = _strip_text_heredocs(command)
    for reason, rx in COMPILED:
        if rx.search(scanned):
            return reason
    return None


SELF_TEST_BLOCK = [
    "gh pr merge 10 --merge",
    "gh pr merge --auto --squash 12",
    "cd x && gh pr merge 3",
    "bash -c 'gh pr merge 3'",
    "gh pr review 10 --approve",
    "gh pr review 10 -a -b ok",
    "gh api -X PUT repos/o/r/pulls/10/merge",
    "gh api repos/o/r/pulls/10/reviews -f event=APPROVE",
    "gh api graphql -f query='mutation{mergePullRequest(input:{}){clientMutationId}}'",
    "gh api graphql -f query='mutation{updateBranchProtectionRule(input:{}){clientMutationId}}'",
    "gh api -X PUT repos/o/r/rulesets/123 --input x.json",
    "gh api repos/o/r/branches/main/protection",
    "gh api -X PATCH repos/o/r -f allow_auto_merge=true",
    "gh repo edit --enable-auto-merge",
    "gh secret set FOO",
    "gh auth token",
    "gh auth refresh -s project",
    "curl -H \"Authorization: token x\" https://api.github.com/repos/o/r/pulls/1/merge -X PUT",
    "git push --force origin feat",
    "git push -f",
    "git push origin +feat:feat",
    "git push origin main",
    "git push origin HEAD:main",
    "echo '{\"disableAllHooks\":true}' > .claude/settings.local.json",
    "rm .claude/settings.json",
    "sed -i '' 's/x/y/' ops/agent-guard.py",
    "bash <<'EOF'\ngh pr merge 10\nEOF",
    "python3 - <<EOF\nimport os; os.system('gh pr merge 10')\nEOF",
    "cat <<'EOF' > x.sh\nok\nEOF\ngh pr merge 10",
    "sed -i '' '/hooks/d' ~/.kimi-code/config.toml",
    "echo '' > ~/.kimi-code/config.toml",
    "cp /tmp/x.json ~/.claude/settings.json",
    "rm .cursor/hooks.json",
    "echo '{}' > .gemini/settings.json",
    "sed -i '' '/hooks/d' .codex/hooks.json",
    "rm -rf .github/hooks/",
    "gh api repos/o/r/pulls/$PR/merge -X PUT",
    "gh api -f event=APPROVE repos/o/r/pulls/3/reviews",
    "gh api repos/o/r -X PATCH -f allow_auto_merge=true",
    "gh api --method=PATCH repos/o/r -f x=y",
    "gh api repos/o/r/merges -f base=main -f head=feat",
    "git push -fu origin feat",
    "git push -uf origin feat",
    "gh auth status -t",
    "gh auth status --show-token",
    "gh auth git-credential get",
    "gh api repos/o/r/pulls/3/reviews --input - <<EOF\n{\"event\":\"APPROVE\"}\nEOF",
    "gh api graphql -f query=\"$(cat <<'EOF'\nmutation{mergePullRequest(input:{}){clientMutationId}}\nEOF\n)\"",
    "echo '{\"disableAllHooks\": true}' >> ~/.claude/settings.json",
]

SELF_TEST_ALLOW = [
    "ls",
    "gh pr view 10 --json headRefOid",
    "gh pr list",
    "gh pr create --base main --head feat --title t --body-file b.md",
    "gh pr comment 10 --body-file c.md",
    "gh pr checks 10",
    "gh pr review 10 --comment -b 'Role review: oob-qa'",
    "gh api repos/o/r/issues/10/comments",
    "gh api repos/o/r/pulls/10/comments/5/replies -f body=x",
    "gh api graphql -f query='query{repository(owner:\"o\",name:\"r\"){id}}'",
    "gh api graphql -f query='mutation{resolveReviewThread(input:{threadId:\"x\"}){thread{isResolved}}}'",
    "gh issue create --title t --body b",
    "gh run list --branch feat",
    "git push origin process/agent-driven-workflow",
    "git push -u origin docs/main-notes",
    "git fetch origin main",
    "git log --oneline origin/main",
    "cat .claude/settings.json",
    "jq . .claude/settings.json",
    "cat ~/.kimi-code/config.toml",
    "cat .cursor/hooks.json .gemini/settings.json .codex/hooks.json",
    "python3 ops/agent-guard.py --self-test",
    "cargo test",
    "git commit -q -F - <<'EOF'\ndocs: never run gh pr merge as an agent\nEOF",
    "cat > /tmp/body.md <<'EOF'\nAgents must not run `gh pr merge` or edit .claude/settings.json > x.\nEOF",
    "git push -u origin feat\ngh pr create --base main --head feat --title t --body-file b.md",
    "git push -u origin feat\nrm -f /tmp/x",
    "python3 ops/agent-guard.py --self-test 2>&1",
    "git diff origin/main -- ops/agent-guard.py 2>/dev/null",
    "grep -n hooks .claude/settings.json 2>/dev/null",
    "git log --format='%h -> %s' -- ops/agent-guard.py",
    "gh pr create --title 'source tree docs' --body-file - <<'EOF'\nAgents must not run gh pr merge.\nEOF",
    "gh auth status",
]


def self_test() -> int:
    failures = 0
    for cmd in SELF_TEST_BLOCK:
        if check(cmd) is None:
            print(f"FAIL (should block): {cmd}")
            failures += 1
    for cmd in SELF_TEST_ALLOW:
        reason = check(cmd)
        if reason is not None:
            print(f"FAIL (should allow): {cmd}  [{reason}]")
            failures += 1
    # Payload shapes from each supported tool.
    shapes = [
        {"tool_name": "Bash", "tool_input": {"command": "gh pr merge 1"}},      # Claude/Kimi/Codex/Gemini
        {"command": "gh pr merge 1", "cwd": "/p"},                               # Cursor
        {"toolName": "bash", "toolArgs": "{\"command\": \"gh pr merge 1\"}"},     # Copilot (string)
        {"toolName": "bash", "toolArgs": {"command": "gh pr merge 1"}},          # Copilot (object)
        {"tool_name": "bash", "tool_input": "{\"command\": \"gh pr merge 1\"}"},  # Copilot snake, string
    ]
    for shape in shapes:
        if check(extract_command(shape)) is None:
            print(f"FAIL (payload shape not read): {shape}")
            failures += 1
    total = len(SELF_TEST_BLOCK) + len(SELF_TEST_ALLOW) + len(shapes)
    print(f"agent-guard self-test: {total - failures}/{total} passed")
    return 1 if failures else 0


def extract_command(payload: object) -> str:
    """Pull the shell command out of any supported tool's hook payload."""
    if not isinstance(payload, dict):
        return ""
    candidates: list[object] = []
    tool_input = payload.get("tool_input")
    if isinstance(tool_input, str):
        try:
            tool_input = json.loads(tool_input)
        except json.JSONDecodeError:
            candidates.append(tool_input)
    if isinstance(tool_input, dict):
        candidates.append(tool_input.get("command"))
    candidates.append(payload.get("command"))          # Cursor beforeShellExecution
    tool_args = payload.get("toolArgs")                # Copilot camelCase
    if isinstance(tool_args, str):
        try:
            tool_args = json.loads(tool_args)
        except json.JSONDecodeError:
            candidates.append(tool_args)
    if isinstance(tool_args, dict):
        candidates.append(tool_args.get("command"))
    parts = [c if isinstance(c, str) else " ".join(map(str, c))
             for c in candidates if isinstance(c, (str, list)) and c]
    return "\n".join(parts)


def main(argv: list[str]) -> int:
    args = argv[1:]
    fmt = "exit2"
    if "--format" in args:
        i = args.index("--format")
        fmt = args[i + 1] if i + 1 < len(args) else "exit2"
        del args[i:i + 2]
    if args[:1] == ["--self-test"]:
        return self_test()
    if args[:1] == ["--check"] and len(args) >= 2:
        command = " ".join(args[1:])
    else:
        raw = sys.stdin.read()
        try:
            payload = json.loads(raw) if raw.strip() else {}
        except json.JSONDecodeError:
            # Unparseable hook input: fail closed for safety.
            print("agent-guard: could not parse hook input; blocking.", file=sys.stderr)
            return 2
        command = extract_command(payload)
        if not command.strip():
            # Every committed hook is scoped to the shell tool, so an empty
            # command means this tool's payload shape is unknown. Fail closed
            # loudly rather than run unguarded.
            print("agent-guard: no shell command found in the hook payload "
                  "(unknown tool payload shape); blocking. Update "
                  "extract_command() in ops/agent-guard.py.", file=sys.stderr)
            return 2
    reason = check(command)
    if reason is None:
        return 0
    message = (f"Blocked by ops/agent-guard.py: {reason}. "
               "If Marshal wants this done, he runs it himself.")
    if fmt == "codex":
        print(json.dumps({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": message}}))
    print(message, file=sys.stderr)
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv))
    except SystemExit:
        raise
    except BaseException as exc:  # fail closed on the guard's own errors
        print(f"agent-guard: internal error ({exc!r}); blocking.", file=sys.stderr)
        sys.exit(2)
