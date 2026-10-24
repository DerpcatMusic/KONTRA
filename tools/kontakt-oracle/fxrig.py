#!/usr/bin/env python3
"""FX response rig: one Kontakt effect, one setting per instrument, set by the instrument's own init.

    fxrig.py SPEC.json OUT.wav        one session, one instrument per setting, one note each

SPEC: {"chain": ["Filters/Solid G-EQ"], "instances": [{"name": "...", "sets": [[slot, "SEQ_LMF_GAIN", 1000000], ...]}]}

Why init and not CC20: eqrender's on-controller path only sets parameters when a CC arrives; the
instrument's `on init` runs as soon as set_instrument_script_source returns (probes/pilot_fx.lua), so
the setting is in place before any note. Every instance has its own zone (impulse WAV) and its own
MIDI channel, so one session renders all instances with one note each, spaced SLOT seconds apart.
"""
import json
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import eqrender  # noqa: E402
import wavutil  # noqa: E402
sys.path.insert(0, str(HERE.parent / "oracle"))
import slot  # noqa: E402  (oracle-slot: this session's MIDI channel block; legacy = channels 1..16)

BASE, CHANNELS = slot.channel_block()  # instance k plays on MIDI channel BASE + k + 1 (1-based in Lua)

SLOT = 2.0       # seconds per instance in the MIDI timeline
FIRST = 2.0      # first note


def ksp_for(chain_len: int, sets) -> str:
    lines = ["on init"]
    for slot, par, val in sets:
        lines.append(f"  set_engine_par($ENGINE_PAR_{par}, {int(val)}, -1, {slot}, 0)")
    lines.append("end on")
    return "\\n".join(lines)


LEVEL_STEPS_DB = [-40, -30, -24, -18, -12, -6, 0]   # tone levels of the "levels" stimulus (1 kHz sine)
LEVEL_SEG = 1.0                                        # seconds per level: 0.5 s tone, 0.5 s silence
LEVEL_LEAD = 0.5                                       # silence before the first level


def levels_lua(path: Path) -> str:
    """Lua that writes the level-stepped tone WAV (stereo, identical channels) inside Kontakt."""
    n_seg = int(LEVEL_SEG * 48000)
    frames = int(LEVEL_LEAD * 48000) + len(LEVEL_STEPS_DB) * n_seg
    levels = ",".join(repr(10 ** (db / 20)) for db in LEVEL_STEPS_DB)
    return f"""local LF = io.open({json.dumps(str(path))}, "wb")
local NF, SEG, LEAD = {frames}, {n_seg}, {int(LEVEL_LEAD * 48000)}
local AMP = {{{levels}}}
LF:write("RIFF", string.pack("<I4", 36 + NF * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", NF * 8))
for i = 0, NF - 1 do
  local j = (i - LEAD) // SEG
  local v = 0.0
  if i >= LEAD and j < #AMP and (i - LEAD) % SEG < SEG // 2 then v = AMP[j + 1] * math.sin(2 * math.pi * 1000 * i / 48000) end
  LF:write(string.pack("<ff", v, v))
end
LF:close()
"""


def make_lua(spec, work: Path) -> str:
    """Lua for the whole session: each instance gets its own zone and init script."""
    wav = work / "impulse_lr.wav"  # written by IMPULSE_LUA inside Kontakt

    chain_paths = [eqrender.winpath(eqrender.init_preset(c)) for c in spec["chain"]]
    body = []
    for k, inst in enumerate(spec["instances"]):
        ksp = ksp_for(len(spec["chain"]), inst["sets"])
        body.append(f"""
do
  local i = kt.add_instrument()
  kt.set_instrument_midi_channel(i, {k + 1 + BASE})
  local z = kt.add_zone(i, 0, WAV)
  kt.set_zone_geometry(i, z, {{ root_key = 60, low_key = 0, high_key = 127 }})
  for slot, p in ipairs(CHAIN) do log('{{"load":' .. tostring(pcall(kt.load_fx_preset, p, i, -1, slot - 1, kt.insert_fx)) .. ',"inst":{k}}}') end
  log('{{"script":' .. tostring(pcall(kt.set_instrument_script_source, i, 0, "{ksp}")) .. ',"inst":{k}}}')
end""")
    chain = ",".join(json.dumps(p) for p in chain_paths)
    return (
        "local kt = Kontakt\n"
        "local out = assert(io.open(ORACLE_OUT, \"w\"))\n"
        "local function log(t) out:write(t .. \"\\n\") out:flush() end\n"
        f"local WAV = {json.dumps(eqrender.winpath(work / ('levels_lr.wav' if spec.get('stimulus') == 'levels' else 'impulse_lr.wav')))}\n"
        f"local CHAIN = {{{chain}}}\n"
        + (levels_lua(work / "levels_lr.wav") if spec.get("stimulus") == "levels" else
           eqrender.IMPULSE_LUA.replace("@WAV@", json.dumps(eqrender.winpath(wav))))
        + "kt.reset_multi()\n"
        + "".join(body)
        + "\nlog('{\"ready\":true}')\nout:close()\n"
    )


