#!/usr/bin/env python3
"""The wavetable mod oscillator's waves and tune units, measured in real Kontakt.

    wt_mod_waves.py probes OUT_DIR       writes waves.lua, units.lua
    (run each with tools/oracle/oracle-slot python3 tools/kontakt-oracle/render.py PROBE.lua OUT.wav SECONDS;
     waves.lua 42 s, units.lua 48 s)
    wt_mod_waves.py extract WAVES.wav OUT.rs   writes crates/engine/voice/src/mod_waves.rs

A probe is nine (waves) or ten (units) wavetable groups on a silent 2048-frame table, mod type MIX at 100 %, so the
output is the modulator alone; instrument k plays one note every 2 s of the engine's uptime, 300 + 100·k ms long, so
a note's length says which case it is. Waves: `$NI_WT_MOD_WAVE_*` 0..8 at key 24 (a long period). Units: tune raw
values in each `$NI_WT_MOD_TUNE_UNIT_*`; measured 2026-11-14 on Kontakt 8.13.1:
  semitones: -36 + 72x st (raw 0 / 250000 / 500000 / 750000 / 1e6 -> 32.7 / 92.5 / 261.6 / 740 / 2093 Hz at key 60);
  ratio: n = round(1 + 15 |2x - 1|) as ratio n above 0.5, 1/n below (510000 -> 1, 520000 -> 2, 540000 -> 2,
         600000 -> 4, 900000 -> 13; 480000 -> 1/2, 400000 -> 1/4, 250000 -> 1/9, 0 -> 1/16, 100000 -> 1/13);
  Hz: 0.5 * 20000^x Hz whatever the note (5.9 / 70.7 / 840.9 / 10000 Hz at 250000 / 500000 / 750000 / 1e6, 70.7 at key 72).
"""
import json
import struct
import sys
from pathlib import Path

import numpy as np

N = 512
DIR = "Z:\\\\data\\\\dev\\\\workspace\\\\BuildScratch\\\\ksp-lane-wt\\\\"  # a scratch dir both sides can see
HEAD = """local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\\n") out:flush() end
local DIR = "%s"
local function write_wav(path, samples)
  local f = assert(io.open(path, "wb"))
  local n = #samples
  f:write("RIFF", string.pack("<I4", 36 + n * 4), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 1, 48000, 192000, 4, 32), "data", string.pack("<I4", n * 4))
  for i = 1, n do f:write(string.pack("<f", samples[i])) end
  f:close()
end
local s = {}
for i = 1, 2048 do s[i] = 0 end
write_wav(DIR .. "zero2048.wav", s)
kt.reset_multi()
local insts = {}
local KEYS = {%s}
"""
CASE = """do
  local inst = kt.add_instrument()
  insts[%d] = inst
  kt.set_instrument_volume(inst, 0)
  local z = kt.add_zone(inst, 0, DIR .. "zero2048.wav")
  kt.set_zone_geometry(inst, z, { root_key = 60, low_key = 0, high_key = 127 })
  pcall(kt.set_group_playback_mode, inst, 0, "wavetable")
  local init = "on init\\n  set_engine_par($ENGINE_PAR_WT_MOD_TYPE, $NI_WT_MOD_TYPE_MIX, 0, -1, -1)\\n  set_engine_par($ENGINE_PAR_WT_MOD_WAVE, %d, 0, -1, -1)\\n  set_engine_par($ENGINE_PAR_WT_MOD_TUNE_UNIT, %d, 0, -1, -1)\\n  set_engine_par($ENGINE_PAR_WT_MOD_TUNE, %d, 0, -1, -1)\\n  set_engine_par($ENGINE_PAR_WT_MOD_AMOUNT, 1000000, 0, -1, -1)\\nend on\\n"
  log('{"init":' .. tostring(pcall(kt.set_instrument_script_source, inst, 0, init)) .. '}')
end
"""
TAIL = """local t0 = os.time() repeat until os.time() - t0 >= 3
for k, inst in ipairs(insts) do
  local play = string.format([[
on init
  declare $last := -1
  set_listener($NI_SIGNAL_TIMER_MS, 100000)
end on
on listener
  if (($ENGINE_UPTIME / 2000) mod %d = %d and ($ENGINE_UPTIME / 2000) # $last)
    $last := $ENGINE_UPTIME / 2000
    play_note(%d, 127, 0, %d)
  end if
end on
]], #insts, k - 1, KEYS[k], 300000 + 100000 * (k - 1))
  log('{"play":' .. tostring(pcall(kt.set_instrument_script_source, inst, 0, play)) .. '}')
end
log('{"ready":true}')
out:close()
"""


