#!/usr/bin/env python3
"""Turn a `kspcheck` run into the per-instrument failure record `build_tsv.py`
reads: one entry per instrument that has a slot which does not compile.

`kspcheck` prints `ok|err<TAB><path><TAB><message>` per slot, where `<path>` is
`$CENSUS_WORK/scripts/<instrument>/slot-<n>.ksp` — so the instrument is the
numeric directory, and an instrument with two failing slots appears once with
two messages.

Usage: ls $CENSUS_WORK/scripts/*/slot-*.ksp | kspcheck > compile.tsv
       compile_fail.py compile.tsv $CENSUS_WORK/compile_fail.json
"""
import collections
import json
import os
import sys


def main():
    tsv, out = sys.argv[1], sys.argv[2]
    work = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")
    manifest = {}
    for line in open(os.path.join(work, "manifest.jsonl")):
        if line.strip():
            r = json.loads(line)
            manifest[str(r["i"])] = r["path"]
    errors = collections.defaultdict(list)
    slots = 0
    ok = 0
    for line in open(tsv):
        kind, path, message = line.rstrip("\n").split("\t")
        slots += 1
        if kind != "err":
            ok += 1
            continue
        instrument = path.split("/")[-2]
        errors[instrument].append(message)
    rec = {
        i: {"path": manifest.get(i, f"scripts/{i}"), "errors": errors[i]}
        for i in sorted(errors, key=int)
    }
    with open(out, "w") as f:
        json.dump(rec, f)
    print(
        f"{slots} slots, {ok} compiled, {len(errors)} instruments with a failing slot -> {out}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
