#!/usr/bin/env python3
"""Impulse responses of Kontakt effect chains, measured through Kontakt itself.

    eqrender.py SPEC.json OUT_PREFIX            (EQ_REUSE=1: analyse an existing OUT_PREFIX.wav)

SPEC: {"chain": ["Filters/Solid G-EQ", ...],      # preset folders loaded into insert slots 0..
       "pre":   [[slot, "PAR", value], ...],       # baseline every step returns to
       "steps": [{"name": "...", "set": [[slot, "PAR", value], ...]}, ...]}     (at most 126)

Render step 0 is a full bypass of every slot (the reference).  Timing comes from a MIDI file played by
aplaymidi, so it is exact: per step, CC#20 = step index (the KSP `on controller` writes that step's
changed parameters with set_engine_par), 350 ms later note C3 plays a stereo sample whose left channel is
a unit impulse and whose right channel is silent.  The recording (render.py: private PipeWire null sink)
is cut at the scheduled onsets; each response is FFT-ed and divided by the reference.
OUT_PREFIX.tsv: one row per step and channel pair with peak/bandwidth metrics and dB at fixed probe
frequencies; OUT_PREFIX.h.npy: the LL magnitude responses in dB (rfft bins, N=16384).
"""
import json
import os
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import wavutil  # noqa: E402

PRESETS = Path("/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/Presets")
PERIOD = 1.6      # seconds between steps
PLAY_AT = 0.3    # note this long after the step's CC (settle time)
PROBES = [30, 60, 100, 200, 500, 1000, 2000, 5000, 10000, 15000]


def init_preset(rel: str) -> Path:
    d = PRESETS / rel
    return (sorted(d.glob("- INIT*.nkp")) or sorted(d.glob("*.nkp")))[0]


def winpath(p) -> str:
    return "Z:" + str(p).replace("/", "\\")


def commands(spec):
    """Per step (ref first) the set_engine_par calls that change the state; only changes are sent."""
    n = len(spec["chain"])
    base = {(s, "EFFECT_BYPASS"): 0 for s in range(n)}
    base.update({(a, b): c for a, b, c in map(tuple, spec.get("pre", []))})
    state, out = {}, []
    wants = [{(s, "EFFECT_BYPASS"): 1 for s in range(n)}]
    for st in spec["steps"]:
        w = dict(base)
        w.update({(a, b): c for a, b, c in map(tuple, st["set"])})
        wants.append(w)
    for w in wants:
        ch = [(k[0], k[1], v) for k, v in w.items() if state.get(k) != v]
        state.update({(a, b): c for a, b, c in ch})
        out.append(ch)
    return out


IMPULSE_LUA = """local WAV = @WAV@
local f = assert(io.open(WAV, "wb"))
local n = 24000
f:write("RIFF", string.pack("<I4", 36 + n * 8), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 2, 48000, 384000, 8, 32), "data", string.pack("<I4", n * 8))
for i = 0, n - 1 do f:write(string.pack("<ff", i == 0 and 0.5 or 0.0, 0.0)) end
f:close()
"""


def make_lua(spec, wav_dir: Path, stimulus: Path = None, extra_lua: str = "") -> str:
    """stimulus: an existing stereo WAV to play instead of the generated impulse."""
    cmds, starts = [], [0]
    for ch in commands(spec):
        cmds += ch
        starts.append(len(cmds))
    m = len(cmds) + 1
    ksp = ["on init", "  declare $k", f"  declare %cpar[{m}]", f"  declare %cslot[{m}]",
           f"  declare %cval[{m}]", f"  declare %cstart[{len(starts) + 1}]"]
    for i, (slot, par, val) in enumerate(cmds):
        ksp += [f"  %cpar[{i}] := $ENGINE_PAR_{par}", f"  %cslot[{i}] := {slot}", f"  %cval[{i}] := {val if isinstance(val, str) else int(val)}"]
    ksp += [f"  %cstart[{i}] := {s}" for i, s in enumerate(starts)]
    ksp += ["end on", "on controller", "  if ($CC_NUM = 20)", "    $k := %cstart[%CC[20]]",
            "    while ($k < %cstart[%CC[20] + 1])",
            f"      set_engine_par(%cpar[$k], %cval[$k], -1, %cslot[$k], {spec.get('generic', 0)})",
            "      inc($k)", "    end while", "  end if", "end on"]
    chain = ",".join(json.dumps(winpath(init_preset(c))) for c in spec["chain"])
    ksp_text = "\\n".join(ksp)
    gen = f"local WAV = {json.dumps(winpath(stimulus))}\n" if stimulus else IMPULSE_LUA.replace("@WAV@", json.dumps(winpath(wav_dir / "impulse_lr.wav")))
    return f"""
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\\n") out:flush() end
{gen}kt.reset_multi()
local inst = kt.add_instrument()
local z = kt.add_zone(inst, 0, WAV)
kt.set_zone_geometry(inst, z, {{ root_key = 60, low_key = 0, high_key = 127 }})
kt.set_instrument_volume(inst, {spec.get('inst_volume', 0)})
local chain = {{{chain}}}
for slot, p in ipairs(chain) do log('{{"load":' .. tostring(pcall(kt.load_fx_preset, p, inst, -1, slot - 1, kt.insert_fx)) .. '}}') end
log('{{"script":' .. tostring(pcall(kt.set_instrument_script_source, inst, 0, "{ksp_text}")) .. '}}')
{extra_lua}
log('{{"ready":true}}')
out:close()
"""