PROBES = [30, 60, 100, 200, 500, 1000, 2000, 5000, 10000, 15000]
CHUNK = 15      # MIDI channels 2..16 per session (one reference instance takes channel 1)


def render_chunk(chain, sets_list, out_wav: Path, stimulus=None):
    """One session: instance 0 is the bypassed reference, then one instance per settings list."""
    work = eqrender.scratch_dir()
    spec = {"chain": chain, "stimulus": stimulus, "instances": [{"name": "ref", "sets": [[0, "EFFECT_BYPASS", 1]]}]
            + [{"name": f"s{i}", "sets": [[0, "EFFECT_BYPASS", 0]] + sets} for i, sets in enumerate(sets_list)]}
    mid, lua = work / "rig.mid", work / "fxrig_gen.lua"
    write_midi(len(spec["instances"]), mid)
    lua.write_text(make_lua(spec, work))
    total = FIRST + len(spec["instances"]) * SLOT + 2.0
    subprocess.run([sys.executable, str(HERE / "render.py"), str(lua), str(out_wav), str(total), str(mid)], check=True)
    return spec


def impulse_fft(x, rate, onset):
    """Magnitude (dB) of the impulse response that starts at the first sample above threshold after onset."""
    m = np.abs(x)
    lo = int(onset * rate)
    k = lo + int(np.argmax(m[lo:lo + int(SLOT * rate) - 4800]))
    seg = np.zeros(16384)
    part = x[k - 16:k - 16 + 16384]
    seg[:len(part)] = part
    return 20 * np.log10(np.maximum(np.abs(np.fft.rfft(seg)), 1e-9)), k


def level_curve(chain, pre, steps, out_prefix: Path):
    """Static curve: the level-stepped tone through each setting; per level the output dB and the change against the
    bypassed reference (gain reduction is negative). Writes out_prefix.tsv: step, name, level, ref dB, out dB, delta dB."""
    global SLOT
    SLOT = LEVEL_LEAD + len(LEVEL_STEPS_DB) * LEVEL_SEG + 1.0
    rows = []
    for c0 in range(0, len(steps), CHUNK):
        part = steps[c0:c0 + CHUNK]
        wav = out_prefix.with_name(f"{out_prefix.name}_c{c0 // CHUNK}.wav")
        render_chunk(chain, [[*pre, *st["set"]] for st in part], wav, stimulus="levels")
        ch, rate, (L, R) = wavutil.read(wav)
        x = np.array(L)
        for i, st in enumerate(part, start=1):
            seg = segment_db(x, rate, FIRST + i * SLOT, LEVEL_SEG)
            ref = segment_db(x, rate, FIRST, LEVEL_SEG) if i == 1 else ref_levels
            if i == 1:
                ref_levels = ref
            for j, db in enumerate(LEVEL_STEPS_DB):
                rows.append([c0 + i, st["name"], db, f"{ref[j]:.2f}", f"{seg[j]:.2f}", f"{seg[j] - ref[j]:.2f}"])
    tsv = out_prefix.with_suffix(".tsv")
    tsv.write_text("\n".join("\t".join(map(str, r)) for r in rows) + "\n")
    return tsv


