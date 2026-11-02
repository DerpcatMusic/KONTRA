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
    unattributed = 0
    for line in open(tsv):
        # `kspcheck` guarantees one line per input line and prints three fields;
        # a message may contain a tab, so only the first two split.
        parts = line.rstrip("\n").split("\t", 2)
        if len(parts) != 3:
            unattributed += 1
            continue
        kind, path, message = parts
        slots += 1
        if kind != "err":
            ok += 1
            continue
        # `kspcheck` answers `<line N>` for an input line it could not read (not
        # UTF-8, unreadable): there is no instrument to charge it to, and the two
        # fields used to raise IndexError/ValueError out of this loop. Counted
        # and reported, because `kspcheck`'s one-line-per-input contract exists
        # so the corpus cannot shrink quietly.
        instrument = path.split("/")[-2] if "/" in path else ""
        if not instrument.isdigit():
            unattributed += 1
            continue
        errors[instrument].append(message)
    rec = {
        i: {"path": manifest.get(i, f"scripts/{i}"), "errors": errors[i]}
        for i in sorted(errors, key=int)
    }
    with open(out, "w") as f:
        json.dump(rec, f)
    print(
        f"{slots} slots, {ok} compiled, {len(errors)} instruments with a failing slot -> {out}"
        + (f"; {unattributed} line(s) with no instrument directory" if unattributed else ""),
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