def vlq(n: int) -> bytes:
    b = [n & 0x7F]
    n >>= 7
    while n:
        b.append((n & 0x7F) | 0x80)
        n >>= 7
    return bytes(reversed(b))


def make_midi(nsteps: int, path: Path):
    """PPQ 480 at 120 bpm: 960 ticks per second."""
    ev = []
    for i in range(nsteps):
        t = 2.0 + i * PERIOD
        ev.append((t, bytes([0xB0, 20, i])))
        for k in ((0, 0.12, 0.24, 0.36) if i == 0 else (0, 0.5)):   # step 0: a 4-note burst marks the reference; the note is sent twice: Kontakt occasionally drops one while rebuilding DSP
            ev += [(t + PLAY_AT + k, bytes([0x90, 60, 100])), (t + PLAY_AT + k + 0.1, bytes([0x80, 60, 0]))]
    trk, last = b"\x00\xff\x51\x03" + (500000).to_bytes(3, "big"), 0
    for t, m in sorted(ev, key=lambda e: e[0]):
        tick = round(t * 960)
        trk += vlq(tick - last) + m
        last = tick
    trk += b"\x00\xff\x2f\x00"
    head = b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1]) + (480).to_bytes(2, "big")
    path.write_bytes(head + b"MTrk" + len(trk).to_bytes(4, "big") + trk)


def metrics(freqs, db):
    k = int(np.argmax(np.abs(db)))
    pk = db[k]
    half, sign = pk / 2, (1 if pk >= 0 else -1)
    i = k
    while i > 0 and sign * db[i] > sign * half:
        i -= 1
    lo = freqs[i]
    i = k
    while i < len(db) - 1 and sign * db[i] > sign * half:
        i += 1
    return freqs[k], pk, lo, freqs[i]


CHUNK = 60   # steps per Kontakt session


