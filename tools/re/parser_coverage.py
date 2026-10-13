#!/usr/bin/env python3
"""Join Kontakt's serializable-class catalog with what vendor/ni-file decodes.

    tools/re/parser_coverage.py ser_classes.tsv reader_schemas.tsv > parser_coverage.md

Two kinds of decoder are counted per class:
  * native schema: vendor/ni-file/src/kontakt/native_schema (generated from Kontakt's own
    readers; reader_schemas.tsv lists the stored versions it accepts and how completely)
  * hand reader: ni-file object docs declare `SerType: 0xNN` and `Versions: ...`

A class is `current` when some decoder accepts the newest version Kontakt 8.13.1 writes.
"""
import csv
import re
import sys
from collections import OrderedDict
from pathlib import Path

hand = {}
for f in Path("vendor/ni-file/src/kontakt").rglob("*.rs"):
    if "native_schema" in f.parts:
        continue
    text = f.read_text(errors="replace")
    for block in re.split(r"\n(?=///\s*#\s)|\n(?=/// Type:)", text):
        st = re.search(r"SerType:\s*(0x[0-9A-Fa-f]+)", block)
        if not st:
            continue
        vs = re.search(r"Versions?:\s*([^\n]+)", block)
        versions = set()
        if vs:
            for lo, hi in re.findall(r"(0x[0-9A-Fa-f]+)(?:\s*(?:\.\.=?|-)\s*(0x[0-9A-Fa-f]+))?", vs.group(1)):
                a = int(lo, 16)
                versions |= set(range(a, int(hi, 16) + 1)) if hi else {a}
        key = int(st.group(1), 16)
        hand.setdefault(key, [set(), set()])
        hand[key][0] |= versions
        hand[key][1].add(f.name)

classes = list(csv.DictReader(open(sys.argv[1]), delimiter="\t"))
native = OrderedDict()  # (class, sertype) -> {version: status}
for r in csv.DictReader(open(sys.argv[2]), delimiter="\t"):
    key = (r["class"], int(r["sertype"], 16))
    entry = native.setdefault(key, {"versions": {}, "stub": None})
    if r["stored_version"] == "-":
        entry["stub"] = r["status"]
    else:
        entry["versions"][int(r["stored_version"], 16)] = r["status"]


def ranges(vs):
    out, vs = [], sorted(vs)
    for v in vs:
        if out and out[-1][1] + 1 == v:
            out[-1][1] = v
        else:
            out.append([v, v])
    return ", ".join(f"0x{a:X}" if a == b else f"0x{a:X}-0x{b:X}" for a, b in out) or "-"


counts = dict(typed=0, caveat=0, behind=0, none=0)
print("| SerType | Kontakt class | native version | native schema | hand reader | status |")
print("|---|---|---|---|---|---|")
for r in classes:
    st, ver = int(r["sertype"], 16), int(r["version"], 16)
    n = native.get((r["class"], st), {"versions": {}, "stub": None})
    h = hand.get(st)
    h_has = bool(h and ver in h[0])
    status_n = n["versions"].get(ver)
    if status_n == "typed":
        status, bucket = "current", "typed"
    elif status_n:
        status, bucket = f"current ({status_n})", "caveat"
    elif h_has:
        status, bucket = "current (hand)", "typed"
    elif n["stub"]:
        status, bucket = f"no stream reader ({n['stub']})", "none"
    elif h or n["versions"]:
        status, bucket = "**native version missing**", "behind"
    else:
        status, bucket = "**no reader**", "none"
    counts[bucket] += 1
    schema = ranges(n["versions"]) if n["versions"] else (n["stub"] or "-")
    hv = ranges(h[0]) if h and h[0] else ("undeclared" if h else "")
    print(f"| 0x{st:02X} | {r['class']} | 0x{ver:X} | {schema} | {hv} | {status} |")
total = len(classes)
print(f"\n{total} native classes: {counts['typed']} current and fully typed, "
      f"{counts['caveat']} current with caveats (host-state branch, nested object of unnamed class, "
      f"or virtual dispatch), {counts['behind']} behind Kontakt 8.13.1, "
      f"{counts['none']} with no stream reader (the reader slot is a stub or not a reader).")

# Same count by the original rule (hand readers only), for before/after comparison.
h_ok = h_old = h_none = 0
for r in classes:
    st, ver = int(r["sertype"], 16), int(r["version"], 16)
    h = hand.get(st)
    if not h:
        h_none += 1
    elif ver in h[0]:
        h_ok += 1
    else:
        h_old += 1
print(f"\nHand readers alone: {h_ok} current, {h_old} behind Kontakt 8.13.1, {h_none} with no reader.")
