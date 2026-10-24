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


BURST_LEAD = 0.5                                       # silence before the burst
BURST_ON = 2.5                                         # tone on (1 kHz, BURST_DB)
BURST_OFF = 2.5                                        # quiet tone after the burst (below the threshold: the gain recovers)
BURST_DB = -12.0
BURST_QUIET_DB = -50.0


def burst_lua(path: Path) -> str:
    """Lua that writes one tone-burst WAV (stereo, identical channels) inside Kontakt: lead, loud tone, quiet tone."""
    frames = int((BURST_LEAD + BURST_ON + BURST_OFF) * 48000)
    lead, on = int(BURST_LEAD * 48000), int(BURST_ON * 48000)
    amp, quiet = 10 ** (BURST_DB / 20), 10 ** (BURST_QUIET_DB / 20)
    return f"""local LF = io.open({json.dumps(str(path))}, "wb")
local NF, LEAD, ON = {frames}, {lead}, {on}
LF:write("RIFF", string.pack("<I4", 36 + NF * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", NF * 8))
for i = 0, NF - 1 do
  local v = 0.0
  if i >= LEAD and i < LEAD + ON then v = {amp!r} * math.sin(2 * math.pi * 1000 * i / 48000)
  elseif i >= LEAD + ON then v = {quiet!r} * math.sin(2 * math.pi * 1000 * i / 48000) end
  LF:write(string.pack("<ff", v, v))
end
LF:close()
"""


TONE_LEN = 5.0                                         # seconds of steady 1 kHz tone (the "tone" stimulus)
TONE_DB = -12.0
TONE_SLOT = 7.0                                        # seconds per instance for the "tone" stimulus


def tone_lua(path: Path) -> str:
    """Lua that writes a steady 1 kHz tone WAV (stereo, identical channels) inside Kontakt."""
    frames = int(TONE_LEN * 48000)
    amp = 10 ** (TONE_DB / 20)
    return f"""local LF = io.open({json.dumps(str(path))}, "wb")
local NF = {frames}
LF:write("RIFF", string.pack("<I4", 36 + NF * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", NF * 8))
for i = 0, NF - 1 do
  local v = {amp!r} * math.sin(2 * math.pi * 1000 * i / 48000)
  LF:write(string.pack("<ff", v, v))
end
LF:close()
"""