def run(spec, prefix, first_no=1):
    steps = spec["steps"]
    work = Path("/tmp/w3/audit")
    work.mkdir(parents=True, exist_ok=True)
    lua, mid, wav = work / "eqrender_gen.lua", work / "eqrender.mid", prefix.with_suffix(".wav")
    lua.write_text(make_lua(spec, work))
    make_midi(len(steps) + 1, mid)
    if not os.environ.get("EQ_REUSE"):
        subprocess.run([sys.executable, str(HERE / "render.py"), str(lua), str(wav),
                        str((2.0 + (len(steps) + 1) * PERIOD) * 1.05 + 6), str(mid)], check=True)
    ch, rate, (L, R) = wavutil.read(wav)
    L, R = np.array(L), np.array(R)
    mag = np.maximum(np.abs(L), np.abs(R))
    # MIDI order is exact but wall-clock spacing jitters by ~100 ms: step n is the loudest sample within
    # [prev + 0.75 s, prev + 2.45 s] (each step plays two identical notes); the analysis window starts 256 samples earlier (magnitude only, so
    # the exact alignment does not matter as long as the whole response fits in the window).
    # ALSA playback is accurate to a few ms (but starts ~1.3 s late and occasionally drops a note): anchor on the
    # reference impulse, then step n is the loudest sample in [T0 + n*PERIOD - 0.25, + 0.8] (its first or
    # second, identical, note).
    i0 = int(np.nonzero(mag > 0.05)[0][0])
    T0 = i0 + int(np.argmax(mag[i0:i0 + int(0.05 * rate)]))
    burst = [T0 + int(k * rate) for k in (0.12, 0.24, 0.36)]
    if any(mag[b - int(0.03 * rate):b + int(0.03 * rate)].max() < 0.05 for b in burst):
        sys.exit("first sound is not the 4-note reference burst (notes lost at start?)")
    last = burst[-1] - int(0.03 * rate)
    ev = [last + int(np.argmax(mag[last:last + int(0.06 * rate)]))]   # last burst note: nothing follows it within the FFT window
    drift = 0
    for n in range(1, len(steps) + 1):
        lo = T0 + int((n * PERIOD - 0.25) * rate) + drift
        w = mag[lo:lo + int(1.05 * rate)]
        if not len(w) or w.max() < 1e-6:
            ev.append(None)       # Kontakt dropped both notes (recording is exact digital silence otherwise): caller retries
            continue
        ev.append(lo + int(np.argmax(w)))
    missing = [n for n, e in enumerate(ev) if e is None]
    ev = [e - 240 if e is not None else None for e in ev]
    print(f"{len(ev) - len(missing)} impulses, missing steps {missing}")
    N = 16384

    def spectrum(x, start):
        seg = np.zeros(N)
        part = x[start - 16:start - 16 + N]
        seg[:len(part)] = part
        return np.fft.rfft(seg)

    ref = spectrum(L, ev[0])
    freqs = np.fft.rfftfreq(N, 1 / rate)
    sel = (freqs >= 20) & (freqs <= 20000)
    rows = [""]
    allh = []
    for n, st in enumerate(steps, start=1):
        if ev[n] is None:
            continue
        for pair, x in (("LL", L), ("LR", R)):
            h = spectrum(x, ev[n]) / np.where(np.abs(ref) < 1e-9, 1e-9, ref)
            db = 20 * np.log10(np.maximum(np.abs(h), 1e-9))
            if pair == "LL":
                allh.append(db)
            f0, pk, lo, hi = metrics(freqs[sel], db[sel])
            at = [np.interp(p, freqs, db) for p in PROBES]
            rows.append("\t".join([str(first_no + n - 1), st["name"], pair, f"{f0:.1f}", f"{pk:.2f}", f"{lo:.1f}", f"{hi:.1f}",
                                   f"{f0 / max(hi - lo, 1e-9):.3f}", f"{np.log2(hi / lo) if lo > 0 else float('nan'):.3f}"]
                                  + [f"{v:.2f}" for v in at]))
    return rows[1:], allh, missing


def main() -> int:
    spec = json.load(open(sys.argv[1]))
    prefix = Path(sys.argv[2])
    rows, allh = [], []
    for c0 in range(0, len(spec["steps"]), CHUNK):
        part = dict(spec, steps=spec["steps"][c0:c0 + CHUNK])
        todo = part
        for attempt in range(3):          # re-render steps whose notes Kontakt dropped
            r, h, miss = run(todo, prefix.with_name(prefix.name + f"_c{c0 // CHUNK}_{attempt}"), 1)
            nums = []
            for row in r:
                f = row.split("\t")
                f[0] = str(c0 + todo["orig"][int(f[0]) - 1] + 1) if "orig" in todo else str(c0 + int(f[0]))
                rows.append("\t".join(f))
                if f[2] == "LL":
                    nums.append(int(f[0]))
            allh += list(zip(nums, h))
            if not miss:
                break
            keep = [(todo["orig"][m - 1] if "orig" in todo else m - 1) for m in miss if m > 0]
            todo = dict(part, steps=[part["steps"][k] for k in keep], orig=keep)
    rows.sort(key=lambda r: (int(r.split("\t")[0]), r.split("\t")[2]))
    allh = [h for _, h in sorted(allh, key=lambda x: x[0])]
    head = "step\tname\tpair\tpeak_hz\tpeak_db\thalf_lo_hz\thalf_hi_hz\tq_half\toctaves_half\t" + "\t".join(f"db@{p}" for p in PROBES)
    prefix.with_suffix(".tsv").write_text("\n".join([head] + rows) + "\n")
    np.save(prefix.with_suffix(".h.npy"), np.array(allh))
    print(prefix.with_suffix(".tsv"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
