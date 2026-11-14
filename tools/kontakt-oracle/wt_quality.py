#!/usr/bin/env python3
"""What $ENGINE_PAR_WT_QUALITY does to a wavetable's sound, measured in real Kontakt.

    wt_quality.py probes OUT.lua             writes the probe (render it 30 s with render.py under oracle-slot)
    wt_quality.py analyze WAV [KONTRA.wav]   per case: harmonic gains and the alias floor

The table is one 2048-frame cycle of an additive sawtooth (harmonics 1..1023 at 1/h, so every partial the
table holds is known). Case (q, key) plays that table at `$NI_WT_QUALITY_*` q on `key` (60, 84, 96: the
table is read 11, 45 and 89 frames per output frame), one note per 2 s of the engine's uptime, 500 + 100 i ms
long, so a note's length says which case it is. Per note the analysis gives the level of harmonics 1, 2, 4,
8, 16 against the ideal 1/h (dB) and the energy that sits on no harmonic (aliasing, dB below the
fundamental).
"""
import struct
import sys
from pathlib import Path

import numpy as np

DIR = "Z:\\\\data\\\\dev\\\\workspace\\\\BuildScratch\\\\ksp-lane-wt\\\\"
KEYS = [60, 84, 96]
QUALITIES = ["LOFI", "MEDIUM", "HIGH", "BEST"]
CASES = [(q, k) for q in range(4) for k in KEYS]
SR = 48000

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
for i = 1, 2048 do
  local v = 0
  for h = 1, 1023 do v = v + math.sin(2 * math.pi * h * (i - 1) / 2048) / h end
  s[i] = v * 0.4
end
write_wav(DIR .. "saw2048.wav", s)
kt.reset_multi()
local insts = {}
local KEYS = {%s}
"""
CASE = """do
  local inst = kt.add_instrument()
  insts[%d] = inst
  kt.set_instrument_volume(inst, 0)
  local z = kt.add_zone(inst, 0, DIR .. "saw2048.wav")
  kt.set_zone_geometry(inst, z, { root_key = 60, low_key = 0, high_key = 127 })
  pcall(kt.set_group_playback_mode, inst, 0, "wavetable")
  local init = "on init\\n  set_engine_par($ENGINE_PAR_WT_QUALITY, $NI_WT_QUALITY_%s, 0, -1, -1)\\nend on\\n"
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
]], #insts, k - 1, KEYS[k], 500000 + 100000 * (k - 1))
  log('{"play":' .. tostring(pcall(kt.set_instrument_script_source, inst, 0, play)) .. '}')
end
log('{"ready":true}')
out:close()
"""


def probes(path):
    body = HEAD % (DIR, ",".join(str(k) for _, k in CASES))
    for i, (q, _) in enumerate(CASES):
        body += CASE % (i + 1, QUALITIES[q])
    Path(path).write_text(body + TAIL)


def read_wav(path):
    b = Path(path).read_bytes()
    i = b.find(b"data")
    n = struct.unpack("<I", b[i + 4:i + 8])[0]
    return np.frombuffer(b[i + 8:i + 8 + n], dtype="<f4").reshape(-1, 2).mean(1).astype(np.float64)


def notes(x):
    """(start, length) of each note, in seconds."""
    env = np.array([np.sqrt(np.mean(x[i:i + 240] ** 2)) for i in range(0, len(x) - 240, 240)])
    thr = env.max() * 0.02
    out, i = [], 0
    while i < len(env):
        if env[i] > thr:
            j = i
            while j < len(env) and env[j:j + 40].max() > thr:
                j += 1
            out.append((i * 240 / SR, (j - i) * 240 / SR))
            i = j
        else:
            i += 1
    return out


def measure(x, t, key):
    n = 4096
    s = x[int((t + 0.15) * SR):int((t + 0.15) * SR) + n] * np.hanning(n + 1)[:n]
    p = np.abs(np.fft.rfft(s)) ** 2
    hz = np.fft.rfftfreq(n, 1 / SR)
    f0 = 261.6256 * 2 ** ((key - 60) / 12)
    at = lambda f: int(round(f * n / SR))
    band = lambda h: range(max(at(h * f0) - 2, 0), at(h * f0) + 3)
    h1 = sum(p[b] for b in band(1))
    on = np.zeros(len(p), bool)
    gains = {}
    for h in range(1, int(20000 / f0) + 1):
        ph = sum(p[b] for b in band(h))
        for b in band(h):
            on[b] = True
        if h in (2, 4, 8, 16, 32):
            gains[h] = 10 * np.log10(ph / h1 * h * h + 1e-20)
    off = p[(hz < 20000) & ~on & (np.arange(len(p)) >= 5)].sum()
    return f0, gains, 10 * np.log10(off / h1 + 1e-20)


def analyze(wav, kontra=None):
    for name, path in (("Kontakt", wav), ("KONTRA", kontra)):
        if not path:
            continue
        x = read_wav(path)
        got = notes(x)
        # k = 0 sits inside the merged long note before k = 1, which is the first note after it.
        t1 = next(got[i + 1][0] for i in range(len(got) - 1) if got[i][1] > 2.5)
        print(f"{name}: cycle starts {t1:.2f}")
        for k, (q, key) in enumerate(CASES):
            f0, g, a = measure(x, t1 + 2 * (k - 1), key)
            print(f"  {QUALITIES[q]:6} key {key} ({f0:7.1f} Hz): " + " ".join(f"h{h} {v:+.1f}" for h, v in g.items()) + f" | alias {a:+.1f} dB")


if __name__ == "__main__":
    {"probes": lambda: probes(sys.argv[2]), "analyze": lambda: analyze(*sys.argv[2:4])}[sys.argv[1]]()
