#!/usr/bin/env python3
"""Effect lab: render one Kontakt effect at several settings and keep the raw audio for fitting a model.

    fx_lab.py render CHAIN STIMULUS OUT.npz  SETS.json     (run under tools/oracle/oracle-slot)

CHAIN is a factory preset folder ("Effects/RDelay"), STIMULUS one of `impulse`, `tone` (5 s, 1 kHz, -12 dB),
`levels`, `burst` (fxrig's stimuli), `ramp` (1 kHz, amplitude 0..1 over 5 s), `clicks`, `sweep`, or one of STIMULI (`long`: 12 s
of 1 kHz at -12 dB; `ramplo`: the ramp to 0.02; `sweeplo`: the sweep at -60 dB; `steps`/`stepslo`: 0.5 s tones at STEPS, -20.6 / -60 dB at the effect) (a 5 s log sweep 20 Hz..20 kHz, -12 dB), SETS.json a list of
settings, each a list of [slot, "PAR", raw] (raw 0..1000000 for knobs). Instance 0 is the bypassed reference.
OUT.npz holds `rate` and one float array per instance (`x0` = reference, `x1`...), each cut from its note's start to
the slot's end, left channel and right (`y1`...).

Models are fitted from these arrays by the effect's own script (fx_rdl.py ...). Delete the .npz when the fit is done.
"""
import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import fxrig  # noqa: E402
import wavutil  # noqa: E402

SWEEP_LEN = 5.0
# Effect knobs take hold when set from a note callback after the instrument has loaded (a note 100 the script
# ignores, 0.7 s before the stimulus note); the bypass switch is set in `on init`. Set in `on init` alone, Replika's
# TIME and FEEDBACK did not take (Kontakt 8.13.1).
PRE_NOTE = 0.7
SWEEP_DB = -12.0


def sweep_lua(path: Path) -> str:
    n = int(SWEEP_LEN * 48000)
    amp = 10 ** (SWEEP_DB / 20)
    return f"""local LF = io.open({json.dumps(str(path))}, "wb")
local NF = {n}
LF:write("RIFF", string.pack("<I4", 36 + NF * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", NF * 8))
local f0, f1 = 20.0, 20000.0
local k = math.log(f1 / f0) / {SWEEP_LEN}
for i = 0, NF - 1 do
  local t = i / 48000
  local v = {amp!r} * math.sin(2 * math.pi * f0 * (math.exp(k * t) - 1) / k)
  LF:write(string.pack("<ff", v, v))
end
LF:close()
"""


def clicks_lua(path: Path) -> str:
    """A click every 25 ms for 5 s (-6 dB): each echo's arrival time is the delay at that moment."""
    n = int(SWEEP_LEN * 48000)
    return f"""local LF = io.open({json.dumps(str(path))}, "wb")
local NF = {n}
LF:write("RIFF", string.pack("<I4", 36 + NF * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", NF * 8))
for i = 0, NF - 1 do
  local v = 0.0
  if i % 1200 == 0 then v = 0.5 end
  LF:write(string.pack("<ff", v, v))
end
LF:close()
"""


def ramp_lua(path: Path) -> str:
    """A 1 kHz sine whose amplitude rises linearly 0 -> 2.7 over 5 s (the instrument brings 1.0 to about 0.37 at the effect: the peak of each cycle is the input level)."""
    n = int(SWEEP_LEN * 48000)
    return f"""local LF = io.open({json.dumps(str(path))}, "wb")
local NF = {n}
LF:write("RIFF", string.pack("<I4", 36 + NF * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", NF * 8))
for i = 0, NF - 1 do
  local v = (i / NF) * 2.7 * math.sin(2 * math.pi * 1000 * i / 48000)
  LF:write(string.pack("<ff", v, v))
end
LF:close()
"""


def stim_lua(seconds, expr):
    """Lua writing a stereo float WAV of `seconds` whose sample i is the Lua expression `expr` (t = i / 48000)."""
    n = int(seconds * 48000)
    return lambda path: f"""local LF = io.open({json.dumps(str(path))}, "wb")
local NF = {n}
LF:write("RIFF", string.pack("<I4", 36 + NF * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", NF * 8))
local k = math.log(1000.0) / {SWEEP_LEN}
local STEPS = {{{", ".join(str(f) for f in STEPS)}}}
for i = 0, NF - 1 do
  local t = i / 48000
  local v = {expr}
  LF:write(string.pack("<ff", v, v))
end
LF:close()
"""


# More stimuli, each (seconds, Lua sample expression). The instrument brings a WAV peak of 1.0 to about 0.37 at the
# effect (-8.6 dB), so `ramp` reaches about 1.0 there and `ramplo` about 0.02.
STIMULI = {
    "long": (12.0, "0.25 * math.sin(2 * math.pi * 1000 * t)"),
    "ramplo": (SWEEP_LEN, f"(t / {SWEEP_LEN}) * 0.054 * math.sin(2 * math.pi * 1000 * t)"),
    "sweeplo": (SWEEP_LEN, "0.001 * math.sin(2 * math.pi * 20 * (math.exp(k * t) - 1) / k)"),
    # Steady tones, 0.5 s each at the STEPS frequencies (a steady-state response, against a sweep's).
    "steps": (6.5, "0.25 * math.sin(2 * math.pi * STEPS[math.floor(t / 0.5) + 1] * t)"),
    "stepslo": (6.5, "0.0025 * math.sin(2 * math.pi * STEPS[math.floor(t / 0.5) + 1] * t)"),
    # Two non-harmonic tones together (4 s: images and intermodulation fall between their harmonics), then the
    # same 500 Hz..16 kHz log sweep fast (0.5 s, from 4.5 s) and slow (12 s, from 5.5 s), all -20.6 dB at the effect.
    "pairsweeps": (18.0, "(t < 4 and 0.125 * (math.sin(2 * math.pi * 1000 * t) + math.sin(2 * math.pi * 7300 * t)))"
                         " or (t >= 4.5 and t < 5 and 0.25 * math.sin(2 * math.pi * 500 * (math.exp(6.931472 * (t - 4.5)) - 1)"
                         " / 6.931472)) or (t >= 5.5 and t < 17.5 and 0.25 * math.sin(2 * math.pi * 500"
                         " * (math.exp(0.288811 * (t - 5.5)) - 1) / 0.288811)) or 0.0"),
}
STEPS = [63, 125, 250, 500, 1000, 2000, 4000, 6000, 8000, 10000, 12000, 14000, 16000]


