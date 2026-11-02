#!/usr/bin/env python3
"""Re-check a sample of unresolved UI assets with the current build, and decide
whether each is absent from the library (not our gap) or present on disk but
unread (our gap).

Usage: ui_sample.py N OUT.json
"""
import collections, json, os, random, subprocess, sys

from library import library_dir, library_of

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


BIN = os.environ.get("KONTRA_INSPECT", "target/release/kontra-inspect")
RECS = [r for r in json.load(open(f"{WORK}/census.json")) if r.get("loaded")]
CACHE = f"{WORK}/files-index"


def index(root):
    """Lowercased relative paths of every file under root, Samples pruned."""
    os.makedirs(CACHE, exist_ok=True)
    key = os.path.join(CACHE, str(abs(hash(root))) + ".txt")
    if os.path.exists(key):
        return open(key).read().splitlines()
    out = subprocess.run(["find", root, "-path", "*/Samples", "-prune", "-o", "-type", "f", "-print"],
                         capture_output=True, text=True).stdout.splitlines()
    rel = sorted(p[len(root) + 1:].lower() for p in out if p.startswith(root + "/"))
    open(key, "w").write("\n".join(rel))
    return rel


def main():
    n = int(sys.argv[1])
    random.seed(20261101)
    pool = [r for r in RECS if r.get("ui_unresolved", 0)]
    by_lib = collections.defaultdict(list)
    for r in pool:
        by_lib[library_dir(r["path"])].append(r)
    sample = []
    for lib, rs in by_lib.items():
        sample.extend(random.sample(rs, min(len(rs), max(2, n // max(1, len(by_lib))))))
    sample = sample[:n]
    rows = []
    for r in sample:
        probe = f"{WORK}/ui-probe.json"
        p = subprocess.run([BIN, "load", r["path"], "--json", probe],
                           capture_output=True, text=True)
        try:
            assets = [a for m in json.load(open(probe))["result"]["ui"]
                      for a in m.get("assets", [])]
        except Exception:
            rows.append({"path": r["path"], "error": "load failed: " + p.stderr[-200:]})
            continue
        unresolved = [a["path"] for a in assets if a.get("read_status") == "not_resolved"]
        root = library_dir(r["path"])
        files = index(root)
        buckets = collections.Counter()
        examples = collections.defaultdict(list)
        seen = set()
        for ap in unresolved:
            if ap in seen:
                continue
            seen.add(ap)
            want = ap[len("Resources/"):] if ap.startswith("Resources/") else ap
            want = "/" + want.lower()
            hit = next((f for f in files if f.endswith(want) or ("/" + f).endswith(want)), None)
            b = "present_loose_unread" if hit else "absent_from_library"
            buckets[b] += 1
            if len(examples[b]) < 4:
                examples[b].append(hit or ap)
        rows.append({"path": r["path"], "library": library_of(r["path"]),
                     "census_unresolved": r["ui_unresolved"],
                     "census_resolved": r["ui_resolved"],
                     "now_resolved": sum(1 for a in assets if a.get("read_status") == "resolved"),
                     "now_unresolved": len(unresolved),
                     "buckets": dict(buckets), "examples": {k: v for k, v in examples.items()}})
        print(f"{rows[-1].get('library')}: {rows[-1].get('census_unresolved')} -> "
              f"{rows[-1].get('now_unresolved')} unresolved; {dict(buckets)}", flush=True)
    json.dump(rows, open(sys.argv[2], "w"), indent=1)


if __name__ == "__main__":
    main()
