#!/usr/bin/env python3
"""Decode a persistence probe run (tools/ksp-conformance/probes/persistence_roundtrip.lua)
into golden values for the interpreter test.

    tools/ksp-conformance/persistence_decode.py RUN.jsonl [OUT.json]

Each variant's encoded points are 8-bit values. Bit b of point k is group base_k + b;
a group read back below -100 dB is a set bit (a set bit is -167 dB, a clear one stays at
0 dB). Output: {variant: {"source": <loader script of the writer A>, "points": {A,B,C,D,S}}}.
The writer's source is the script text; the sources are KONTRA probe text, not library code.
"""
import json
import sys
from pathlib import Path

BASES = {"A": 0, "B": 8, "C": 16, "D": 24, "S": 32}
SET_DB = -100.0


def decode(run: Path) -> dict:
    sources, readbacks = {}, {}
    for line in run.read_text().splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        if "source" in row:
            sources[row["case"].removesuffix("_A")] = row["source"]
        elif "group_volume_db" in row:
            readbacks[row["case"]] = row["group_volume_db"]
    golden = {}
    for name, groups in readbacks.items():
        points = {}
        for point, base in BASES.items():
            value = 0
            for bit in range(8):
                if groups[base + bit] < SET_DB:
                    value |= 1 << bit
            points[point] = value
        # Group 64 is written first in on init, 65 last: a missing start marker means Kontakt
        # did not compile the script; a missing end marker means init stopped early.
        compiled = groups[64] < SET_DB
        completed = groups[65] < SET_DB
        # Group 66 is written first in on persistence_changed. Loads through the Lua API did not
        # run it (see docs/re/ksp/PERSISTENCE_CENSUS.md), so this stays clear there.
        golden[name] = {"source": sources.get(name, ""), "compiled": compiled,
                        "completed": completed, "persistence_changed_ran": groups[66] < SET_DB,
                        "points": points}
    return golden


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    golden = decode(Path(argv[1]))
    text = json.dumps(golden, indent=1, sort_keys=True) + "\n"
    if len(argv) > 2:
        Path(argv[2]).write_text(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