TIMED = ('levels', 'burst', 'tone')  # stimuli that are one long WAV per note: hold the note for the slot
STIM_WAV = {'levels': 'levels_lr.wav', 'burst': 'burst_lr.wav', 'tone': 'tone_lr.wav'}


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
        f"local WAV = {json.dumps(eqrender.winpath(work / (STIM_WAV[spec.get('stimulus')] if spec.get('stimulus') in STIM_WAV else 'impulse_lr.wav')))}\n"
        f"local CHAIN = {{{chain}}}\n"
        + (levels_lua(work / "levels_lr.wav") if spec.get("stimulus") == "levels" else
           burst_lua(work / "burst_lr.wav") if spec.get("stimulus") == "burst" else
           tone_lua(work / "tone_lr.wav") if spec.get("stimulus") == "tone" else
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
    write_midi(len(spec["instances"]), mid, hold=SLOT - 1.0 if spec.get("stimulus") in TIMED else 0.1)
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


def burst_curve(chain, pre, steps, out_prefix: Path):
    """Attack/release of each setting against the bypassed reference, from the 1 ms RMS envelope of the burst.

    The tone starts at the first sample above threshold after the previous instance's tail (1.4 s after the note).
    Writes out_prefix.tsv: name, attack ms (tone onset to 63% of the gain 2 s in), release ms (end of the loud tone to
    63% recovered), gain 0.5 s / 1.0 s / 2.0 s into the tone (dB, active minus reference; the steady-state check).
    An instance whose sound ends before 4.9 s after onset is marked INCOMPLETE and its numbers are left blank."""
    global SLOT
    SLOT = BURST_LEAD + BURST_ON + BURST_OFF + 1.0
    tone_ms = off_ms = int(BURST_ON * 1000)
    rows = []
    for c0 in range(0, len(steps), CHUNK):
        part = steps[c0:c0 + CHUNK]
        wav = out_prefix.with_name(f"{out_prefix.name}_c{c0 // CHUNK}.wav")
        render_chunk(chain, [[*pre, *st["set"]] for st in part], wav, stimulus="burst")
        ch, rate, (L, R) = wavutil.read(wav)
        x = np.array(L)
        ref = burst_env_db(x, rate, FIRST)
        for i, st in enumerate(part, start=1):
            act = burst_env_db(x, rate, FIRST + i * SLOT)
            if act is None or ref is None:
                rows.append([c0 + i, st["name"], "", "", "", "", "", "INCOMPLETE"])
                continue
            gain = act - ref  # 1 ms bins from the tone onset, dB, negative = reduction
            steady = gain[2000]
            attack = next((k for k in range(0, tone_ms) if gain[k] <= 0.63 * steady), None)
            g0 = gain[tone_ms - 1]
            release = next((k for k in range(off_ms, len(gain)) if gain[k] >= 0.37 * g0), None)
            rows.append([c0 + i, st["name"], attack or "", "" if release is None else release - off_ms,
                         f"{gain[500]:.2f}", f"{gain[1000]:.2f}", f"{gain[2000]:.2f}", "ok"])
    tsv = out_prefix.with_suffix(".tsv")
    tsv.write_text("step\tname\tattack_ms\trelease_ms\tgain0.5s\tgain1.0s\tgain2.0s\tstatus\n"
                   + "\n".join("\t".join(map(str, r)) for r in rows) + "\n")
    return tsv


def burst_env_db(x, rate, note):
    """1 ms RMS dB from the tone onset of the instance whose note is at `note`; None if the instance is silent or the
    sound ends before 4.9 s after onset. The search starts 1.4 s after the note: the previous instance's tail ends there."""
    m = np.abs(x)
    a = int((note + 1.4) * rate)
    hit = np.nonzero(m[a:a + int(0.5 * rate)] > 1e-3)[0]
    if not len(hit):
        return None
    onset = a + int(hit[0])
    span = int(4.9 * rate)
    if onset + span > len(x):
        return None
    seg = x[onset:onset + int(5.5 * rate)]
    last = np.nonzero(np.abs(x[onset:onset + int(5.5 * rate)]) > 1e-4)[0]
    if not len(last) or last[-1] < span:
        return None
    bins = seg[:len(seg) // 48 * 48].reshape(-1, 48)
    return 10 * np.log10(np.mean(bins ** 2, axis=1) + 1e-14)


def tone_metrics(chain, pre, steps, out_prefix: Path):
    """Steady-tone response of each setting against the bypassed reference. Per instance, over 3 s of the steady
    part of the output: RMS dB, carrier (1 kHz) dB, the strongest sideband within 0.5..30 Hz on each side (its offset
    from the carrier is the modulation rate, its level relative to the carrier is the depth). Writes out_prefix.tsv:
    step, name, rms dB, carrier dB, rate Hz (mean of both sides), sideband dB (louder side, relative to carrier), dB
    delta of rms/carrier against the reference."""
    global SLOT
    SLOT = TONE_SLOT
    rows, ref = [], None
    for c0 in range(0, len(steps), CHUNK):
        part = steps[c0:c0 + CHUNK]
        wav = out_prefix.with_name(f"{out_prefix.name}_c{c0 // CHUNK}.wav")
        render_chunk(chain, [[*pre, *st["set"]] for st in part], wav, stimulus="tone")
        ch, rate, (L, R) = wavutil.read(wav)
        x = np.array(L)
        for i, st in enumerate(part, start=0):
            m = tone_stats(x, rate, FIRST + (i + 1) * SLOT)          # instance i+1 (instance 0 is the reference)
            rows.append([c0 + i + 1, st["name"], *m])
        if c0 == 0:
            ref = tone_stats(x, rate, FIRST)
    out = []
    for r in rows:
        out.append([r[0], r[1], *[f"{v:.2f}" if isinstance(v, float) else v for v in r[2:]],
                    f"{r[2] - ref[0]:.2f}", f"{r[3] - ref[1]:.2f}"])
    tsv = out_prefix.with_suffix(".tsv")
    hdr = "\t".join(["step", "name", "rms_db", "carrier_db", "rate_hz", "sideband_db", "d_rms_db", "d_carrier_db"])
    tsv.write_text(hdr + "\n" + "\n".join("\t".join(map(str, r)) for r in out) + "\n")
    print("ref\t" + "\t".join(f"{v:.2f}" for v in ref))
    return tsv


def tone_stats(x, rate, onset):
    """(rms dB, carrier dB, rate Hz, sideband dB) of one instance's steady tone, from the sound start after onset."""
    m = np.abs(x)
    lo = int(onset * rate)
    first = lo + int(np.argmax(m[lo:lo + int(SLOT * rate)] > 1e-4))
    seg = x[first + int(1.0 * rate): first + int(4.0 * rate)]
    n = len(seg)
    win = np.hanning(n)
    spec = np.abs(np.fft.rfft(seg * win)) * 2 / np.sum(win)
    freqs = np.fft.rfftfreq(n, 1 / rate)
    rms = 20 * np.log10(np.sqrt(np.mean(seg ** 2)) + 1e-12)
    carrier = spec[np.argmin(np.abs(freqs - 1000.0))]
    band = lambda a, b: (freqs >= a) & (freqs <= b)
    up = np.where(band(1000.5, 1030))[0]
    dn = np.where(band(970, 999.5))[0]
    ku, kd = up[np.argmax(spec[up])], dn[np.argmax(spec[dn])]
    f_up, f_dn = freqs[ku] - 1000.0, 1000.0 - freqs[kd]
    sb = max(spec[ku], spec[kd])
    return rms, 20 * np.log10(carrier + 1e-12), (f_up + f_dn) / 2, 20 * np.log10(sb / carrier + 1e-12)


def main() -> int:
    if sys.argv[1] == "tone":
        spec = json.load(open(sys.argv[2]))
        print(tone_metrics(spec["chain"], spec.get("pre", []), spec["steps"], Path(sys.argv[3]).resolve()))
        return 0
    if sys.argv[1] == "levels":
        spec = json.load(open(sys.argv[2]))
        print(level_curve(spec["chain"], spec.get("pre", []), spec["steps"], Path(sys.argv[3]).resolve()))
        return 0
    if sys.argv[1] == "burst":
        spec = json.load(open(sys.argv[2]))
        print(burst_curve(spec["chain"], spec.get("pre", []), spec["steps"], Path(sys.argv[3]).resolve()))
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
    write_midi(n, mid, hold=SLOT - 1.0 if spec.get("stimulus") in TIMED else 0.1)
    lua = work / "fxrig_gen.lua"
    lua.write_text(make_lua(spec, work))
    total = FIRST + n * SLOT + 2.0
    subprocess.run([sys.executable, str(HERE / "render.py"), str(lua), str(out_wav), str(total), str(mid)], check=True)
    print(out_wav)
    return 0


def write_midi(n: int, path: Path, hold: float = 0.1):
    """One note per instance on its own channel, SLOT seconds apart (PPQ 480, 120 bpm). `hold` is the note length:
    a long stimulus needs the note held, or the sample is cut by the note-off."""
    ev = []
    for k in range(n):
        t = FIRST + k * SLOT
        ev += [(t, bytes([0x90 | (BASE + k), 60, 100])), (t + hold, bytes([0x80 | (BASE + k), 60, 0]))]
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
