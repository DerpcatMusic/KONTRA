#!/usr/bin/env python3
"""Merges `ksp_constants.py names.txt` output into crates/script/ksp/data/constants.tsv.

    apply_constants.py MEASURED.txt [--date 2026-11-14]

Lines are `$NAME<TAB>-7 -> VALUE` (measured) or `$NAME<TAB>unknown to Kontakt`.
Measured names are appended under a dated comment unless the table has them;
unknown names are listed on stderr and left alone. Run from anywhere.
"""
import pathlib
import sys

TSV = pathlib.Path(__file__).resolve().parent / "../../crates/script/ksp/data/constants.tsv"
have = {l.split("\t")[0] for l in TSV.read_text().splitlines() if l and l[0] != "#"}
new, unknown = [], []
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    if "\t" not in line:
        continue
    name, rest = line.split("\t", 1)
    if "->" in rest and name not in have:
        new.append((name, int(rest.split("->")[1])))
    elif "unknown" in rest:
        unknown.append(name)
date = sys.argv[sys.argv.index("--date") + 1] if "--date" in sys.argv else "today"
if new:
    with TSV.open("a") as f:
        f.write(f"# Measured in real Kontakt 8.13.1 ({date}, tools/kontakt-oracle/ksp_constants.py names.txt).\n")
        for name, value in sorted(new):
            f.write(f"{name}\t{value}\n")
print(f"{len(new)} added, {len(unknown)} unknown to Kontakt: {' '.join(unknown)}", file=sys.stderr)
