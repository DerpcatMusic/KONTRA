#!/usr/bin/env python3
"""Join Kontakt's serializable-class catalog with what vendor/ni-file decodes.

    tools/re/parser_coverage.py docs/re/kontakt-catalog/ser_classes.tsv > coverage.md

ni-file object docs declare `SerType: 0xNN` and `Versions: ...`; a SerType with no
reader, or whose newest native version is not in the declared list, is a gap.
"""
import csv, re, sys
from pathlib import Path

ours = {}
for f in Path("vendor/ni-file/src/kontakt").rglob("*.rs"):
    text = f.read_text(errors="replace")
    for block in re.split(r"\n(?=///\s*#\s)", text):
        st = re.search(r"SerType:\s*(0x[0-9A-Fa-f]+)", block)
        if not st:
            continue
        vs = re.search(r"Versions?:\s*([^\n]+)", block)
        versions = {int(v, 16) for v in re.findall(r"0x[0-9A-Fa-f]+", vs.group(1))} if vs else set()
        ours.setdefault(int(st.group(1), 16), (set(), set()))
        ours[int(st.group(1), 16)][0].update(versions)
        ours[int(st.group(1), 16)][1].add(str(f.relative_to("vendor/ni-file/src")))

rows = list(csv.DictReader(open(sys.argv[1]), delimiter="\t"))
ok = old = missing = 0
print("| SerType | Kontakt class | native version | ni-file versions | status |")
print("|---|---|---|---|---|")
for r in rows:
    st, ver = int(r["sertype"], 16), int(r["version"], 16)
    have = ours.get(st)
    if not have:
        status, missing = "**no reader**", missing + 1
        vs = ""
    else:
        vs = ", ".join(f"0x{v:X}" for v in sorted(have[0])) or "undeclared"
        if ver in have[0]:
            status, ok = "current", ok + 1
        else:
            status, old = "**native version missing**", old + 1
    print(f"| 0x{st:02X} | {r['class']} | 0x{ver:X} | {vs} | {status} |")
print(f"\n{len(rows)} native classes: {ok} current, {old} behind Kontakt 8.13.1, {missing} with no reader.")
