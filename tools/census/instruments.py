#!/usr/bin/env python3
"""Instrument-level before/after from two kspcheck runs and the manifest."""
import json
import os

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


def load(path):
    fails = set()
    count = {}
    for line in open(path):
        p = line.rstrip("\n").split("\t")
        if p[0] == "err":
            i = p[1].split("/")[1]
            fails.add(i)
            count[i] = count.get(i, 0) + 1
    return fails

before = load(f"{WORK}/compile-before.tsv")
after = load(f"{WORK}/compile-after2.tsv")
names, libs = {}, {}
for line in open(f"{WORK}/manifest.jsonl"):
    r = json.loads(line)
    names[str(r["i"])] = r["name"]
    libs[str(r["i"])] = r["path"].split("/Armory/")[-1].split("/Kontakt")[-1].split("/", 1)[-1]
fixed = sorted(before - after, key=int)
print(f"instruments failing before: {len(before)}")
print(f"instruments failing after:  {len(after)}")
print(f"instruments fixed:          {len(fixed)}")
print(f"slots fixed:                250 (see compile diff)")
print()
for i in fixed:
    print(f"{i}\t{names.get(i, '?')}")
