#!/usr/bin/env python3
"""KONTRA v3 planner: docs/architecture-v3/tasks.toml -> checks, briefs, GitHub.

  plan.py validate               manifest is well-formed, DAG acyclic, doc links resolve
  plan.py index                  regenerate docs/architecture-v3/TASKS.md
  plan.py brief ID               print the self-contained agent brief (= issue body)
  plan.py ready [--offline]      tasks that can start now, as a conflict-free batch
  plan.py scope ID [--base REF]  fail if the branch changes files outside the task's owns
  plan.py task-of BRANCH         print the task id encoded in a v3-<id>-slug branch
  plan.py sync [--dry-run]       labels, milestones, epics, issues, sub-issues, deps, project
  plan.py refresh                status:ready / status:blocked labels only (CI-safe, no project access)
  plan.py on-merge BRANCH        close the task issue of a merged v3-<id> branch, then refresh

Workflow and rules: docs/architecture-v3/12-WORKFLOW.md. Python >= 3.11, stdlib only, `gh` for sync.
"""
from __future__ import annotations

import argparse
import functools
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "docs/architecture-v3/tasks.toml"
REPO = "DerpcatMusic/KONTRA"
OWNER = "DerpcatMusic"
MARK = "<!-- v3-task: {id} -->"
EPIC_MARK = "<!-- v3-epic: {phase} -->"
AGENTS = {"worker", "coordinator", "spike"}
SIZES = {"S", "M", "L"}
LANES = {"tooling", "coordinator", "playback", "script", "structure", "dsp", "engine", "format", "app"}


# ── manifest ────────────────────────────────────────────────────────────────

def load(path: Path = MANIFEST) -> dict:
    data = tomllib.loads(path.read_text())
    tasks = data.get("task", [])
    data["by_id"] = {t["id"]: t for t in tasks}
    return data


def owns(plan: dict, task: dict, *, skip_modes: tuple[str, ...] = ()) -> list[str]:
    """Owned globs with @lock references expanded."""
    out = []
    for g in task.get("owns", []):
        if g.startswith("@"):
            lock = plan["locks"][g[1:]]
            if lock["mode"] not in skip_modes:
                out += lock["paths"]
        else:
            out.append(g)
    return out


# ── globs ───────────────────────────────────────────────────────────────────

def braces(p: str) -> list[str]:
    m = re.search(r"\{([^{}]*)\}", p)
    if not m:
        return [p]
    return [x for alt in m[1].split(",") for x in braces(p[: m.start()] + alt + p[m.end():])]