def segment_db(x, rate, onset, seg_s):
    """RMS dB of each level segment, measured over the steady half of its tone (after the first half)."""
    m = np.abs(x)
    lo = int(onset * rate)
    first = lo + int(np.argmax(m[lo:lo + int(SLOT * rate)] > 1e-4)) - int(LEVEL_LEAD * rate)  # file start
    out = []
    for j in range(len(LEVEL_STEPS_DB)):
        a = first + int((LEVEL_LEAD + j * LEVEL_SEG + 0.25) * rate)
        b = a + int(0.25 * rate)
        out.append(20 * np.log10(np.sqrt(np.mean(x[a:b] ** 2)) + 1e-12))
    return out


def sweep(chain, pre, steps, out_prefix: Path):
    """steps: [{"name", "set": [[slot, PAR, value], ...]}]; pre: sets applied before every step.
    Writes out_prefix.tsv: step, name, dB at PROBES (active minus bypassed reference)."""
    rows, freqs = [], np.fft.rfftfreq(16384, 1 / 48000)  # noqa
    for c0 in range(0, len(steps), CHUNK):
        part = steps[c0:c0 + CHUNK]
        wav = out_prefix.with_name(f"{out_prefix.name}_c{c0 // CHUNK}.wav")
        render_chunk(chain, [[*pre, *st["set"]] for st in part], wav)
        ch, rate, (L, R) = wavutil.read(wav)
        L = np.array(L)
        ref, _ = impulse_fft(L, rate, FIRST)  # instance 0: the bypassed reference
        for i, st in enumerate(part, start=1):
            db, _ = impulse_fft(L, rate, FIRST + i * SLOT)
            h = db - ref
            rows.append([c0 + i, st["name"]] + [f"{np.interp(p, freqs, h):.2f}" for p in PROBES])
    tsv = out_prefix.with_suffix(".tsv")
    tsv.write_text("\n".join("\t".join(map(str, r)) for r in rows) + "\n")
    return tsv


def main() -> int:
    if sys.argv[1] == "levels":
        spec = json.load(open(sys.argv[2]))
        print(level_curve(spec["chain"], spec.get("pre", []), spec["steps"], Path(sys.argv[3]).resolve()))
        return 0
    if sys.argv[1] == "sweep":
        spec = json.load(open(sys.argv[2]))
        print(sweep(spec["chain"], spec.get("pre", []), spec["steps"], Path(sys.argv[3]).resolve()))
        return 0
    spec = json.load(open(sys.argv[1]))
    out_wav = Path(sys.argv[2]).resolve()
    work = eqrender.scratch_dir()
    mid = work / "rig.mid"
    n = len(spec["instances"])
    if n > CHANNELS:
        sys.exit(f"fxrig: {n} instances, this session has {CHANNELS} MIDI channels; split the spec")
    write_midi(n, mid)
    lua = work / "fxrig_gen.lua"
    lua.write_text(make_lua(spec, work))
    total = FIRST + n * SLOT + 2.0
    subprocess.run([sys.executable, str(HERE / "render.py"), str(lua), str(out_wav), str(total), str(mid)], check=True)
    print(out_wav)
    return 0


def write_midi(n: int, path: Path):
    """One note per instance on its own channel, SLOT seconds apart (PPQ 480, 120 bpm)."""
    ev = []
    for k in range(n):
        t = FIRST + k * SLOT
        ev += [(t, bytes([0x90 | (BASE + k), 60, 100])), (t + 0.1, bytes([0x80 | (BASE + k), 60, 0]))]
    trk, last = b"\x00\xff\x51\x03" + (500000).to_bytes(3, "big"), 0
    for t, m in sorted(ev, key=lambda e: e[0]):
        tick = round(t * 960)
        trk += eqrender.vlq(tick - last) + m
        last = tick
    trk += b"\x00\xff\x2f\x00"
    head = b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1]) + (480).to_bytes(2, "big")
    path.write_bytes(head + b"MTrk" + len(trk).to_bytes(4, "big") + trk)


if __name__ == "__main__":
    sys.exit(main())
