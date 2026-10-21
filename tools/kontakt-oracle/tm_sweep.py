#!/usr/bin/env python3
"""Time Machine source-parameter sweep: which stored source-record field does each ENGINE_PAR drive?

    TM_SOURCE_DUMP=<path to tm_source_dump> tm_sweep.py OUT.tsv [--modes time_machine_1,time_machine_2,time_machine_pro]
                        [--pars SPEED,SMOOTH,GRAIN_LENGTH,FORMANT_SHIFT,TMPRO_KEEP_FORMANTS]
                        [--values 0,250000,500000,750000,1000000]

One group per mode, one zone on a generated wav. For each (par, value) a one-line KSP `on init`
writes the engine parameter on that group, the instrument is saved, and the saved .nki is decoded
with the `tm_source_dump` example (ni_file's group source reader). Every source-record field that differs
from the untouched baseline is written as a TSV row. Runs one Kontakt session (see NATIVE_ORACLE.md).

Output: mode, par, value, path, field_idx, kind, baseline, stored
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
DECODER = Path(os.environ["TM_SOURCE_DUMP"])
WORK = Path("/tmp/w3/tm_sweep")
WIN_DIR = "Z:\\tmp\\w3\\tm_sweep\\"


def lua_s(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def gen_lua(modes: list[str], steps: list[tuple[str, str, int]]) -> str:
    """steps: (mode, par, value); par "" is the baseline for the mode."""
    rows = ",".join(f"{{{lua_s(m)},{lua_s(p)},{v}}}" for m, p, v in steps)
    return f"""
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local DIR = {lua_s(WIN_DIR)}
local STEPS = {{{rows}}}
local wav = DIR .. "tone.wav"
do
  local f = assert(io.open(wav, "wb"))
  local n = 48000
  local function u32(v) return string.pack("<I4", v) end
  f:write("RIFF", u32(36 + n * 4), "WAVEfmt ", u32(16), string.pack("<I2I2I4I4I2I2", 3, 1, 48000, 192000, 4, 32), "data", u32(n * 4))
  for i = 0, n - 1 do f:write(string.pack("<f", 0.5 * math.sin(i * 0.05))) end
  f:close()
end
for n, s in ipairs(STEPS) do
  -- a fresh instrument per step: no value written by an earlier step can leak into this record
  local mode, par, value = s[1], s[2], s[3]
  kt.reset_multi()
  local inst = kt.add_instrument()
  local zone = kt.add_zone(inst, 0, wav)
  kt.set_zone_geometry(inst, zone, {{ root_key = 60, low_key = 0, high_key = 127 }})
  kt.set_group_playback_mode(inst, 0, mode)
  local src = "on init\\n"
  if par ~= "" then
    src = src .. " set_engine_par($ENGINE_PAR_" .. par .. ", " .. value .. ", 0, -1, 0)\\n"
  end
  src = src .. "end on\\n"
  pcall(kt.set_instrument_script_source, inst, 0, src)
  pcall(kt.save_instrument, inst, DIR .. string.format("s_%d.nki", n), {{}})
  out:write(string.format('{{"step":%d,"mode":"%s","par":"%s","value":%d}}\\n', n, mode, par, value))
  out:flush()
end
out:write('{{"done":true}}\\n')
out:close()
"""


def fields(path: Path) -> dict:
    """(group, field name) -> (stored mode/version, stored value) for every group's source record."""
    res = {}
    txt = subprocess.run([str(DECODER), str(path)], capture_output=True, text=True).stdout
    for line in txt.splitlines():
        c = line.split("\t")
        if len(c) != 7:
            continue
        res[(c[1], c[4])] = (f"mode{c[2]} v{c[3]} @{c[5]}", c[6])
    return res


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--modes", default="time_machine_1,time_machine_2,time_machine_pro")
    ap.add_argument("--pars", default="SPEED,SMOOTH,GRAIN_LENGTH,FORMANT_SHIFT,TMPRO_KEEP_FORMANTS")
    ap.add_argument("--values", default="0,250000,500000,750000,1000000")
    a = ap.parse_args()
    modes = a.modes.split(",")
    pars = a.pars.split(",")
    values = [int(v) for v in a.values.split(",")]
    steps = []
    for m in modes:
        steps.append((m, "", 0))
        for p in pars:
            for v in values:
                steps.append((m, p, v))
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True)
    script = WORK.parent / "tm_sweep_gen.lua"
    script.write_text(gen_lua(modes, steps))
    env = dict(os.environ, ORACLE_LUA=str(script), ORACLE_TIMEOUT=str(600))
    subprocess.run([sys.executable, str(HERE / "dump.py"), str(WORK / "probe.jsonl")], env=env, check=True)
    rows = []
    base = {}
    for n, (m, p, v) in enumerate(steps, 1):
        f = WORK / f"s_{n}.nki"
        if not f.exists():
            continue
        got = fields(f)
        if p == "":
            base[m] = got
            continue
        for key, (kind, stored) in sorted(got.items()):
            old = base.get(m, {}).get(key)
            if old is None or old[1] != stored:
                rows.append([m, p, str(v), key[0], key[1], kind, old[1] if old else "", stored])
    Path(a.out).write_text("mode\tpar\tvalue\tgroup\tfield\tkind\tbaseline\tstored\n"
                           + "".join("\t".join(r) + "\n" for r in rows))
    print(f"{a.out}: {len(rows)} field changes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
