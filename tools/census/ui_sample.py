#!/usr/bin/env python3
"""Re-check a sample of unresolved UI assets with the current build, and decide
whether each is absent from the library (not our gap) or present on disk but
unread (our gap).

Usage: ui_sample.py N OUT.json
"""
import collections, hashlib, json, os, random, subprocess, sys

from library import library_dir, library_of

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


BIN = os.environ.get("KONTRA_INSPECT", "target/release/kontra-inspect")
RECS = [r for r in json.load(open(f"{WORK}/census.json")) if r.get("loaded")]
CACHE = f"{WORK}/files-index"
#: Seconds one `kontra-inspect load` may take. A hung load used to block the run
#: with no way to tell it from a slow one.
TIMEOUT = float(os.environ.get("UI_SAMPLE_TIMEOUT", "600"))


def index(root):
    """Lowercased relative paths of every file under root, Samples pruned."""
    os.makedirs(CACHE, exist_ok=True)
    # `hash()` of a str is salted per process, so the old key never matched
    # between two runs and the index was rebuilt every time. A digest of the
    # path is stable, and the file it names is the same index.
    key = os.path.join(CACHE, hashlib.sha256(root.encode()).hexdigest()[:16] + ".txt")
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
    # Shuffle before truncating. The per-library quota is at least two, so with
    # more libraries than n/2 the list is longer than n, and `sample[:n]` kept
    # the libraries that happen to come first in `census.json`: 44 rows covered
    # 22 of 49 libraries and the rest could never have been sampled. The seed
    # keeps the run deterministic.
    random.shuffle(sample)
    sample = sample[:n]
    rows = []
    probe = f"{WORK}/ui-probe.json"
    for r in sample:
        # One probe path for every instrument, so it is removed before each load:
        # a load that fails before writing it used to leave the previous
        # instrument's JSON in place, and its asset counts were attributed to
        # this one with no error.
        try:
            os.unlink(probe)
        except FileNotFoundError:
            pass
        try:
            p = subprocess.run([BIN, "load", r["path"], "--json", probe],
                               capture_output=True, text=True, timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            rows.append({"path": r["path"], "error": f"load timed out after {TIMEOUT}s"})
            continue
        if p.returncode != 0:
            rows.append({"path": r["path"],
                         "error": "load failed: " + (p.stderr or "").strip()[-200:]})
            continue
        try:
            assets = [a for m in json.load(open(probe))["result"]["ui"]
                      for a in m.get("assets", [])]
        except Exception:
            rows.append({"path": r["path"], "error": "load wrote no usable summary: "
                         + (p.stderr or "").strip()[-200:]})
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
            # The reference is library-relative (`Resources/pictures/x.png`), so
            # the file the library would have to hold is that exact path. The old
            # test compared the tail after `Resources/` against every path ending,
            # so `Resources/Resources/pictures/x.png` -- the layout of two Muze
            # products -- counted as the reference being present when it sits one
            # directory deeper than the script asks for.
            want = ap.lower().lstrip("/")
            hit = next((f for f in files if f == want), None)
            if hit is None and not want.startswith("resources/"):
                # An instrument-relative spelling (`pictures/x.png`), which the
                # library would hold under its own `Resources/`.
                hit = next((f for f in files if f == "resources/" + want), None)
            b = "present_loose_unread" if hit else "absent_from_library"
            buckets[b] += 1
            if len(examples[b]) < 4:
                examples[b].append(hit or ap)
        rows.append({"path": r["path"], "library": library_of(r["path"]),
                     # Measured, not asserted: whether this library ships a
                     # `pictures/` directory at all, so the summary can say how
                     # many do without a typed count.
                     "library_has_pictures": any("pictures/" in f for f in files),
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
