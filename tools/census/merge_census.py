#!/usr/bin/env python3
"""Concatenate the census shards into the single list `census.json` the analysis
scripts read.

`census_sweep.py` writes one JSONL shard per worker; `analyze_census.py`,
`extract.py`, `ui_sample.py` and `full_check.py` all take the concatenation, in
shard order, as a JSON list.

Usage: merge_census.py SHARDS S... OUT.json
       merge_census.py '$CENSUS_WORK/partA/shard-*.jsonl' '$CENSUS_WORK/census.json'
A pattern is expanded by the shell, so quote it only when passing one file.
"""
import glob
import json
import sys


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
    with open(out, "w") as f:
        json.dump(recs, f)
    print(f"{len(recs)} records from {len(paths)} shards -> {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
