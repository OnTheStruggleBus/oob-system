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
  Claude Code PreToolUse hook (reads the hook JSON on stdin):
      python3 ops/agent-guard.py
      -> exit 0 allows; exit 2 blocks, reason on stderr.
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
RULES: list[tuple[str, str]] = [
    # --- merging and approving -------------------------------------------
    ("merging a PR is Marshal's alone (AGENTS.md §6 rule 1)",
     r"\bgh\s+pr\s+merge\b"),
    ("approving a PR is not allowed for agents (AGENTS.md §11)",
     r"\bgh\s+pr\s+review\b[^|;&]*(--approve|\s-a\b)"),
    ("merging via the API is Marshal's alone",
     r"\bgh\s+api\b[^|;&]*pulls/\d+/merge\b"),
    ("approving via the API is not allowed for agents",
     r"\bgh\s+api\b[^|;&]*pulls/\d+/reviews\b[^|;&]*approve"),
    ("merge/approve/admin GraphQL mutations are not allowed for agents",
     r"\bgh\s+api\s+graphql\b[\s\S]*\b(mergePullRequest|enablePullRequestAutoMerge"
     r"|addPullRequestReview\b[\s\S]*APPROVE|submitPullRequestReview\b[\s\S]*APPROVE"
     r"|(create|update|delete)BranchProtectionRule|(create|update|delete)RepositoryRuleset"
     r"|updateRepository|deleteRepository|transferRepository|archiveRepository)\b"),
    # --- repository admin --------------------------------------------------
    ("repository admin endpoints are Marshal's alone",
     r"\bgh\s+api\b[^|;&]*(branches/[^\s/]+/protection|/rulesets\b|/collaborators\b"
     r"|/hooks\b|/keys\b|/actions/(secrets|variables|permissions)\b|/environments\b"
     r"|/transfer\b|/teams\b|/invitations\b)"),
    ("changing repository settings is Marshal's alone",
     r"\bgh\s+api\b[^|;&]*(-X|--method)\s*(PATCH|DELETE)\s+[^|;&]*repos/[^/\s]+/[^/\s]+/?(\s|$)"),
    ("changing repository settings is Marshal's alone",
     r"\bgh\s+repo\s+(edit|delete|rename|archive|unarchive|transfer|set-default)\b"),
    ("secrets and variables are Marshal's alone",
     r"\bgh\s+(secret|variable)\s+(set|delete|remove)\b"),
    ("rulesets are Marshal's alone",
     r"\bgh\s+ruleset\s+(create|edit|delete)\b"),
    # --- credentials -------------------------------------------------------
    ("reading or changing the gh login is not allowed for agents",
     r"\bgh\s+auth\s+(token|login|logout|refresh|switch|setup-git)\b"),
    ("raw GitHub API calls must go through gh, not curl/wget/http",
     r"\b(curl|wget|http|https|xh)\b[^|;&]*\bapi\.github\.com\b"),
    # --- git ----------------------------------------------------------------
    ("force-pushing is blocked (AGENTS.md §6 rule 6)",
     r"\bgit\s+push\b[^|;&]*(\s--force(-with-lease)?\b|\s-f\b|\s\+\S)"),
    ("pushing to main is blocked; every change goes through a PR",
     r"\bgit\s+push\b[^|;&]*(\s|:)(refs/heads/)?(main|master)(\s|$|;|&|\|)"),
    # --- the guard itself ----------------------------------------------------
    ("disabling or editing the agent guard is Marshal's alone",
     r"(\.claude/settings(\.local)?\.json|ops/agent-guard\.py|\.kimi-code/config\.toml|disableAllHooks)"
     r"[\s\S]*?(>|\btee\b|\bsed\s+-i|\bperl\s+-[pi]|\bmv\b|\bcp\b|\brm\b|\bln\b|\btruncate\b|\bchmod\b)"),
    ("disabling or editing the agent guard is Marshal's alone",
     r"(>|\btee\b|\bsed\s+-i|\bmv\b|\bcp\b|\brm\b|\bln\b|\btruncate\b|\bchmod\b)"
     r"[^|;&]*(\.claude/settings(\.local)?\.json|ops/agent-guard\.py|\.kimi-code/config\.toml)"),
]

COMPILED = [(reason, re.compile(rx, re.IGNORECASE)) for reason, rx in RULES]

# Heredoc bodies are usually text (commit messages, PR bodies) that may
# merely *mention* a blocked command. Drop a body before matching, unless
# the line that opens it feeds it to an interpreter.
HEREDOC = re.compile(r"^(?P<opener>[^\n]*<<-?\s*(?P<q>['\"]?)(?P<tag>\w+)(?P=q)[^\n]*)\n"
                     r"(?P<body>.*?)\n[ \t]*(?P=tag)[ \t]*$",
                     re.DOTALL | re.MULTILINE)
INTERPRETER = re.compile(r"\b(bash|sh|zsh|dash|ksh|fish|eval|exec|source|xargs|ssh|"
                         r"python3?|perl|ruby|node|osascript)\b|^\s*\.\s")


def _strip_text_heredocs(command: str) -> str:
    def repl(m: re.Match) -> str:
        if INTERPRETER.search(m.group("opener")):
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
    "python3 ops/agent-guard.py --self-test",
    "cargo test",
    "git commit -q -F - <<'EOF'\ndocs: never run gh pr merge as an agent\nEOF",
    "cat > /tmp/body.md <<'EOF'\nAgents must not run `gh pr merge` or edit .claude/settings.json > x.\nEOF",
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
    total = len(SELF_TEST_BLOCK) + len(SELF_TEST_ALLOW)
    print(f"agent-guard self-test: {total - failures}/{total} passed")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    if len(argv) >= 2 and argv[1] == "--self-test":
        return self_test()
    if len(argv) >= 3 and argv[1] == "--check":
        command = " ".join(argv[2:])
    else:
        raw = sys.stdin.read()
        try:
            payload = json.loads(raw) if raw.strip() else {}
        except json.JSONDecodeError:
            # Unparseable hook input: fail closed for safety.
            print("agent-guard: could not parse hook input; blocking.", file=sys.stderr)
            return 2
        command = (payload.get("tool_input") or {}).get("command") or ""
    reason = check(command)
    if reason is None:
        return 0
    print(f"Blocked by ops/agent-guard.py: {reason}. "
          "If Marshal wants this done, he runs it himself.", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
