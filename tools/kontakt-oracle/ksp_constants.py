#!/usr/bin/env python3
"""Checks every value in crates/script/ksp/data/constants.tsv against real Kontakt.

ksp_constants.py <instrument.nki> <work dir>

Each constant is compared in a script (`if ($NAME = value)`) that writes the answer into a
group's tune (+1 / -1 units of 0.36 st). A name Kontakt does not know fails its whole script,
so the cases go into script slots 0 and 1 in halves, and a failing half is halved again until
the unknown names are found. A constant that differs is then read in five base-100 digits
over five groups. Prints `name ours -> Kontakt's` for each difference and the unknown names.

Run under tools/oracle/oracle-slot like dump.py; the instrument needs enough groups (ANALOG
STRINGS has 483) and free slots 0 and 1. Results (Kontakt 8.13.1, 2026-10-28) are in
constants.tsv; $NI_SYNC_UNIT_ZONE_LENGTH is unknown to Kontakt 8 and was dropped.
"""

import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TSV = os.path.join(HERE, "../../crates/script/ksp/data/constants.tsv")
HEAD = [
    "local kt = Kontakt",
    'local out = assert(io.open(ORACLE_OUT, "w"))',
    'local function log(t) out:write(t .. "\\n") out:flush() end',
    "kt.reset_multi()",
    "local i = kt.load_instrument(ORACLE_INPUTS[1])",
    'for s = 2, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\\nend on\\n") end',
]


def tune(value, group):
    return f"  set_engine_par($ENGINE_PAR_TUNE, {value}, {group}, -1, -1)"


def probe(cases, halves, digits):
    """The probe's Lua: `halves` are two index ranges of `cases`, one per script slot."""
    lua, groups = list(HEAD), 0
    for slot, (lo, hi) in enumerate(halves):
        src = ["on init", "  declare $v"]
        for k in range(lo, hi):
            name, value = cases[k]
            if digits:
                src.append(f"  $v := {name}")
                for d in range(5):
                    src.append(tune(f"500000 + 5000 * (($v / {100 ** d}) mod 100)", 5 * k + d))
                groups = max(groups, 5 * k + 5)
            else:
                src += [f"  if ({name} = {value})", "  " + tune(505000, k), "  else",
                        "  " + tune(495000, k), "  end if"]
                groups = max(groups, k + 1)
        src.append("end on")
        lua.append(f"local src{slot} = [[\n" + "\n".join(src) + "\n]]")
        lua.append(f"pcall(kt.set_instrument_script_source, i, {slot}, src{slot})")
    lua += ["local t0 = os.time()", "repeat until os.time() - t0 >= 8",
            f"for g = 0, {groups - 1} do",
            '  log(string.format(\'{"g":%d,"u":%.2f}\', g, kt.get_group_tune(i, g) / 0.36))',
            "end", "out:close()"]
    return "\n".join(lua) + "\n"


def run(nki, work, name, lua):
    path = os.path.join(work, name)
    with open(path + ".lua", "w") as f:
        f.write(lua)
    env = dict(os.environ, ORACLE_LUA=path + ".lua", ORACLE_TIMEOUT="500")
    subprocess.run(["../oracle/oracle-slot", "python3", "dump.py", path + ".jsonl", nki],
                   cwd=HERE, env=env, check=False)
    units = {}
    with open(path + ".jsonl") as f:
        for line in f:
            row = json.loads(line)
            if "g" in row:
                units[row["g"]] = round(row["u"])
    return units


def main():
    nki, work = sys.argv[1], sys.argv[2]
    rows = []
    with open(TSV) as f:
        for line in f:
            if line.strip() and not line.startswith("#"):
                name, value = line.rstrip("\n").split("\t")[:2]
                rows.append((name, int(value)))
    same, unknown, todo, n = {}, [], [(0, len(rows))], 0
    while todo:
        lo, hi = todo.pop()
        if hi - lo == 1:
            unknown.append(rows[lo][0])
            continue
        mid = (lo + hi) // 2
        units = run(nki, work, f"eq{n}", probe(rows[lo:hi], [(0, mid - lo), (mid - lo, hi - lo)], False))
        n += 1
        for a, b in [(lo, mid), (mid, hi)]:
            got = [units.get(g - lo) for g in range(a, b)]
            if all(u in (1, -1) for u in got):
                same.update({rows[g][0]: units[g - lo] == 1 for g in range(a, b)})
            else:
                todo.append((a, b))
    differ = [r for r in rows if same.get(r[0]) is False]
    for start in range(0, len(differ), 90):
        chunk = differ[start:start + 90]
        units = run(nki, work, f"digits{start}", probe(chunk, [(0, len(chunk) // 2), (len(chunk) // 2, len(chunk))], True))
        for k, (name, ours) in enumerate(chunk):
            value = sum(units.get(5 * k + d, 0) * 100 ** d for d in range(5))
            print(f"{name}\t{ours} -> {value}")
    for name in unknown:
        print(f"{name}\tunknown to Kontakt")


if __name__ == "__main__":
    main()
