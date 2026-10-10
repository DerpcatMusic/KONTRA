"""SOURCE ONLY. Inspect Git bytes and cached primary HTML; execute no Rust/KSP."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

BASE = "13c044d6f9f80576507521a2dc6521e9aad7d9c5"
TIP = "c40a41d87ec6f807e8db236fe9831f58132021fd"
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PACKET = "docs/audit-2026-10-10/ksp-following-reviewed-three-fixes/source-manifest.json"


def git(*args):
    return subprocess.check_output(["git", "-C", str(ROOT), *args])


def sha(data):
    return hashlib.sha256(data).hexdigest()


def data(commit, path):
    return git("show", f"{commit}:{path}")


def pin(commit, path, start=None, end=None):
    body = data(commit, path)
    result = {"commit": commit, "path": path,
              "blob": git("rev-parse", f"{commit}:{path}").decode().strip(),
              "file_sha256": sha(body)}
    if start is not None:
        result.update(lines=[start, end], span_sha256=sha(b"".join(body.splitlines(True)[start-1:end])))
    return result


def tree(commit):
    result = {}
    for entry in git("ls-tree", "-rz", commit).split(b"\0"):
        if entry:
            attrs, path = entry.split(b"\t", 1)
            result[path.decode()] = attrs.decode()
    return result


def diff(commit, path=None):
    args = ["diff", "--no-ext-diff", "--no-textconv", "--no-color", "--no-renames",
            "--diff-algorithm=myers", "--no-indent-heuristic", commit+"^", commit]
    return git(*args, *(["--", path] if path else []))


def delta(patch):
    result = {}
    for line in patch.splitlines(True):
        if line.startswith(b"diff --git "):
            path = line.split(b" b/", 1)[1].rstrip(b"\n").decode()
            result[path] = {"added": b"", "removed": b""}
        elif line.startswith(b"+") and not line.startswith(b"+++"):
            result[path]["added"] += line[1:]
        elif line.startswith(b"-") and not line.startswith(b"---"):
            result[path]["removed"] += line[1:]
    return result


def replay(commit, path):
    # Exact zero-context unified hunks, applied to immutable parent lines in memory.
    patch = git("diff", "--no-ext-diff", "--no-textconv", "--no-color", "--no-renames",
                "--diff-algorithm=myers", "--no-indent-heuristic", "--unified=0",
                commit+"^", commit, "--", path)
    parent = tree(commit+"^")
    before = data(commit+"^", path).splitlines(True) if path in parent else []
    result, cursor, old, new, position = [], 0, [], [], None
    def flush():
        nonlocal cursor
        if position is not None:
            assert before[position:position+len(old)] == old, (commit, path, position)
            result.extend(before[cursor:position])
            result.extend(new)
            cursor = position + len(old)
    for line in patch.splitlines(True):
        if line.startswith(b"@@"):
            flush()
            match = re.match(rb"@@ -(\d+)(?:,(\d+))? \+", line)
            length = int(match[2]) if match[2] is not None else 1
            position = int(match[1]) - (1 if length else 0)
            old, new = [], []
        elif position is not None and line.startswith(b"-"):
            old.append(line[1:])
            last_side = old
        elif position is not None and line.startswith(b"+"):
            new.append(line[1:])
            last_side = new
        elif line.startswith(b"\\"):
            assert last_side[-1].endswith(b"\n")
            last_side[-1] = last_side[-1][:-1]
    flush()
    result.extend(before[cursor:])
    assert b"".join(result) == data(commit, path), (commit, path)


def main():
    handoff = Path(sys.argv[1])
    assert git("rev-parse", "HEAD").decode().strip() == BASE
    assert not git("status", "--porcelain", "--untracked-files=no")
    manifest = json.loads(data(TIP, PACKET))
    original = handoff / "ksp-following-reviewed-three-fixes.json"
    review = handoff / "ksp-three-fixes-text-successor-review.md"
    for source, name in [(original, "original-packet.json"), (review, "review-snapshot.md")]:
        destination = HERE / name
        if destination.exists():
            assert destination.read_bytes() == source.read_bytes()
        else:
            destination.write_bytes(source.read_bytes())
    git("merge-base", "--is-ancestor", BASE, TIP)
    commits = git("rev-list", "--reverse", BASE+".."+TIP).decode().splitlines()
    chain, previous = [], BASE
    for commit in commits:
        fields = git("show", "-s", "--format=%P%n%T%n%s", commit).decode().splitlines()
        assert fields[0] == previous
        paths = git("diff-tree", "--no-commit-id", "--name-only", "-r", commit).decode().splitlines()
        for path in paths:
            replay(commit, path)
        chain.append({"commit": commit, "parent": previous, "tree": fields[1],
                      "subject": fields[2], "patch_sha256": sha(diff(commit)), "paths": paths})
        previous = commit
    cherries = []
    for item in manifest["ordered_cherries"]:
        a, b = item["original"]["commit"], item["assembled"]["commit"]
        da, db = delta(diff(a)), delta(diff(b))
        assert da == db
        expected = item["canonical"]["exact_shared_added_removed_bytes"]
        for path, sides in da.items():
            for side, body in sides.items():
                assert sha(body) == expected[path][side]["sha256"]
        ids = [subprocess.check_output(["git", "patch-id", "--stable"], input=diff(c)).decode().split()[0]
               for c in (a, b)]
        cherries.append({"original": a, "assembled": b, "added_removed_bytes_equal": True,
                         "stable_patch_ids": ids, "stable_patch_ids_equal": ids[0] == ids[1]})
    bt, tt = tree(BASE), tree(TIP)
    changed = sorted(p for p in bt.keys() | tt.keys() if bt.get(p) != tt.get(p))
    unchanged = sorted(p for p in bt if bt[p] == tt.get(p))
    assert len(changed) == 16 and len(unchanged) == 2353
    assert chain[-1]["paths"] == ["crates/sampler-ksp/tests/strings.rs"]
    assert chain[-2]["paths"] == [PACKET.replace("source-manifest.json", "REPORT.md"), PACKET]
    tests = []
    for item in manifest["tests"]:
        e = item["evidence"]
        p = pin(TIP, item["path"], e["start_line"], e["end_line"])
        body = data(TIP, item["path"])
        name = item["filter"].split("::")[-1]
        assert len(re.findall(rb"#\[test\]\s*fn " + name.encode() + rb"\(", body)) == 1
        assert b"#[ignore" not in body.splitlines(True)[e["start_line"]-2]
        tests.append({k: item[k] for k in ("package", "target", "filter", "exact", "status")} | {"evidence": p})
    assert len(tests) == 14
    assert b'mod display_ext_tests;' in data(TIP, "crates/sampler-ksp/src/lower.rs")
    assert b'name = "sampler-ksp"' in data(TIP, "crates/sampler-ksp/Cargo.toml")
    from bs4 import BeautifulSoup
    primary = []
    for item in manifest["primary_evidence"]:
        archive = Path(item["archive"]).read_bytes()
        excerpt = Path(item["owned_excerpt_file"]).read_bytes()
        assert sha(archive) == item["document_sha256"]
        assert sha(excerpt) == item["excerpt_sha256"]
        soup = BeautifulSoup(archive, "html.parser")
        element = soup.find(id=item["url"].split("#")[1])
        section = element if element.name == "section" else element.find_parent("section")
        text = section.get_text(" ", strip=True).encode()
        assert text == excerpt == item["excerpt"].encode()
        primary.append({k: item[k] for k in ("url", "section", "document_sha256", "excerpt_sha256", "retrieved_utc", "excerpt")})
    git("diff", "--check", BASE, TIP)
    result = {"status": "SOURCE_READY_PREPARATION_ONLY_NOT_AUTHORIZED_NOT_MERGED",
              "base": {"commit": BASE, "tree": git("rev-parse", BASE+"^{tree}").decode().strip()},
              "candidate": {"commit": TIP, "tree": git("rev-parse", TIP+"^{tree}").decode().strip()},
              "ordered_stack": chain, "original_cherries": cherries,
              "changed_files": [pin(TIP, p) | {"base_blob": bt.get(p)} for p in changed],
              "preservation": {"base_unchanged_paths": len(unchanged), "candidate_entries": len(tt),
                               "all_seven_commit_patches_replayed_in_memory": True,
                               "only_two_production_files_changed": ["crates/sampler-ksp/src/eval.rs", "crates/sampler-ksp/src/lower.rs"]},
              "immutable_inputs": [pin(TIP, PACKET),
                  {"path": "original-packet.json", "sha256": sha(original.read_bytes())},
                  {"path": "review-snapshot.md", "sha256": sha(review.read_bytes())}],
              "primary_spec_only": primary, "tests": tests,
              "limits": {"new_tests": 13, "changed_legacy_tests": 1,
                         "rust": "NOT_RUN_NOT_COMPILED", "native": "UNKNOWN",
                         "CPU_AND_RAM_below_BOTH_v1_Kontakt": "UNACHIEVED",
                         "build_authorization": False, "automatic_merge": False,
                         "release_checkout_writes": False, "mutable_shared_evidence_dependencies": False}}
    destination = HERE / "source-ready.json"
    assert not destination.exists()
    destination.write_text(json.dumps(result, indent=2, ensure_ascii=False)+"\n")
    print(json.dumps({"status": result["status"], "commits": len(chain), "changed": len(changed),
                      "tests": len(tests), "primary": len(primary), "sha256": sha(destination.read_bytes())}))


if __name__ == "__main__":
    main()
