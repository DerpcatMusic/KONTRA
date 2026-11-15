#!/usr/bin/env python3
"""A script-loaded effect's fresh state, read back from Kontakt (no save needed).

    fresh_probe.py lua PARS.txt OUT.nka TYPE [TYPE ...] > probe.lua
    tools/oracle/oracle-slot run tools/kontakt-oracle/render.py probe.lua /dev/null 8
    fresh_probe.py read PARS.txt OUT.nka TYPE [TYPE ...]

PARS.txt holds $ENGINE_PAR_ names without the prefix, one per line (the effect
type's own, plus EFFECT_TYPE, INSERT_EFFECT_OUTPUT_GAIN, SEND_EFFECT_DRY_LEVEL).
TYPE is an $EFFECT_TYPE_ name without the prefix (IRC, FREAK, ...), slot n in
argument order. The KSP loads each type from a 200 ms timer listener (tick 2: a
type set in `on init` does not take, and a render session is needed: with no
audio the listener never fires), then a block later only READS every
get_engine_par of each slot from `on pgs_changed` and saves them with
save_array_str. `read` prints, per type, the pars that are not 0 (raw values,
x1e6 for 0..1; 396850 is 0 dB).
"""
import sys
from pathlib import Path


def lua(pars, out, types):
    win = "Z:" + str(Path(out).resolve())
    reads = "".join(
        f"  %vals[$j] := get_engine_par($ENGINE_PAR_{p}, -1, {s}, 1)\n  inc($j)\n"
        for s in range(len(types)) for p in pars)
    loads = "".join(f"    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_{t}, -1, {s}, 1)\n"
                    for s, t in enumerate(types))
    ksp = (f"on init\n  pgs_create_key(READ, 1)\n  declare $tick := 0\n  declare %vals[{len(pars) * len(types)}]\n"
           "  declare $j\n  set_listener($NI_SIGNAL_TIMER_MS, 200000)\nend on\n"
           f"on listener\n  inc($tick)\n  if ($tick = 2)\n{loads}  end if\n"
           "  if ($tick = 12)\n    pgs_set_key_val(READ, 0, 1)\n  end if\nend on\n"
           f'on pgs_changed\n  $j := 0\n{reads}  save_array_str(%vals, "{win}")\nend on\n')
    return ("local kt = Kontakt\nlocal out = assert(io.open(ORACLE_OUT, \"w\"))\nkt.reset_multi()\n"
            "local i = kt.add_instrument()\n"
            f"local ok, err = pcall(kt.set_instrument_script_source, i, 0, [==[{ksp}]==])\n"
            "out:write(string.format('{\"compiled\":%s,\"err\":\"%s\"}\\n', tostring(ok), tostring(err):gsub('\"', \"'\")))\n"
            "out:write('{\"ready\":true}\\n')\nout:close()\n")


def main():
    if len(sys.argv) < 5 or sys.argv[1] not in ("lua", "read"):
        sys.exit(__doc__)
    mode, pars, out, types = sys.argv[1], Path(sys.argv[2]).read_text().split(), sys.argv[3], sys.argv[4:]
    if mode == "lua":
        print(lua(pars, out, types), end="")
        return
    vals = Path(out).read_text().split()[1:]
    for s, t in enumerate(types):
        row = vals[s * len(pars):(s + 1) * len(pars)]
        print(t, " ".join(f"{p}={v}" for p, v in zip(pars, row) if v != "0"))


if __name__ == "__main__":
    main()