def lua(cases, keys):
    body = HEAD % (DIR, ",".join(map(str, keys)))
    for k, (wave, unit, raw) in enumerate(cases):
        body += CASE % (k + 1, wave, unit, raw)
    return body + TAIL


def probes(out):
    out = Path(out)
    (out / "waves.lua").write_text(lua([(w, 0, 500000) for w in range(9)], [24] * 9))
    ratios = [(0, 1, r) for r in (505000, 510000, 520000, 540000, 600000, 900000, 480000, 400000, 100000)] + [(0, 2, 500000)]
    (out / "units.lua").write_text(lua(ratios, [60] * 9 + [72]))


def extract(wav, rs):
    b = Path(wav).read_bytes()
    i = b.find(b"data")
    n = struct.unpack("<I", b[i + 4:i + 8])[0]
    x = np.frombuffer(b[i + 8:i + 8 + n], dtype="<f4").reshape(-1, 2).mean(1).astype(np.float64)
    sr, period = 48000, 48000 / (440 * 2 ** ((24 - 69) / 12))
    # Notes are 2 s apart; find the first note of the last full cycle by its length: 9 notes, 300 + 100k ms.
    env = np.array([np.sqrt(np.mean(x[i:i + 240] ** 2)) for i in range(0, len(x) - 240, 240)])
    thr = env.max() * 0.02
    starts, i = [], 0
    while i < len(env):
        if env[i] > thr:
            j = i
            while j < len(env) and env[j:j + 40].max() > thr:
                j += 1
            starts.append((i * 240 / sr, (j - i) * 240 / sr))
            i = j
        else:
            i += 1
    # A note's length is 0.26 s + 0.1 k: the first full cycle starts at the first one near 0.56 s. Case 1 begins
    # with a short fragment (the recording started a block into its note), so it is the entry after that.
    c = next(s for s in range(len(starts)) if abs(starts[s][1] - 0.56) < 0.04)
    at = [starts[c][0], starts[c + 2][0]] + [starts[c + 3 + k][0] for k in range(7)]
    raw = []
    for t in at:
        s = x[int((t + 0.08) * sr):int((t + 0.33) * sr)]
        raw.append(np.array([np.interp(m * period / N, np.arange(len(s)), s) for m in range(N)]))
    peak = np.abs(raw[0]).max()
    waves = []
    for p in raw:
        v = p / peak
        z = np.abs(v) <= 0.004
        run = lambda i: next(n for n in range(N + 1) if n == N or not z[(i - 1 - n) % N])
        cands = [(run(i), i) for i in range(N) if z[i] and v[(i + 3) % N] > v[(i - 3) % N] and abs(v[(i + 6) % N]) > 0.004]
        waves.append(np.roll(v, -sorted(cands, reverse=True)[0][1]))
    lines = ["//! The wavetable mod oscillator's waves 1..=8 (`$NI_WT_MOD_WAVE_TRIANGLE`, `TX2`..`TX8`) as Kontakt 8.13.1",
             "//! plays them: one period in 512 steps of 1/32767 of the sine's peak, from the rising zero (after the",
             "//! longest zero run). Measured by `tools/kontakt-oracle/wt_mod_waves.py` (not generated by hand).", "",
             "pub const STEPS: usize = %d;" % N, "", "pub const WAVES: [[i16; STEPS]; 8] = ["]
    for w in waves[1:]:
        lines.append("    [")
        vals = [int(round(v * 32767)) for v in w]
        for i in range(0, N, 16):
            lines.append("        " + ", ".join(str(v) for v in vals[i:i + 16]) + ",")
        lines.append("    ],")
    lines.append("];")
    Path(rs).write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    {"probes": lambda a: probes(a[0]), "extract": lambda a: extract(a[0], a[1])}[sys.argv[1]](sys.argv[2:])