@functools.cache
def rx(glob: str) -> re.Pattern:
    out, i = "", 0
    while i < len(glob):
        if glob.startswith("**/", i):
            out, i = out + "(?:.*/)?", i + 3
        elif glob.startswith("**", i):
            out, i = out + ".*", i + 2
        elif glob[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif glob[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + re.escape(glob[i]), i + 1
    return re.compile(out + r"\Z")


def match(path: str, globs: list[str]) -> bool:
    return any(rx(b).match(path) for g in globs for b in braces(g))


def literal(glob: str) -> str:
    return re.split(r"[*?{]", glob, maxsplit=1)[0]


def overlap(a: list[str], b: list[str], files: list[str]) -> list[str]:
    """Paths (or glob prefixes, for files that do not exist yet) both sets may touch."""
    hits = [f for f in files if match(f, a) and match(f, b)]
    if hits:
        return hits
    for x in (y for g in a for y in braces(g)):
        for z in (y for g in b for y in braces(g)):
            lx, lz = literal(x), literal(z)
            if match(lx, [z]) or match(lz, [x]):
                return [f"{x} ~ {z}"]
            short, long_, pat = (lx, lz, x) if len(lx) <= len(lz) else (lz, lx, z)
            if long_.startswith(short) and pat[len(short):].startswith("**"):
                return [f"{x} ~ {z}"]
    return []


@functools.cache
def tracked() -> tuple[str, ...]:
    out = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True)
    return tuple(out.stdout.split())


# ── graph ───────────────────────────────────────────────────────────────────

def ancestors(plan: dict, tid: str) -> set[str]:
    seen, stack = set(), list(plan["by_id"][tid].get("deps", []))
    while stack:
        d = stack.pop()
        if d not in seen:
            seen.add(d)
            stack += plan["by_id"][d].get("deps", [])
    return seen


def topo(plan: dict) -> list[str]:
    order, state = [], {}

    def visit(t: str, path: list[str]) -> None:
        if state.get(t) == 2:
            return
        if state.get(t) == 1:
            raise ValueError("dependency cycle: " + " -> ".join(path + [t]))
        state[t] = 1
        for d in plan["by_id"][t].get("deps", []):
            visit(d, path + [t])
        state[t] = 2
        order.append(t)

    for t in plan["by_id"]:
        visit(t, [])
    return order


# ── doc links ───────────────────────────────────────────────────────────────

def slug(heading: str) -> str:
    s = heading.strip().lower()
    s = re.sub(r"[^\w\- ]", "", s)
    return s.replace(" ", "-")


def doc_path(ref: str) -> tuple[Path, str]:
    file, _, anchor = ref.partition("#")
    base = ROOT if file.startswith("docs/") else ROOT / "docs/architecture-v3"
    return (base / file).resolve(), anchor


def anchors(path: Path) -> set[str]:
    return {slug(m[1]) for m in re.finditer(r"^#{1,6}\s+(.*)$", path.read_text(), re.M)}


def doc_url(ref: str, branch: str) -> str:
    path, anchor = doc_path(ref)
    rel = path.relative_to(ROOT).as_posix()
    return f"https://github.com/{REPO}/blob/{branch}/{rel}" + (f"#{anchor}" if anchor else "")


# ── validate ────────────────────────────────────────────────────────────────

def validate(plan: dict) -> tuple[list[str], list[str]]:
    errors, warnings = [], []
    ids = [t["id"] for t in plan.get("task", [])]
    if len(ids) != len(set(ids)):
        errors.append("duplicate task ids: " + ", ".join(sorted({i for i in ids if ids.count(i) > 1})))
    for n, name in plan["phases"].items():
        if "," in name:
            errors.append(f"phase {n}: no commas in phase names (project select options are comma-separated)")
    for t in plan.get("task", []):
        tid = t["id"]
        if not re.fullmatch(r"P\d\.\d+[a-z]?", tid):
            errors.append(f"{tid}: id must look like P1.2 or P5.3a")
        for f in ("title", "phase", "lane", "size", "agent", "owns", "goal", "steps", "gate", "done"):
            if f not in t:
                errors.append(f"{tid}: missing {f}")
        if str(t.get("phase")) not in plan["phases"]:
            errors.append(f"{tid}: unknown phase {t.get('phase')}")
        if t.get("agent") not in AGENTS:
            errors.append(f"{tid}: agent must be one of {sorted(AGENTS)}")
        if t.get("size") not in SIZES:
            errors.append(f"{tid}: size must be S/M/L")
        if t.get("lane") not in LANES:
            errors.append(f"{tid}: unknown lane {t.get('lane')}")
        for d in t.get("deps", []):
            if d not in plan["by_id"]:
                errors.append(f"{tid}: unknown dep {d}")
        for g in t.get("owns", []):
            if g.startswith("@") and g[1:] not in plan["locks"]:
                errors.append(f"{tid}: unknown lock {g}")
        for ref in t.get("docs", []):
            path, anchor = doc_path(ref)
            if not path.exists():
                (warnings if ref.startswith("docs/") else errors).append(f"{tid}: doc not found: {ref}")
            elif anchor and anchor not in anchors(path):
                errors.append(f"{tid}: anchor not found: {ref}")
    if errors:
        return errors, warnings
    try:
        topo(plan)
    except ValueError as e:
        errors.append(str(e))
        return errors, warnings
    # parallel tasks that share exclusive paths serialize at dispatch time; report them
    files = list(tracked())
    tasks = plan["task"]
    for i, a in enumerate(tasks):
        for b in tasks[i + 1:]:
            if a["id"] in ancestors(plan, b["id"]) or b["id"] in ancestors(plan, a["id"]):
                continue
            hit = overlap(owns(plan, a, skip_modes=("append", "regenerate")),
                          owns(plan, b, skip_modes=("append", "regenerate")), files)
            if hit:
                warnings.append(f"serialized: {a['id']} and {b['id']} share {hit[0]}")
    return errors, warnings


# ── briefs ──────────────────────────────────────────────────────────────────

def brief(plan: dict, tid: str, numbers: dict[str, int] | None = None) -> str:
    t = plan["by_id"][tid]
    meta = plan["meta"]
    branch = meta["branch"]
    phase = plan["phases"][str(t["phase"])]
    numbers = numbers or {}
    slug_ = re.sub(r"[^a-z0-9]+", "-", t["title"].lower()).strip("-")[:40].rstrip("-")
    ref = lambda d: f"#{numbers[d]} " if d in numbers else ""
    lines = [
        MARK.format(id=tid) + " <!-- generated from docs/architecture-v3/tasks.toml; edit there, not here -->",
        f"> **{tid}** · Phase {t['phase']} {phase} · lane `{t['lane']}` · size {t['size']} · agent: **{t['agent']}**",
    ]
    if t.get("draft"):
        lines.append("> ⚠️ **Draft brief.** Scope and steps are a forecast. The coordinator re-checks them against the tree and removes `draft` before dispatch.")
    lines += ["", "## Goal", t["goal"], "", "## Read first",
              f"- [README.md]({doc_url('README.md', branch)}): the ten rules",
              f"- [12-WORKFLOW.md]({doc_url('12-WORKFLOW.md', branch)}): how to work on v3 (branching, scope, tests, PR)",
              "- `.claude/skills/v3-task/SKILL.md`: the step-by-step playbook for a v3 task, including resuming a draft PR "
              "(load it with the Skill tool as `v3-task`).",
              "- `.claude/skills/beautiful-rust/SKILL.md`: the Rust style every v3 change follows (load it with the Skill tool "
              "as `beautiful-rust`, or read the file). Where it conflicts with these docs or the KISS/real-time rules, the docs win."]
    lines += [f"- [{d}]({doc_url(d, branch)})" for d in t.get("docs", [])]
    lines += ["", "## Steps"] + [f"{i}. {s}" for i, s in enumerate(t["steps"], 1)]
    lines += ["", "## Scope (enforced by the `v3 plan` CI check)",
              "You may change only these paths:", ""]
    lines += [f"- `{g}`" for g in owns(plan, t)]
    lines += ["", ("Everything else is read-only, except `Cargo.lock` (regenerated by cargo; see step 4) and "
                   "`tools/xtask/size-baseline.json` (`cargo xtask size --update`, only with the `size-growth` label). "
                   "If the task needs more, stop and comment on this issue: "
                   "`Scope request: <paths> because <reason>`. The coordinator updates tasks.toml; do not edit outside scope.")]
    if t.get("deps"):
        lines += ["", "## Depends on (merged into `v3` first)"]
        lines += [f"- {ref(d)}{d} {plan['by_id'][d]['title']}" for d in t["deps"]]
    lines += ["", "## Gate (run these; paste the output in the PR)", "```sh"] + t["gate"] + ["```"]
    lines += ["", "## Done when"] + [f"- [ ] {d}" for d in t["done"]]
    lines += [f"- [ ] `python3 tools/plan/plan.py scope {tid}` passes",
              "- [ ] `KONTRA_CARGO='build-slot cargo' cargo xtask ci --base origin/v3` passes (output in the PR)",
              "- [ ] Net production lines reported (`cargo xtask size`)"]
    lines += ["", "## How to work",
              f"1. Branch from `{branch}`: `wt switch -c {meta['branch_prefix']}{tid}-{slug_} -b {branch}` "
              f"(T3/Claude worktrees: `git switch -c {meta['branch_prefix']}{tid}-{slug_} origin/{branch}`).",
              f"2. Commit early and often. Conventional Commits, with a `Task: {tid}` trailer. After the first commit push and open a "
              f"**draft** PR (`gh pr create --draft --base {branch} --title \"{tid}: {t['title']}\"`) and keep a `## Progress` list "
              "in its body; if your session ends, the next agent resumes from it.",
              ("3. Write Rust per the `beautiful-rust` skill. Commit everything first (the gate sees only committed files), then run "
              "`KONTRA_CARGO='build-slot cargo' cargo xtask ci --base origin/v3` (contributors: plain `cargo xtask ci`): "
              "rustfmt and clippy on the files you changed, tests of their packages, and the deps, debt and size ratchets, "
              "a local subset of the PR checks. Rerun it after your last commit. Never `cargo fmt` (it reformats unrelated files). Test code may unwrap, expect, "
              "panic and index without `#[allow]` (clippy.toml allows it); every new `#[allow]` grows the debt ratchet."),
              "4. `Cargo.lock`: only additions your change needs. Revert any unrelated version change cargo makes "
              "(CI checks e.g. that gpu-allocator and wgpu-hal share one `windows` version). Repo tools must never "
              "require `build-slot`; it exists only on the maintainer's machine.",
              *[f"   Your scope touches the source of `{f}`: regenerate it with `{cmd}` and commit it."
                for f, (src, cmd) in GENERATED.items() if overlap(owns(plan, t), [src], [])],
              "5. Run only the gate's crate-scoped commands, always through `build-slot`. No workspace builds, corpus runs, "
              "real-library surveys or plugin bundles: the coordinator does those once per merge batch.",
              "6. Push, fill in the PR template (gate output, scope check, net lines, deviations) and `gh pr ready`.",
              "7. Report back: PR URL, head SHA, gate output, any deviation from this brief and why."]
    return "\n".join(lines) + "\n"


# ── index ───────────────────────────────────────────────────────────────────

def index(plan: dict) -> str:
    out = ["# v3 tasks", "",
           "Generated by `python3 tools/plan/plan.py index` from [tasks.toml](tasks.toml). Do not edit by hand.",
           "Live status: the `KONTRA v3` GitHub project and `plan.py ready`.", ""]
    for n, name in plan["phases"].items():
        ts = [t for t in plan["task"] if str(t["phase"]) == n]
        out += [f"## Phase {n}: {name}", "", "| Id | Task | Lane | Size | Agent | Deps |", "|---|---|---|---|---|---|"]
        for t in ts:
            draft = " (draft)" if t.get("draft") else ""
            out.append(f"| {t['id']} | {t['title']}{draft} | {t['lane']} | {t['size']} | {t['agent']} | {', '.join(t.get('deps', [])) or '–'} |")
        out.append("")
    out += ["## Dependency graph", "", "```mermaid", "flowchart LR"]
    for n, name in plan["phases"].items():
        out.append(f'  subgraph ph{n}["{n}. {name}"]')
        out += [f'    {t["id"].replace(".", "_")}["{t["id"]}"]' for t in plan["task"] if str(t["phase"]) == n]
        out.append("  end")
    for t in plan["task"]:
        out += [f'  {d.replace(".", "_")} --> {t["id"].replace(".", "_")}' for d in t.get("deps", [])]
    out += ["```", ""]
    return "\n".join(out)


# ── GitHub ──────────────────────────────────────────────────────────────────

def gh(*args: str, check: bool = True) -> str:
    r = subprocess.run(["gh", *args], capture_output=True, text=True)
    if check and r.returncode:
        raise RuntimeError(f"gh {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def gh_json(*args: str):
    out = gh(*args)
    return json.loads(out) if out.strip() else None


def issues() -> dict[str, dict]:
    """task id / epic key -> issue {number, id, state, labels, assignees, body}."""
    found = {}
    page = 1
    while True:
        batch = gh_json("api", f"repos/{REPO}/issues?labels=v3&state=all&per_page=100&page={page}")
        if not batch:
            break
        for i in batch:
            if "pull_request" in i:
                continue
            m = re.search(r"<!-- v3-(task|epic): ([\w.]+) -->", i.get("body") or "")
            if m:
                found[m[2] if m[1] == "task" else f"epic{m[2]}"] = i
        page += 1
    return found


def status(plan: dict, existing: dict[str, dict]) -> dict[str, str]:
    """done | claimed | ready | blocked, from issue state, assignees and labels."""
    out = {}
    for tid in topo(plan):
        i = existing.get(tid)
        labels = {l["name"] for l in (i or {}).get("labels", [])}
        if i and i["state"] == "closed":
            out[tid] = "done"
        elif i and (i.get("assignees") or "status:claimed" in labels):
            out[tid] = "claimed"
        elif all(out.get(d) == "done" for d in plan["by_id"][tid].get("deps", [])):
            out[tid] = "ready"
        else:
            out[tid] = "blocked"
    return out


def ready_batch(plan: dict, st: dict[str, str]) -> tuple[list[str], list[tuple[str, str, str]]]:
    """Greedy conflict-free batch of ready tasks, given claimed ones already hold their paths."""
    files = list(tracked())
    held = [tid for tid, s in st.items() if s == "claimed"]
    batch, skipped = [], []
    for tid in topo(plan):
        if st[tid] != "ready":
            continue
        mine = owns(plan, plan["by_id"][tid], skip_modes=("append", "regenerate"))
        clash = next(((o, h[0]) for o in held + batch
                      if (h := overlap(mine, owns(plan, plan["by_id"][o], skip_modes=("append", "regenerate")), files))), None)
        if clash:
            skipped.append((tid, clash[0], clash[1]))
        else:
            batch.append(tid)
    return batch, skipped


LABELS = {
    "v3": ("5319e7", "KONTRA v3 refactor (docs/architecture-v3)"),
    "v3-epic": ("3e1f8c", "v3 phase epic"),
    "status:ready": ("0e8a16", "All dependencies merged; can be claimed"),
    "status:blocked": ("b60205", "Waiting on dependencies"),
    "status:claimed": ("fbca04", "Someone (an agent or person) is working on it"),
    "draft-brief": ("d4c5f9", "Scope/steps are a forecast; coordinator re-checks before dispatch"),
    "scope-override": ("e99695", "Coordinator approved changes outside the task's owned paths"),
    "size-growth": ("fbca04", "PR may grow production lines; updates tools/xtask/size-baseline.json"),
    "agent:worker": ("c2e0c6", "Self-contained; an agent can do it from the brief"),
    "agent:coordinator": ("bfd4f2", "Merges, measurements, real libraries, settings"),
    "agent:spike": ("fef2c0", "Research with a written verdict"),
    "size:S": ("ededed", "< half a day of agent work"),
    "size:M": ("cccccc", "about a day"),
    "size:L": ("999999", "several days or a large diff"),
}


def managed(name: str) -> bool:
    """Labels sync owns. Claims and overrides are set by people and kept."""
    if name in ("status:claimed", "scope-override", "size-growth"):
        return False
    return name in LABELS or name.startswith(("phase:", "lane:"))


def sync(plan: dict, dry: bool) -> None:
    say = print if dry else (lambda *a: print(*a, file=sys.stderr))
    # labels
    wanted = dict(LABELS)
    for n, name in plan["phases"].items():
        wanted[f"phase:{n}"] = ("1d76db", f"v3 phase {n}: {name}")
    for lane in sorted(LANES):
        wanted[f"lane:{lane}"] = ("0052cc", f"v3 lane {lane}")
    have = {l["name"] for l in gh_json("label", "list", "--limit", "300", "--json", "name")}
    for name, (color, desc) in wanted.items():
        if name not in have:
            say(f"label + {name}")
            if not dry:
                gh("label", "create", name, "--color", color, "--description", desc)
    # milestones
    ms = {m["title"]: m["number"] for m in gh_json("api", f"repos/{REPO}/milestones?state=all&per_page=100")}
    mtitle = lambda n: f"v3 · P{n} {plan['phases'][n]}"
    for n in plan["phases"]:
        if mtitle(n) not in ms:
            say(f"milestone + {mtitle(n)}")
            if not dry:
                ms[mtitle(n)] = gh_json("api", f"repos/{REPO}/milestones", "-f", f"title={mtitle(n)}",
                                        "-f", f"description=docs/architecture-v3/11-MIGRATION.md phase {n}")["number"]
    existing = issues()

    def upsert(key: str, title: str, body: str, labels: list[str], milestone: int | None, kind: str) -> dict:
        cur = existing.get(key)
        if cur:
            keep = [l["name"] for l in cur["labels"] if not managed(l["name"])]
            new_labels = sorted(set(keep) | set(labels))
            old_labels = sorted(l["name"] for l in cur["labels"])
            if cur["title"] != title or (cur.get("body") or "").strip() != body.strip() or new_labels != old_labels:
                say(f"issue ~ #{cur['number']} {title}")
                if not dry:
                    payload = {"title": title, "body": body, "labels": new_labels}
                    if milestone:
                        payload["milestone"] = milestone
                    subprocess.run(["gh", "api", "-X", "PATCH", f"repos/{REPO}/issues/{cur['number']}", "--input", "-"],
                                   input=json.dumps(payload), text=True, capture_output=True, check=True)
            return cur
        say(f"issue + {title}")
        if dry:
            return {"number": 0, "id": 0, "state": "open", "labels": [], "assignees": []}
        payload = {"title": title, "body": body, "labels": labels, "type": kind}
        if milestone:
            payload["milestone"] = milestone
        r = subprocess.run(["gh", "api", f"repos/{REPO}/issues", "--input", "-"], input=json.dumps(payload),
                           text=True, capture_output=True)
        if r.returncode:  # issue types unavailable: retry without
            payload.pop("type")
            r = subprocess.run(["gh", "api", f"repos/{REPO}/issues", "--input", "-"], input=json.dumps(payload),
                               text=True, capture_output=True, check=True)
        made = json.loads(r.stdout)
        existing[key] = made
        return made

    st = status(plan, existing)
    # pass 1: create every task issue so numbers exist for cross-links
    for tid in topo(plan):
        t = plan["by_id"][tid]
        if tid not in existing:
            upsert(tid, f"[{tid}] {t['title']}", brief(plan, tid), ["v3"], ms.get(mtitle(str(t["phase"]))), "Task")
    numbers = {k: v["number"] for k, v in existing.items()}
    # pass 2: full bodies + labels
    for tid in topo(plan):
        t = plan["by_id"][tid]
        s = st[tid]
        labels = ["v3", f"phase:{t['phase']}", f"lane:{t['lane']}", f"size:{t['size']}", f"agent:{t['agent']}"]
        labels += ["draft-brief"] if t.get("draft") else []
        labels += [f"status:{s}"] if s in ("ready", "blocked") else []
        upsert(tid, f"[{tid}] {t['title']}", brief(plan, tid, numbers), labels, ms.get(mtitle(str(t["phase"]))), "Task")
    # epics
    for n, name in plan["phases"].items():
        ts = [t for t in plan["task"] if str(t["phase"]) == n]
        body = "\n".join([EPIC_MARK.format(phase=n) + " <!-- generated; edit tasks.toml -->",
                          f"Phase {n} of the v3 refactor: **{name}**.", "",
                          f"Plan: [11-MIGRATION.md]({doc_url('11-MIGRATION.md', plan['meta']['branch'])}) · "
                          f"Task index: [TASKS.md]({doc_url('TASKS.md', plan['meta']['branch'])})", "",
                          "Tasks are sub-issues of this epic; the phase gate task closes it."])
        epic = upsert(f"epic{n}", f"v3 Phase {n}: {name}", body, ["v3", "v3-epic", f"phase:{n}"], ms.get(mtitle(n)), "Feature")
        if dry:
            continue
        subs = {i["number"] for i in gh_json("api", f"repos/{REPO}/issues/{epic['number']}/sub_issues?per_page=100") or []}
        for t in ts:
            iss = existing[t["id"]]
            if iss["number"] not in subs:
                say(f"sub-issue #{epic['number']} <- #{iss['number']}")
                gh("api", f"repos/{REPO}/issues/{epic['number']}/sub_issues", "-F", f"sub_issue_id={iss['id']}")
    # dependencies
    if not dry:
        for tid in topo(plan):
            iss = existing[tid]
            have = {i["number"] for i in gh_json("api", f"repos/{REPO}/issues/{iss['number']}/dependencies/blocked_by?per_page=100") or []}
            for d in plan["by_id"][tid].get("deps", []):
                if existing[d]["number"] not in have:
                    say(f"blocked_by #{iss['number']} <- #{existing[d]['number']}")
                    gh("api", f"repos/{REPO}/issues/{iss['number']}/dependencies/blocked_by", "-F", f"issue_id={existing[d]['id']}")
    project(plan, existing, dry, say)


def project(plan: dict, existing: dict, dry: bool, say) -> None:
    title = plan["meta"]["project"]
    projects = gh_json("project", "list", "--owner", OWNER, "--format", "json", "--limit", "100")["projects"]
    p = next((x for x in projects if x["title"] == title), None)
    if not p:
        say(f"project + {title}")
        if dry:
            return
        p = gh_json("project", "create", "--owner", OWNER, "--title", title, "--format", "json")
        gh("project", "link", str(p["number"]), "--owner", OWNER, "--repo", REPO, check=False)
    num = str(p["number"])
    fields = {f["name"]: f for f in gh_json("project", "field-list", num, "--owner", OWNER, "--format", "json", "--limit", "100")["fields"]}
    choice = {"Phase": [f"P{n} {name}" for n, name in plan["phases"].items()], "Lane": sorted(LANES),
              "Size": ["S", "M", "L"], "Agent": sorted(AGENTS)}
    for name, opts in choice.items():
        if name not in fields:
            say(f"project field + {name}")
            if not dry:
                gh("project", "field-create", num, "--owner", OWNER, "--name", name, "--data-type", "SINGLE_SELECT",
                   "--single-select-options", ",".join(opts))
    if "Task" not in fields:
        say("project field + Task")
        if not dry:
            gh("project", "field-create", num, "--owner", OWNER, "--name", "Task", "--data-type", "TEXT")
    if dry:
        return
    fields = {f["name"]: f for f in gh_json("project", "field-list", num, "--owner", OWNER, "--format", "json", "--limit", "100")["fields"]}
    items = {i["content"].get("number"): i for i in gh_json("project", "item-list", num, "--owner", OWNER, "--format", "json", "--limit", "1000")["items"]
             if i.get("content")}
    for tid in topo(plan):
        t, iss = plan["by_id"][tid], existing[tid]
        item = items.get(iss["number"])
        if not item:
            say(f"project item + #{iss['number']}")
            item = gh_json("project", "item-add", num, "--owner", OWNER, "--url", iss["html_url"], "--format", "json")
        want = {"Phase": f"P{t['phase']} {plan['phases'][str(t['phase'])]}", "Lane": t["lane"], "Size": t["size"], "Agent": t["agent"]}
        for name, value in want.items():
            if item.get(name.lower()) == value:
                continue
            opt = next(o["id"] for o in fields[name]["options"] if o["name"] == value)
            gh("project", "item-edit", "--id", item["id"], "--project-id", p["id"], "--field-id", fields[name]["id"],
               "--single-select-option-id", opt)
        if item.get("task") != tid:
            gh("project", "item-edit", "--id", item["id"], "--project-id", p["id"], "--field-id", fields["Task"]["id"], "--text", tid)


def refresh(plan: dict, existing: dict | None = None) -> None:
    existing = existing if existing is not None else issues()
    st = status(plan, existing)
    for tid, s in st.items():
        iss = existing.get(tid)
        if not iss or iss["state"] == "closed":
            continue
        names = {l["name"] for l in iss["labels"]}
        want = {"ready": "status:ready", "blocked": "status:blocked"}.get(s)
        drop = [l for l in ("status:ready", "status:blocked") if l in names and l != want]
        for l in drop:
            gh("issue", "edit", str(iss["number"]), "-R", REPO, "--remove-label", l)
        if want and want not in names:
            print(f"#{iss['number']} {tid} -> {want}")
            gh("issue", "edit", str(iss["number"]), "-R", REPO, "--add-label", want)


def on_merge(plan: dict, branch: str, pr: str) -> None:
    tid = task_of(branch, plan)
    if not tid or tid in SPECIAL:
        print(f"{branch}: not a task branch, nothing to close")
        return
    existing = issues()
    iss = existing.get(tid)
    if iss and iss["state"] == "open":
        gh("issue", "close", str(iss["number"]), "-R", REPO, "--reason", "completed",
           "--comment", f"Merged into `{plan['meta']['branch']}` by #{pr}.")
        iss["state"] = "closed"
        gh("issue", "edit", str(iss["number"]), "-R", REPO, "--remove-label", "status:claimed", check=False)
    refresh(plan, existing)


# ── scope ───────────────────────────────────────────────────────────────────

# Non-task branches into v3 and what they may touch.
SPECIAL = {
    "plan": ["docs/architecture-v3/**", "tools/plan/**", ".github/**", ".claude/skills/**", ".claude/agents/**",
             ".coderabbit.yaml", "AGENTS.md", "CONTRIBUTING.md"],
    "sync-main": ["**"],
}


def task_of(branch: str, plan: dict) -> str | None:
    prefix = plan["meta"]["branch_prefix"]
    for name in SPECIAL:
        if branch.startswith(f"{prefix}{name}-"):
            return name
    m = re.match(re.escape(prefix) + r"(P\d\.\d+[a-z]?)(?:-|$)", branch)
    return m[1] if m and m[1] in plan["by_id"] else None


# Files a tool regenerates from source: any task that owns part of the source may commit them.
GENERATED = {
    "docs/re/ksp/SYMBOLS.tsv": ("crates/sampler-ksp/**", "python3 tools/re/ksp_symbols.py sync"),
}


def allowed(plan: dict, tid: str) -> list[str]:
    """Globs a task's branch may change: its owns, files generated from them, and
    regenerate-mode locks (Cargo.lock, the size baseline), which any task may touch without serializing."""
    out = SPECIAL[tid] if tid in SPECIAL else owns(plan, plan["by_id"][tid])
    out = out + [f for f, (src, _) in GENERATED.items() if overlap(out, [src], [])]
    return out + [g for lock in plan["locks"].values() if lock["mode"] == "regenerate" for g in lock["paths"]]


def scope(plan: dict, tid: str, base: str) -> list[str]:
    diff = subprocess.run(["git", "diff", "--name-only", f"{base}...HEAD"], cwd=ROOT, capture_output=True, text=True, check=True)
    ok = allowed(plan, tid)
    return [f for f in diff.stdout.split() if not match(f, ok)]


# ── cli ─────────────────────────────────────────────────────────────────────

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("validate")
    sub.add_parser("index").add_argument("--check", action="store_true", help="fail if TASKS.md is stale")
    sub.add_parser("brief").add_argument("id")
    r = sub.add_parser("ready")
    r.add_argument("--offline", action="store_true", help="assume nothing is done or claimed")
    r.add_argument("--json", action="store_true")
    s = sub.add_parser("scope")
    s.add_argument("id")
    s.add_argument("--base", default="origin/v3")
    sub.add_parser("task-of").add_argument("branch")
    sub.add_parser("sync").add_argument("--dry-run", action="store_true")
    sub.add_parser("refresh")
    m = sub.add_parser("on-merge")
    m.add_argument("branch")
    m.add_argument("--pr", default="?")
    a = ap.parse_args(argv)
    plan = load()

    if a.cmd == "validate":
        errors, warnings = validate(plan)
        for w in warnings:
            print("warning:", w)
        for e in errors:
            print("error:", e)
        print(f"{len(plan['task'])} tasks, {len(errors)} errors, {len(warnings)} warnings")
        return 1 if errors else 0
    if a.cmd == "index":
        text = index(plan)
        out = ROOT / "docs/architecture-v3/TASKS.md"
        if a.check:
            if not out.exists() or out.read_text() != text:
                print("TASKS.md is stale: run python3 tools/plan/plan.py index")
                return 1
            return 0
        out.write_text(text)
        return 0
    if a.cmd == "brief":
        if a.id not in plan["by_id"]:
            print(f"unknown task {a.id}")
            return 1
        print(brief(plan, a.id), end="")
        return 0
    if a.cmd == "ready":
        st = status(plan, {} if a.offline else issues())
        batch, skipped = ready_batch(plan, st)
        if a.json:
            print(json.dumps({"batch": batch, "skipped": skipped, "status": st}, indent=2))
            return 0
        print("Claimed:", ", ".join(t for t, s in st.items() if s == "claimed") or "none")
        print("Start now (no path conflicts):")
        for t in batch:
            task = plan["by_id"][t]
            print(f"  {t:6} {task['agent']:11} {task['size']}  {task['title']}{'  [draft]' if task.get('draft') else ''}")
        for t, other, why in skipped:
            print(f"  wait   {t}: conflicts with {other} on {why}")
        return 0
    if a.cmd == "scope":
        if a.id not in plan["by_id"] and a.id not in SPECIAL:
            print(f"unknown task {a.id}")
            return 1
        bad = scope(plan, a.id, a.base)
        for f in bad:
            print(f"outside {a.id} scope: {f}")
        print(f"scope {a.id}: {'ok' if not bad else f'{len(bad)} file(s) outside owns'}")
        return 1 if bad else 0
    if a.cmd == "task-of":
        tid = task_of(a.branch, plan)
        print(tid or "")
        return 0 if tid else 1
    if a.cmd == "refresh":
        refresh(plan)
        return 0
    if a.cmd == "on-merge":
        on_merge(plan, a.branch, a.pr)
        return 0
    if a.cmd == "sync":
        errors, _ = validate(plan)
        if errors:
            print("\n".join(errors))
            return 1
        sync(plan, a.dry_run)
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main())
