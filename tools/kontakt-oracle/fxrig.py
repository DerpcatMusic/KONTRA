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


TIMED = ('levels', 'burst')  # stimuli that are one long WAV per note: hold the note for the slot
STIM_WAV = {'levels': 'levels_lr.wav', 'burst': 'burst_lr.wav'}


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
    Writes out_prefix.tsv: name, attack ms (onset to 63% of the gain 2 s in), release ms (quiet tone onset to 63%
    recovered, i.e. 37% of the reduction left, against the gain at the quiet onset), gain 0.5 s / 1.0 s / 2.0 s into
    the loud tone (dB, active minus reference; the steady-state check)."""
    global SLOT
    SLOT = BURST_LEAD + BURST_ON + BURST_OFF + 1.0
    rows = []
    for c0 in range(0, len(steps), CHUNK):
        part = steps[c0:c0 + CHUNK]
        wav = out_prefix.with_name(f"{out_prefix.name}_c{c0 // CHUNK}.wav")
        render_chunk(chain, [[*pre, *st["set"]] for st in part], wav, stimulus="burst")
        ch, rate, (L, R) = wavutil.read(wav)
        x = np.array(L)
        ref = env_db(x, rate, FIRST)
        for i, st in enumerate(part, start=1):
            act = env_db(x, rate, FIRST + i * SLOT)
            gain = act - ref  # per 1 ms, dB, negative = reduction
            on = int(BURST_LEAD * 1000)
            off = on + int(BURST_ON * 1000)
            steady = gain[on + 2000]
            attack = next((k for k in range(on, off) if gain[k] <= 0.63 * steady), None)
            release = next((k for k in range(off, len(gain)) if gain[k] >= 0.37 * gain[off]), None)
            rows.append([c0 + i, st["name"],
                         "" if attack is None else attack - on,
                         "" if release is None else release - off,
                         f"{gain[on + 500]:.2f}", f"{gain[on + 1000]:.2f}", f"{gain[on + 2000]:.2f}"])
    tsv = out_prefix.with_suffix(".tsv")
    tsv.write_text("\n".join("\t".join(map(str, r)) for r in rows) + "\n")
    return tsv


def env_db(x, rate, onset):
    """RMS dB in 1 ms bins from the instance's first sound (file start = first sample above 1e-4 after onset)."""
    m = np.abs(x)
    lo = int(onset * rate)
    first = lo + int(np.argmax(m[lo:lo + int(SLOT * rate)] > 1e-4))
    n = int(BURST_LEAD * 1000 + (BURST_ON + BURST_OFF) * 1000)
    seg = x[first - int(BURST_LEAD * rate):first - int(BURST_LEAD * rate) + n * 48]
    bins = seg.reshape(-1, 48)
    return 10 * np.log10(np.mean(bins ** 2, axis=1) + 1e-12)


TONE_SLOT = 5.0     # seconds per instance (live modes): note 0..4.5 s, one CC20 step at STEP_AT, then silence
STEP_AT = 1.5       # seconds after the note-on that CC20 arrives (controller applies the "post" sets)


def tone_lua(path: Path, freq: float, db: float, dur: float) -> str:
    """Lua that writes a steady sine WAV (stereo, identical channels, starts at sample 0) inside Kontakt."""
    frames = int(dur * 48000)
    amp = 10 ** (db / 20)
    return f"""local LF = io.open({json.dumps(str(path))}, "wb")
local NF = {frames}
LF:write("RIFF", string.pack("<I4", 36 + NF * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", NF * 8))
for i = 0, NF - 1 do
  local v = {amp!r} * math.sin(2 * math.pi * {freq!r} * i / 48000)
  LF:write(string.pack("<ff", v, v))
end
LF:close()
"""


