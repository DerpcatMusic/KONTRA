#!/usr/bin/env python3
"""Runs a KSP probe in real Kontakt and prints what it measured.

ksp_probe.py <instrument.nki> <probe.kspp> [work dir]   (ORACLE_PROBE_NKR=<.nkr> lends it pictures)

A probe file has one line per step (`#` starts a comment):

    S <statement>     run in `on init`, in order (declarations too)
    R <label> <expr>  record the integer `expr` (evaluated in order with the statements)
    T <label> <expr>  record the string `expr`: saved with save_array_str at the end of init and
                      printed verbatim (KSP has no string length or comparison to encode it)
    C <name> <expr>   a callback script: `<name>` is the callback's header line, e.g.
                      `C on async_complete` followed by indented `S`/`R` lines is NOT supported;
                      use `X <text>` lines instead: raw KSP appended after `on init ... end on`.

Each recorded value is written into the tune of five groups (five base-100 digits of
value + 1 000 000, so negatives work), read back with `get_group_tune`, and printed as
`label<TAB>value`. Run under tools/oracle/oracle-slot like ksp_constants.py (this tool calls it
itself). The instrument needs enough groups (ANALOG STRINGS has 483, so 96 results).
An unknown name (not in the KSP manual, or new in a later Kontakt) fails the whole script and every
value reads -1000000: use manual names only. `cargo test -p kontra-ksp every_oracle_probe_compiles`
compiles every probe with our compiler first, which catches typos. Results, once a law is decided, go
in the probe's header comment and in the code that uses them.
"""

import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
HEAD = [
    "local kt = Kontakt",
    'local out = assert(io.open(ORACLE_OUT, "w"))',
    'local function log(t) out:write(t .. "\\n") out:flush() end',
    "kt.reset_multi()",
    "local i = kt.load_instrument(ORACLE_INPUTS[1])",
    'for s = 1, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\\nend on\\n") end',
]
OFFSET = 1_000_000


def script(lines, nka=None):
    init, extra, labels, texts = ["on init", "  declare $v"], [], [], []
    for raw in lines:
        line = raw.rstrip("\n")
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        kind, _, rest = line.strip().partition(" ")
        if kind == "S":
            init.append("  " + rest)
        elif kind == "R":
            label, _, expr = rest.partition(" ")
            k = len(labels)
            labels.append(label)
            init.append(f"  $v := ({expr}) + {OFFSET}")
            for d in range(5):
                init.append(
                    f"  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (($v / {100 ** d}) mod 100), {5 * k + d}, -1, -1)"
                )
        elif kind == "T":
            label, _, expr = rest.partition(" ")
            init.append(f"  !oracle_t[{len(texts)}] := {expr}")
            texts.append(label)
        elif kind == "X":
            extra.append(rest)
        else:
            raise SystemExit(f"bad probe line: {line}")
    if texts:
        init.insert(2, f"  declare !oracle_t[{len(texts)}]")
        init.append(f'  save_array_str(!oracle_t, "Z:{nka}")')
    init.append("end on")
    return "\n".join(init + extra) + "\n", labels, texts


def main():
    nki, probe = sys.argv[1], sys.argv[2]
    work = sys.argv[3] if len(sys.argv) > 3 else "/tmp/ksp-probe"
    os.makedirs(work, exist_ok=True)
    nka = os.path.abspath(os.path.join(work, os.path.basename(probe) + ".nka"))
    if os.path.exists(nka):
        os.remove(nka)
    with open(probe) as f:
        src, labels, texts = script(f, nka)
    lua = list(HEAD)
    # ORACLE_PROBE_NKR=<.nkr>: give the instrument that resource container (pictures, fonts).
    nkr = os.environ.get("ORACLE_PROBE_NKR")
    if nkr:
        win = "Z:" + os.path.abspath(nkr).replace("/", "\\")
        lua.append(f"pcall(kt.create_resource_container, i, [[{win}]])")
    lua.append(f"local src = [[\n{src}]]")
    lua.append("pcall(kt.set_instrument_script_source, i, 0, src)")
    lua += [
        "local t0 = os.time()",
        "repeat until os.time() - t0 >= 45",
        f"for g = 0, {5 * len(labels) - 1} do",
        '  log(string.format(\'{"g":%d,"u":%.2f}\', g, kt.get_group_tune(i, g) / 0.36))',
        "end",
        "out:close()",
    ]
    name = os.path.join(work, os.path.basename(probe))
    with open(name + ".lua", "w") as f:
        f.write("\n".join(lua) + "\n")
    env = dict(os.environ, ORACLE_LUA=name + ".lua", ORACLE_TIMEOUT="300")
    subprocess.run(
        # The slot goes to stderr: a broken slot reads every result as -1000000.
        [os.path.join(HERE, "../oracle/oracle-slot"), "bash", "-c",
         'echo "oracle slot $ORACLE_SLOT" >&2; exec python3 dump.py "$@"', "-", name + ".jsonl", nki],
        cwd=HERE,
        env=env,
        check=False,
    )
    units = {}
    with open(name + ".jsonl") as f:
        for line in f:
            row = json.loads(line)
            if "g" in row:
                units[row["g"]] = round(row["u"])
    for k, label in enumerate(labels):
        digits = [units.get(5 * k + d) for d in range(5)]
        if any(d is None for d in digits):
            print(f"{label}\tmissing")
            continue
        print(f"{label}\t{sum(d * 100 ** i for i, d in enumerate(digits)) - OFFSET}")
    if texts:
        # save_array_str: a header line, then one element per line.
        saved = open(nka, encoding="latin-1").read().replace("\r", "").split("\n")[1:] if os.path.exists(nka) else []
        for k, label in enumerate(texts):
            print(f"{label}\t{saved[k] if k < len(saved) else 'missing'}")


if __name__ == "__main__":
    main()
