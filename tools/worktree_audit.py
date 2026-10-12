#!/usr/bin/env python3
"""Read-only recovery inventory; graph divergence is never called missing work.

All Git commands disable optional locks. No checkout, index, ref, or user file is
written. Outputs go only into the explicitly requested output directory.
"""
import argparse
import concurrent.futures
import csv
import datetime
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess


def git(repo, *args, allow_failure=False):
    result = subprocess.run(
        ["git", "--no-optional-locks", "-C", str(repo), *args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        env={**os.environ, "GIT_OPTIONAL_LOCKS": "0"},
    )
    if result.returncode and not allow_failure:
        raise RuntimeError(result.stderr.decode(errors="replace").strip())
    return result


def output(repo, *args):
    return git(repo, *args).stdout.decode(errors="replace").strip()


def tree(repo, revision):
    entries = {}
    for row in git(repo, "ls-tree", "-r", "-z", revision).stdout.split(b"\0"):
        if row:
            metadata, path = row.split(b"\t", 1)
            mode, kind, oid = metadata.decode().split()
            entries[os.fsdecode(path)] = (mode, kind, oid)
    return entries


def parse_worktrees(data):
    records = []
    for row in data.decode(errors="replace").splitlines():
        if row.startswith("worktree "):
            records.append({"path": row[9:]})
        elif row and records:
            key, _, value = row.partition(" ")
            records[-1][key] = value or True
    return records


def parse_status(data):
    rows = data.split(b"\0")
    entries, index = [], 0
    while index < len(rows):
        row = rows[index]
        index += 1
        if not row:
            continue
        entry = {"status": row[:2].decode(), "path": os.fsdecode(row[3:])}
        if "R" in entry["status"] or "C" in entry["status"]:
            entry["original_path"] = os.fsdecode(rows[index])
            index += 1
        entries.append(entry)
    return entries


def area(path):
    if path.startswith(("docs/", "audits/")) or path.endswith(".md"):
        return "documentation"
    if path.startswith(("tools/", "examples/")) or "/examples/" in path:
        return "instrumentation"
    if "/tests/" in path or path.startswith("tests/") or "test" in Path(path).stem:
        return "tests"
    if path.startswith(("target/", ".cache/")):
        return "generated"
    return "source_or_configuration"


def audit(repo, baseline, output_dir, jobs, scan_roots):
    base = output(repo, "rev-parse", baseline + "^{commit}")
    base_tree = tree(repo, base)
    base_commits = set(output(repo, "rev-list", base).splitlines())
    historical_trees = {}
    for row in output(repo, "log", "--format=%H %T", base).splitlines():
        commit, oid = row.split()
        historical_trees.setdefault(oid, []).append(commit)
    branches = []
    for row in git(repo, "for-each-ref", "--format=%(refname:short)\t%(objectname)\t%(upstream:short)", "refs/heads").stdout.decode().splitlines():
        name, tip, upstream = row.split("\t")
        branches.append({"name": name, "tip": tip, "upstream": upstream})
    worktrees = parse_worktrees(git(repo, "worktree", "list", "--porcelain").stdout)
    tips = sorted({b["tip"] for b in branches} | {w["HEAD"] for w in worktrees})

    def tip_record(tip):
        snapshot = tree(repo, tip)
        differing = sorted(path for path in base_tree.keys() | snapshot.keys()
                           if base_tree.get(path) != snapshot.get(path))
        meta = output(repo, "show", "-s", "--format=%T%n%cs%n%s%n%P", tip).splitlines()
        parent = meta[3].split()[0] if len(meta) > 3 and meta[3] else None
        parent_tree = tree(repo, parent) if parent else {}
        changed = sorted(path for path in parent_tree.keys() | snapshot.keys()
                         if parent_tree.get(path) != snapshot.get(path))
        tip_content_equal = [p for p in changed if snapshot.get(p) == base_tree.get(p)]
        left, right = map(int, output(repo, "rev-list", "--left-right", "--count", base + "..." + tip).split())
        return tip, {
            "date": meta[1], "subject": meta[2], "tree": meta[0],
            "reachable_from_baseline": tip in base_commits,
            "graph_baseline_only": left, "graph_tip_only": right,
            "exact_tree_equal_to_baseline": not differing,
            "exact_tree_equal_to_historical_baseline_commits": historical_trees.get(meta[0], []),
            "different_paths_vs_baseline": differing,
            "tip_changed_paths_vs_first_parent": changed,
            "tip_changed_paths_equal_to_baseline": tip_content_equal,
            "tip_changed_scope_entirely_equal_to_baseline": bool(changed) and len(changed) == len(tip_content_equal),
        }

    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        tip_records = dict(pool.map(tip_record, tips))
    for branch in branches:
        branch["analysis"] = tip_records[branch["tip"]]

    def worktree_record(record):
        result = dict(record)
        path = Path(result["path"])
        result["exists"] = path.is_dir()
        result["analysis"] = tip_records[result["HEAD"]]
        result["changes"], result["operations"] = [], {}
        if not result["exists"]:
            return result
        status = git(path, "status", "--porcelain=v1", "-z", "--untracked-files=all", allow_failure=True)
        if status.returncode:
            result["status_error"] = status.stderr.decode(errors="replace").strip()
            return result
        result["changes"] = parse_status(status.stdout)
        for change in result["changes"]:
            filename = path / change["path"]
            change["area"] = area(change["path"])
            change["baseline_has_path"] = change["path"] in base_tree
            if filename.is_file() and not filename.is_symlink():
                size = filename.stat().st_size
                change["size_bytes"] = size
                if size <= 8 * 1024 * 1024:
                    digest = hashlib.sha1(b"blob " + str(size).encode() + b"\0" + filename.read_bytes()).hexdigest()
                    change["working_blob"] = digest
                    change["working_bytes_equal_to_baseline"] = (
                        change["path"] in base_tree and digest == base_tree[change["path"]][2])
        for operation in ("CHERRY_PICK_HEAD", "MERGE_HEAD", "REVERT_HEAD", "rebase-apply", "rebase-merge"):
            gitpath = Path(output(path, "rev-parse", "--path-format=absolute", "--git-path", operation))
            if gitpath.exists():
                result["operations"][operation] = gitpath.read_text().strip() if gitpath.is_file() else "directory exists"
        result["unmerged_index_paths"] = output(path, "diff", "--name-only", "--diff-filter=U").splitlines()
        return result

    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        worktrees = list(pool.map(worktree_record, worktrees))

    discovered, registered = [], {str(Path(w["path"]).resolve()) for w in worktrees}
    common = Path(output(repo, "rev-parse", "--path-format=absolute", "--git-common-dir")).resolve()
    for scan_root in scan_roots:
        root = Path(scan_root)
        if not root.exists():
            continue
        for directory, dirs, files in os.walk(root, followlinks=False):
            dirs[:] = [d for d in dirs if d not in {".git", "target", "node_modules", ".cache", ".venv"}]
            if len(Path(directory).relative_to(root).parts) >= 6:
                dirs[:] = []
            if ".git" not in files or str(Path(directory).resolve()) in registered:
                continue
            resolved = git(directory, "rev-parse", "--path-format=absolute", "--git-common-dir", allow_failure=True)
            discovered.append({"path": directory, "common_git_directory": resolved.stdout.decode().strip(),
                               "same_repository": resolved.returncode == 0 and Path(resolved.stdout.decode().strip()).resolve() == common,
                               "error": resolved.stderr.decode().strip()})

    counts = {
        "local_branches": len(branches), "registered_worktrees": len(worktrees),
        "missing_worktrees": sum(not w["exists"] for w in worktrees),
        "dirty_worktrees": sum(bool(w["changes"]) for w in worktrees),
        "worktrees_with_unmerged_index": sum(bool(w.get("unmerged_index_paths")) for w in worktrees),
        "branches_graph_reachable": sum(b["analysis"]["reachable_from_baseline"] for b in branches),
        "branches_exact_current_tree": sum(b["analysis"]["exact_tree_equal_to_baseline"] for b in branches),
        "unregistered_same_repository_worktrees": sum(d["same_repository"] for d in discovered),
        "unregistered_other_repository_candidates": sum(not d["same_repository"] for d in discovered),
    }
    inventory = {
        "generated_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "baseline_ref": baseline, "baseline_commit": base, "counts": counts,
        "interpretation": [
            "Graph-only commits may have been squash-merged, rewritten, or superseded; they are not proof of missing behavior.",
            "Exact-tree equality proves the entire committed snapshot matches the baseline.",
            "Tip-scope equality proves only files changed by the last commit; preceding branch commits remain unproven.",
            "Working-byte equality compares raw Git blobs; a differing file is not automatically a missing fix.",
            "Operation markers and unmerged index paths are evidence; author intent and reasons need separate handoff records.",
            "Directory scan is limited to requested roots and six levels; it cannot prove every historical checkout still exists.",
        ],
        "branches": branches, "worktrees": worktrees, "unregistered_candidates": discovered,
    }
    output_dir.mkdir(parents=True, exist_ok=True)
    # Normalize repeated tip analyses and compress the full path evidence. The
    # ordinary JSON remains small enough to read and review in the repository.
    normalized = {
        **inventory,
        "branches": [{k: v for k, v in b.items() if k != "analysis"} for b in branches],
        "worktrees": [{k: v for k, v in w.items() if k != "analysis"} for w in worktrees],
        "tip_analyses": tip_records,
    }
    with gzip.open(output_dir / "worktree-inventory-full.json.gz", "wt") as handle:
        json.dump(normalized, handle, separators=(",", ":"))
    bulky = {"different_paths_vs_baseline", "tip_changed_paths_vs_first_parent", "tip_changed_paths_equal_to_baseline"}
    compact = {
        **normalized,
        "full_path_evidence": "worktree-inventory-full.json.gz",
        "tip_analyses": {
            tip: {**{k: v for k, v in record.items() if k not in bulky},
                  **{key + "_count": len(record[key]) for key in bulky}}
            for tip, record in tip_records.items()
        },
    }
    (output_dir / "worktree-inventory.json").write_text(json.dumps(compact, indent=2) + "\n")
    with (output_dir / "branches.tsv").open("w", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t")
        writer.writerow(["branch", "tip", "upstream", "date", "graph_tip_only", "graph_baseline_only", "exact_tree_current", "tip_scope_current", "different_paths", "subject"])
        for branch in branches:
            analysis = branch["analysis"]
            writer.writerow([branch["name"], branch["tip"], branch["upstream"], analysis["date"],
                             analysis["graph_tip_only"], analysis["graph_baseline_only"],
                             analysis["exact_tree_equal_to_baseline"], analysis["tip_changed_scope_entirely_equal_to_baseline"],
                             len(analysis["different_paths_vs_baseline"]), analysis["subject"]])
    with (output_dir / "worktrees.tsv").open("w", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t")
        writer.writerow(["path", "branch", "head", "exists", "changes", "unmerged_paths", "operations", "exact_tree_current"])
        for worktree in worktrees:
            writer.writerow([worktree["path"], worktree.get("branch", "DETACHED"), worktree["HEAD"],
                             worktree["exists"], len(worktree["changes"]),
                             ",".join(worktree.get("unmerged_index_paths", [])), json.dumps(worktree["operations"]),
                             worktree["analysis"]["exact_tree_equal_to_baseline"]])
    print(json.dumps(counts, sort_keys=True))
    return inventory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--baseline", default="origin/main")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--scan-root", type=Path, action="append", default=[])
    args = parser.parse_args()
    audit(args.repo, args.baseline, args.output, args.jobs, args.scan_root)


if __name__ == "__main__":
    main()