def tone_session_lua(spec, work: Path) -> str:
    """One session: each instance plays the tone on its own channel and zone. `pre` sets apply in on init;
    `post` sets apply in on controller when CC20 arrives (the live control change at STEP_AT). Instance 0 is
    the bypassed reference (EFFECT_BYPASS=1)."""
    wav = work / "tone_lr.wav"
    tone = spec["stimulus"]
    chain_paths = [eqrender.winpath(eqrender.init_preset(c)) for c in spec["chain"]]
    body = []
    for k, inst in enumerate(spec["instances"]):
        init = "\\n".join(["on init"] + [f"  set_engine_par($ENGINE_PAR_{p}, {int(v)}, -1, {s}, 0)" for s, p, v in inst["pre"]] + ["end on"])
        ctrl = "\\n".join(["on controller", "  if ($CC_NUM = 20)"] + [f"    set_engine_par($ENGINE_PAR_{p}, {int(v)}, -1, {s}, 0)" for s, p, v in inst.get("post", [])] + ["  end if", "end on"])
        body.append(f"""
do
  local i = kt.add_instrument()
  kt.set_instrument_midi_channel(i, {k + 1 + BASE})
  local z = kt.add_zone(i, 0, WAV)
  kt.set_zone_geometry(i, z, {{ root_key = 60, low_key = 0, high_key = 127 }})
  for slot, p in ipairs(CHAIN) do log('{{"load":' .. tostring(pcall(kt.load_fx_preset, p, i, -1, slot - 1, kt.insert_fx)) .. ',"inst":{k}}}') end
  log('{{"script":' .. tostring(pcall(kt.set_instrument_script_source, i, 0, "{init}\\n{ctrl}")) .. ',"inst":{k}}}')
end""")
    chain = ",".join(json.dumps(p) for p in chain_paths)
    return (
        "local kt = Kontakt\n"
        "local out = assert(io.open(ORACLE_OUT, \"w\"))\n"
        "local function log(t) out:write(t .. \"\\n\") out:flush() end\n"
        f"local WAV = {json.dumps(eqrender.winpath(wav))}\n"
        f"local CHAIN = {{{chain}}}\n"
        + tone_lua(work / "tone_lr.wav", tone["freq"], tone["db"], tone["dur"])
        + "kt.reset_multi()\n"
        + "".join(body)
        + "\nlog('{\"ready\":true}')\nout:close()\n"
    )


def write_midi_step(n: int, path: Path):
    """Per instance: note-on at its slot start, CC20=127 at STEP_AT, note-off at the slot end (PPQ 480, 120 bpm)."""
    ev = []
    for k in range(n):
        t = FIRST + k * TONE_SLOT
        ch = BASE + k
        ev += [(t, bytes([0x90 | ch, 60, 100])), (t + STEP_AT, bytes([0xB0 | ch, 20, 127])),
               (t + TONE_SLOT - 0.5, bytes([0x80 | ch, 60, 0]))]
    trk, last = b"\x00\xff\x51\x03" + (500000).to_bytes(3, "big"), 0
    for t, m in sorted(ev, key=lambda e: e[0]):
        tick = round(t * 960)
        trk += eqrender.vlq(tick - last) + m
        last = tick
    trk += b"\x00\xff\x2f\x00"
    head = b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1]) + (480).to_bytes(2, "big")
    path.write_bytes(head + b"MTrk" + len(trk).to_bytes(4, "big") + trk)


def render_tone(spec, out_wav: Path):
    """One Kontakt session for a tone spec (live CC20 steps). Returns nothing; the WAV is written to out_wav."""
    work = eqrender.scratch_dir()
    mid, lua = work / "rig.mid", work / "fxrig_tone.lua"
    write_midi_step(len(spec["instances"]), mid)
    lua.write_text(tone_session_lua(spec, work))
    total = FIRST + len(spec["instances"]) * TONE_SLOT + 2.0
    subprocess.run([sys.executable, str(HERE / "render.py"), str(lua), str(out_wav), str(total), str(mid)], check=True)