def ksp_for(chain_len, sets):
    call = lambda p, v, sl: f"  set_engine_par($ENGINE_PAR_{p}, {int(v)}, -1, {sl}, 1)"
    init = [call(p, v, sl) for sl, p, v in sets if p == "EFFECT_BYPASS"]
    late = [call(p, v, sl) for sl, p, v in sets if p != "EFFECT_BYPASS"]
    return "\\n".join(["on init"] + init + ["end on", "on note", "  if ($EVENT_NOTE = 100)", "    ignore_event($EVENT_ID)"]
                       + ["  " + l for l in late] + ["  end if", "end on"])


def write_midi(n, path, hold=0.1):
    ev = []
    for k in range(n):
        t = fxrig.FIRST + k * fxrig.SLOT
        c = fxrig.BASE + k
        ev += [(t, bytes([0x90 | c, 100, 100])), (t + 0.05, bytes([0x80 | c, 100, 0])),
               (t + PRE_NOTE, bytes([0x90 | c, 60, 100])), (t + PRE_NOTE + hold, bytes([0x80 | c, 60, 0]))]
    trk, last = b"\x00\xff\x51\x03" + (500000).to_bytes(3, "big"), 0
    for t, m in sorted(ev, key=lambda e: e[0]):
        tick = round(t * 960)
        trk += fxrig.eqrender.vlq(tick - last) + m
        last = tick
    trk += b"\x00\xff\x2f\x00"
    head = b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1]) + (480).to_bytes(2, "big")
    path.write_bytes(head + b"MTrk" + len(trk).to_bytes(4, "big") + trk)


def render(chain, stimulus, sets_list, out):
    """Instances 1.. play `sets_list`; returns (rate, [(left, right) per instance incl. the reference])."""
    if stimulus == "tone":
        fxrig.SLOT = fxrig.STEADY_TONE_SLOT
    elif stimulus == "sweep":  # the tone stimulus's WAV, written as a sweep
        fxrig.SLOT = fxrig.STEADY_TONE_SLOT
        fxrig.steady_tone_lua = sweep_lua
        stimulus = "tone"
    elif stimulus == "clicks":  # likewise
        fxrig.SLOT = fxrig.STEADY_TONE_SLOT
        fxrig.steady_tone_lua = clicks_lua
        stimulus = "tone"
    elif stimulus == "ramp":  # likewise
        fxrig.SLOT = fxrig.STEADY_TONE_SLOT
        fxrig.steady_tone_lua = ramp_lua
        stimulus = "tone"
    elif stimulus in STIMULI:
        seconds, expr = STIMULI[stimulus]
        fxrig.SLOT = seconds + 2.0
        fxrig.steady_tone_lua = stim_lua(seconds, expr)
        stimulus = "tone"
    elif stimulus == "levels":
        fxrig.SLOT = fxrig.LEVEL_LEAD + len(fxrig.LEVEL_STEPS_DB) * fxrig.LEVEL_SEG + 1.0
    elif stimulus == "burst":
        fxrig.SLOT = fxrig.BURST_LEAD + fxrig.BURST_ON + fxrig.BURST_OFF + 1.0
    if stimulus == "impulse":
        fxrig.SLOT = 4.0
    fxrig.ksp_for = ksp_for
    fxrig.write_midi = write_midi
    fxrig.SLOT += PRE_NOTE
    # An oracle slot owns a block of MIDI channels (16 // slots); instances beyond it came back silent or as copies.
    fxrig.CHUNK = min(fxrig.CHUNK, fxrig.CHANNELS - 1)
    results = []
    for c0 in range(0, len(sets_list), fxrig.CHUNK):
        part = sets_list[c0:c0 + fxrig.CHUNK]
        wav = Path(str(out) + f".c{c0 // fxrig.CHUNK}.wav")
        fxrig.render_chunk(chain, part, wav, stimulus=None if stimulus == "impulse" else stimulus)
        ch, rate, (left, right) = wavutil.read(wav)
        left, right = np.array(left), np.array(right)
        t0 = int(np.argmax(np.abs(left) > 1e-4))
        origin = t0 / rate - fxrig.FIRST - PRE_NOTE
        for k in range(len(part) + 1):
            a = int((origin + fxrig.FIRST + k * fxrig.SLOT) * rate)
            b = a + int(fxrig.SLOT * rate)
            if c0 and k == 0:
                continue
            results.append((left[a:b], right[a:b]))
        wav.unlink()
    return rate, results


if __name__ == "__main__":
    if sys.argv[1] != "render":
        sys.exit(__doc__)
    chain, stimulus, out, sets_json = sys.argv[2:6]
    sets = json.loads(Path(sets_json).read_text())
    rate, res = render([chain], stimulus, sets, Path(out))
    np.savez(out, rate=rate, **{f"x{i}": l for i, (l, r) in enumerate(res)}, **{f"y{i}": r for i, (l, r) in enumerate(res)})
