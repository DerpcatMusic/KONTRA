#!/usr/bin/env python3
"""Concatenate the census shards into the single list `census.json` the analysis
scripts read.

`census_sweep.py` writes one JSONL shard per worker; `analyze_census.py`,
`extract.py`, `ui_sample.py` and `full_check.py` all take the concatenation, in
shard order, as a JSON list.

The merge is keyed on the instrument's `path`, last record wins. That is what
makes a re-run safe: `census_sweep.py` appends to `shard-<label>.jsonl`, so
running a shard twice into the same directory leaves two records for every file
it swept. Keying on the path means the second run replaces the first instead of
duplicating it — and a path that appears in two shards keeps the later record,
which is the one a later build measured. A path keeps the position of its first
appearance, so the order stays the shard order.

Usage: merge_census.py SHARDS S... OUT.json
       merge_census.py '$CENSUS_WORK/partA/shard-*.jsonl' '$CENSUS_WORK/census.json'
A pattern is expanded by the shell, so quote it only when passing one file.
"""
import glob
import json
import sys


def merge(records):
    """Deduplicate `records` by `path`: last write wins, first position kept."""
    by_path = {}
    for r in records:
        by_path[r["path"]] = r
    return list(by_path.values())


def main():
    args = sys.argv[1:]
    if len(args) < 2:
        sys.exit(__doc__)
    out = args[-1]
    paths = []
    for a in args[:-1]:
        paths.extend(sorted(glob.glob(a)) or [a])
    recs = []
    for p in paths:
        with open(p) as f:
            for line in f:
                line = line.strip()
                if line:
                    recs.append(json.loads(line))
    merged = merge(recs)
    with open(out, "w") as f:
        json.dump(merged, f)
    dupes = len(recs) - len(merged)
    print(f"{len(recs)} records from {len(paths)} shards -> {len(merged)} instruments in {out}"
          + (f" ({dupes} repeated paths replaced by their last record)" if dupes else ""),
          file=sys.stderr)


if __name__ == "__main__":
    main()