def time_origin(x, rate):
    """Seconds from the file start to the scheduled MIDI time 0. The recording starts before the MIDI (render.py
    sleeps 1 s after pw-record), and instances play back to back, so silence cannot mark each onset. The first
    instance (ref) starts from silence: its first sound gives the offset for the whole session."""
    first = int(np.argmax(np.abs(x) > 1e-4))
    return first / rate - FIRST


def env_bins(x, rate, onset_s, t0, t1, bin_s, hilbert=False):
    """dB in bins of bin_s seconds from t0 to t1, relative to onset_s (scheduled instance start in file time).
    RMS per bin, or (hilbert=True) the mean analytic amplitude per bin: no ripple for tones below ~1 kHz, where a
    bin cannot hold a whole period."""
    nb = int(round(bin_s * rate))
    a0, a1 = int((onset_s + t0) * rate), int((onset_s + t1) * rate)
    seg = x[a0:a1]
    if hilbert:
        n = len(seg)
        h = np.zeros(n)
        h[0] = 1
        h[1:(n + 1) // 2] = 2
        if n % 2 == 0:
            h[n // 2] = 1
        seg = np.abs(np.fft.ifft(np.fft.fft(seg) * h)) / np.sqrt(2)
        return np.array([10 * np.log10(np.mean(seg[i:i + nb] ** 2) + 1e-14) for i in range(0, len(seg) - nb + 1, nb)])
    return np.array([10 * np.log10(np.mean(seg[i:i + nb] ** 2) + 1e-14) for i in range(0, len(seg) - nb + 1, nb)])


def cross_frac(t, y, pre, post, t_from, frac):
    """First time after t_from where y passes pre + frac (post - pre), linear interpolation; None if no step."""
    if abs(post - pre) < 0.05:
        return None
    lvl = pre + frac * (post - pre)
    up = post > pre
    for i in np.where(t >= t_from)[0][1:]:
        if np.isfinite(y[i]) and np.isfinite(y[i - 1]) and ((y[i] >= lvl) if up else (y[i] <= lvl)) and y[i] != y[i - 1]:
            return float(t[i - 1] + (lvl - y[i - 1]) / (y[i] - y[i - 1]) * (t[i] - t[i - 1]))
    return None


def fit_step(t, y, t_nom, span=0.02, win=0.5):
    """One-pole step fit with the start time free: y = pre before t0, y = a + b*exp(-(t - t0)/tau) from t0.
    t0 is searched within +-span of t_nom (the scheduled CC20), tau on a log grid. CC20 timing jitter between
    instances is a pure shift, so a free t0 keeps tau free of it. Returns (t0, tau, plateau, pre, rms residual)."""
    m = (t > t_nom - span - 0.05) & (t < t_nom + win) & np.isfinite(y)
    tt, yy = t[m], y[m]
    pre = float(np.mean(yy[tt < t_nom - span]))
    best = None
    for t0 in np.arange(t_nom - span, t_nom + span + 1e-9, 0.00025):
        on = tt >= t0
        for tau in np.geomspace(0.0005, 0.5, 240):
            e = np.exp(-(tt[on] - t0) / tau)
            A = np.stack([np.ones_like(e), e], axis=1)
            coef, *_ = np.linalg.lstsq(A, yy[on], rcond=None)
            r = np.concatenate([yy[~on] - pre, yy[on] - A @ coef])
            rms = float(np.sqrt(np.mean(r * r)))
            if best is None or rms < best[4]:
                best = (float(t0), float(tau), float(coef[0]), pre, rms)
    return best


def fit_ramp(t, y, t_nom, span=0.02, win=0.5):
    """Linear-ramp alternative to fit_step (dB domain): y = pre, then a linear ramp of length D from t0 to the
    plateau. Returns (t0, D, rms residual). Same free start time as fit_step."""
    m = (t > t_nom - span - 0.05) & (t < t_nom + win) & np.isfinite(y)
    tt, yy = t[m], y[m]
    pre = float(np.mean(yy[tt < t_nom - span]))
    best = None
    for t0 in np.arange(t_nom - span, t_nom + span + 1e-9, 0.00025):
        for dur in np.arange(0.0005, 0.1, 0.0005):
            A = np.clip((tt - t0) / dur, 0, 1)[:, None]
            c, *_ = np.linalg.lstsq(A, yy - pre, rcond=None)
            rms = float(np.sqrt(np.mean((yy - pre - A @ c) ** 2)))
            if best is None or rms < best[2]:
                best = (float(t0), float(dur), rms)
    return best


def step_analysis(spec, wav: Path, out_prefix: Path):
    """Per gain instance (gain = active minus reference, dB, bin_ms bins; dropouts below -60 dB excluded):
    model-free 10-90% rise time, and a one-pole fit (free start time, least squares) on dB and on the linear gain.
    The bypass instances (post EFFECT_BYPASS=1, a hard switch) are only a per-event timing check: each CC20 has its
    own MIDI jitter, so they are reported and not used to shift the gain fits.
    Writes out_prefix.tsv (raw envelope rows) and out_prefix.fit.tsv."""
    ch, rate, (L, R) = wavutil.read(wav)
    x = np.array(L)
    bin_s = spec.get("bin_ms", 1.0) / 1000  # one whole period of the tone per bin (1 kHz: 1 ms; 40 Hz: 25 ms)
    off = time_origin(x, rate)
    t = np.arange(int(round((STEP_AT + 3.0 + 0.3) / bin_s))) * bin_s - 0.3
    hil = bool(spec.get("hilbert", False))
    ref = env_bins(x, rate, FIRST + off, -0.3, STEP_AT + 3.0, bin_s, hil)
    gains, raw = {}, []
    for k, inst in enumerate(spec["instances"]):
        if k == 0:
            continue
        env = env_bins(x, rate, FIRST + k * TONE_SLOT + off, -0.3, STEP_AT + 3.0, bin_s, hil)
        gain = env - ref
        gain[(env < -60) | (ref < -60)] = np.nan  # recording dropouts (digital silence inside the tone)
        gains[inst["name"]] = (inst, gain)
        raw += [[inst["name"], f"{tt * 1000:.1f}", f"{g:.3f}"] for tt, g in zip(t, gain)]
    cal, fits = [], []
    for name, (inst, gain) in gains.items():
        pre = float(np.nanmean(gain[(t > 0.2) & (t < STEP_AT - 0.1)]))
        post = float(np.nanmean(gain[(t > STEP_AT + 0.2) & (t < STEP_AT + 0.5)]))  # before any note-off cut
        if any(p == "EFFECT_BYPASS" for _, p, _ in inst.get("post", [])):
            if abs(post - pre) > 0.5:
                cal.append(f"{name} {(cross_frac(t, gain, pre, post, STEP_AT - 0.3, 0.632) - STEP_AT) * 1000:.1f}")
            continue
        r10, r90 = (cross_frac(t, gain, pre, post, STEP_AT - 0.1, f) for f in (0.1, 0.9))
        rise = "n/a" if r10 is None or r90 is None else f"{(r90 - r10) * 1000:.2f}"
        db = fit_step(t, gain, STEP_AT)
        lin = fit_step(t, 10 ** (gain / 20), STEP_AT)
        ramp = fit_ramp(t, gain, STEP_AT)
        ramp_lin = fit_ramp(t, 10 ** (gain / 20), STEP_AT)
        fits.append([name, f"{(db[0] - STEP_AT) * 1000:.2f}", f"{db[1] * 1000:.2f}", f"{db[4]:.3f}", f"{pre:.2f}",
                     f"{post:.2f}", rise, f"{(lin[0] - STEP_AT) * 1000:.2f}", f"{lin[1] * 1000:.2f}", f"{lin[4]:.4f}",
                     f"{ramp[1] * 1000:.2f}", f"{ramp[2]:.3f}", f"{ramp_lin[1] * 1000:.2f}", f"{ramp_lin[2]:.5f}",
                     str(int(np.sum(~np.isfinite(gain[t > STEP_AT]))))])
    tsv = out_prefix.with_suffix(".tsv")
    tsv.write_text("name\tt_ms\tgain_db\n" + "\n".join("\t".join(map(str, r)) for r in raw) + "\n")
    fit_tsv = out_prefix.with_name(out_prefix.name + ".fit.tsv")
    fit_tsv.write_text("name\tdb_t0_ms\tdb_tau_ms\tdb_resid_db\tpre_db\tpost_db\trise10_90_ms\tlin_t0_ms\tlin_tau_ms\tlin_resid\tramp_dur_ms\tramp_resid_db\tramp_lin_dur_ms\tramp_lin_resid\tdropout_bins\n"
                       + "\n".join("\t".join(map(str, r)) for r in fits)
                       + f"\n# t0 relative to the scheduled CC20 at {STEP_AT} s. time origin {off:.4f} s. bypass-edge timing check (63%, ms): {'; '.join(cal) or 'none'}\n")
    return tsv, fit_tsv


def spectrum_analysis(spec, wav: Path, out_prefix: Path):
    """Steady-state spectrum per instance (a 1 s Hann window after the tone settles). Reports the fundamental, the
    2nd and 3rd harmonics, and the alias bin of the 3rd harmonic (48 kHz - 3f when 3f > 24 kHz), in dBFS-relative
    dB of the fundamental, plus the floor at 22 kHz (no content expected there)."""
    ch, rate, (L, R) = wavutil.read(wav)
    x = np.array(L)
    f0 = spec["stimulus"]["freq"]
    off = time_origin(x, rate)
    rows = []
    for k, inst in enumerate(spec["instances"]):
        on = FIRST + k * TONE_SLOT + off
        lo = int((on + 0.5) * rate)
        seg = x[lo:lo + rate] * np.hanning(rate)
        mag = 20 * np.log10(np.abs(np.fft.rfft(seg)) / np.sum(np.hanning(rate)) * 2 + 1e-12)
        fr = np.fft.rfftfreq(rate, 1 / rate)

        def at(f, half=30.0):
            sel = (fr > f - half) & (fr < f + half)
            return float(np.max(mag[sel]))
        f3 = 3 * f0
        alias = 2 * 24000 - f3 if f3 > 24000 else f3
        base = at(f0)
        h3 = f"{at(f3) - base:.2f}" if f3 < 24000 else "above-nyquist"  # 3f > 24 kHz folds to the alias bin
        rows.append([inst["name"], f"{base:.2f}", f"{at(2 * f0) - base:.2f}", h3,
                     f"{at(alias) - base:.2f}", f"{at(22000) - base:.2f}"])
    tsv = out_prefix.with_suffix(".tsv")
    tsv.write_text("name\tfund_dbfs\th2_rel_db\th3_rel_db\talias_rel_db\tfloor22k_rel_db\n"
                   + "\n".join("\t".join(map(str, r)) for r in rows) + "\n")
    return tsv


def tone_cmd(spec_path: Path, out_prefix: Path, wav: Path = None):
    """tone SPEC.json OUT_PREFIX [WAV]: one session (or, given WAV, analysis only); mode 'step' (CC20 live steps)
    or 'spectrum' (static, no CC)."""
    spec = json.load(open(spec_path))
    wav = wav or out_prefix.with_suffix(".wav")
    if len(spec["instances"]) > CHANNELS:
        sys.exit(f"fxrig tone: {len(spec['instances'])} instances, this oracle slot has {CHANNELS} MIDI channels; split the spec")
    if not wav.exists():
        render_tone(spec, wav)
    if spec.get("mode", "step") == "spectrum":
        return spectrum_analysis(spec, wav, out_prefix)
    return step_analysis(spec, wav, out_prefix)


def main() -> int:
    if sys.argv[1] == "tone":
        print(tone_cmd(Path(sys.argv[2]), Path(sys.argv[3]).resolve(), Path(sys.argv[4]).resolve() if len(sys.argv) > 4 else None))
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
