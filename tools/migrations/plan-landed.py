#!/usr/bin/env python3
"""Mark each v3 task in tasks.toml as `landed` or `retired`, from evidence.

Re-runnable after a rebase: it rewrites only the landed/retired/note lines
that follow each task's `title`.

  landed  the task's issue says "Merged into `v3` by #N" (or a commit on
          origin/main names the task in its subject, `<id>: ...`, with a
          PR suffix), and that PR's merge commit is an ancestor of origin/main
  retired the issue was closed as not planned and nothing above holds

Tasks with no evidence either way are left open. Needs `gh` (read-only).
Overrides below cover what the evidence cannot see; each says why.

  python3 tools/migrations/plan-landed.py [--base origin/main]
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "docs/architecture-v3/tasks.toml"
REPO = "DerpcatMusic/KONTRA"
RETIRED = "2026-11-12 tracker cleanup: v3 became main; open work moved to 13-IR-VNEXT.md"
PROBE = "tools/probe retired later by f2b07ed02 (#1196)"

# (sha, note) where the issue/commit trail is not enough on its own.
OVERRIDES = {
    "P0.5": ("b1522fb25", "v3.yml plan checks; closed by the coordinator without a PR"),
    "P8.1": (None, "#962 (#959 was the plan change)"),
    "P8.4b": ("823dba09a", "as crates/app/face (nui.rs, nui/layout.rs) over kontra-lua's nui module, "
                           "not a crates/format/native-ui crate"),
}
PR_OVERRIDES = {"P8.1": 962}  # the issue names the plan PR, not the implementation
NOTES = {"P0.2": PROBE, "P0.3": PROBE, "P0.3b": PROBE, "P5.6a": "with P5.6c in #856"}


def run(*args: str) -> str:
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout


def on_base(sha: str, base: str) -> bool:
    return subprocess.run(["git", "merge-base", "--is-ancestor", sha, base]).returncode == 0


def issues() -> dict[str, dict]:
    out = {}
    raw = run("gh", "api", "--paginate", f"repos/{REPO}/issues?labels=v3&state=all&per_page=100",
              "--jq", '.[] | select(.pull_request|not) | {number, state, state_reason, body}')
    for line in raw.splitlines():
        i = json.loads(line)
        m = re.search(r"<!-- v3-task: ([\w.]+) -->", i.get("body") or "")
        if m:
            out[m[1]] = i
    return out


def merged_by(number: int) -> int | None:
    comments = run("gh", "api", f"repos/{REPO}/issues/{number}/comments", "--jq", ".[].body")
    m = re.findall(r"Merged into `v3` by #(\d+)", comments)
    return int(m[-1]) if m else None


def merge_commit(pr: int) -> str | None:
    oid = run("gh", "pr", "view", str(pr), "-R", REPO, "--json", "state,mergeCommit",
              "--jq", 'select(.state=="MERGED") | .mergeCommit.oid').strip()
    return oid or None


def commit_naming(tid: str, base: str) -> str | None:
    """Oldest commit on base whose subject starts `<id>: ` and ends in a PR number."""
    log = run("git", "log", "--reverse", "--format=%h %s", base)
    pat = re.compile(rf"^(\w+) {re.escape(tid)}: .*\(#\d+\)$")
    return next((m[1] for line in log.splitlines() if (m := pat.match(line))), None)


def verdict(tid: str, issue: dict | None, base: str) -> tuple[str, str, str | None]:
    sha, note = OVERRIDES.get(tid, (None, NOTES.get(tid)))
    if not sha and issue and issue["state"] == "closed":
        pr = PR_OVERRIDES.get(tid) or (merged_by(issue["number"]) if issue["state_reason"] == "completed" else None)
        oid = merge_commit(pr) if pr else None
        sha = oid[:9] if oid and on_base(oid, base) else None
    sha = sha or commit_naming(tid, base)
    if sha:
        return "landed", sha, note
    if issue and issue.get("state_reason") == "not_planned":
        return "retired", RETIRED, None
    return "open", "", None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="origin/main")
    a = ap.parse_args()
    found = issues()
    text = MANIFEST.read_text()
    text = re.sub(r'^(landed|retired|note) = "[^"]*"\n', "", text, flags=re.M)
    counts = {"landed": 0, "retired": 0, "open": 0}

    def mark(m: re.Match) -> str:
        tid = m[1]
        state, value, note = verdict(tid, found.get(tid), a.base)
        counts[state] += 1
        out = m[0]
        if state != "open":
            out += f'\n{state} = "{value}"'
        if note and state == "landed":
            out += f'\nnote = "{note}"'
        return out

    text = re.sub(r'^id = "([^"]+)"\ntitle = "[^\n]*"', mark, text, flags=re.M)
    MANIFEST.write_text(text)
    print(", ".join(f"{k} {v}" for k, v in counts.items()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
