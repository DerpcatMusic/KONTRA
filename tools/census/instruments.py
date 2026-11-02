#!/usr/bin/env python3
"""Instrument-level before/after from two kspcheck runs and the manifest.

`kspcheck` prints `ok|err<TAB><path><TAB><message>`, where `<path>` is the
script file it compiled — `$CENSUS_WORK/scripts/<instrument>/slot-<n>.ksp` when
the shell expands the README's `ls "$CENSUS_WORK"/scripts/*/slot-*.ksp`. The
instrument is the component after `scripts`, not the second component of the
path, so the same parse works for a relative, an absolute or a `..`-prefixed
path.

Usage: instruments.py   # reads compile-before.tsv and compile-after2.tsv
"""
import json
import os
import sys

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


def instrument_of(script):
    """`…/scripts/<instrument>/slot-<n>.ksp` -> `<instrument>`."""
    parts = script.split("/")
    if "scripts" in parts:
        i = len(parts) - 1 - parts[::-1].index("scripts")
        if i + 1 < len(parts) - 1:
            return parts[i + 1]
    return parts[-2]


def load(path):
    """(instruments that fail, slots that fail) in one `compile.tsv`."""
    fails, slots = set(), set()
    for line in open(path):
        p = line.rstrip("\n").split("\t")
        if p[0] == "err":
            fails.add(instrument_of(p[1]))
            slots.add(p[1])
    return fails, slots


def main():
    before, before_slots = load(f"{WORK}/compile-before.tsv")
    after, after_slots = load(f"{WORK}/compile-after2.tsv")
    names = {}
    for line in open(f"{WORK}/manifest.jsonl"):
        r = json.loads(line)
        names[str(r["i"])] = r["name"]
    fixed = sorted(before - after, key=int)
    print(f"instruments failing before: {len(before)}")
    print(f"instruments failing after:  {len(after)}")
    print(f"instruments fixed:          {len(fixed)}")
    print(f"slots fixed:                {len(before_slots - after_slots)}")
    print()
    for i in fixed:
        print(f"{i}\t{names.get(i, '?')}")


if __name__ == "__main__":
    sys.exit(main())
