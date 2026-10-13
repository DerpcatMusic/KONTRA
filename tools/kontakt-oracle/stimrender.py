#!/usr/bin/env python3
"""Time-domain renders of a nonlinear effect through Kontakt: one stimulus, many parameter steps.

    stimrender.py SPEC.json OUT_PREFIX         -> OUT_PREFIX.npz: x (stimulus), y[step] (L: through the chain), r[step] (R: same
                                                  stimulus on a second, effect-free instrument), names

SPEC: {"chain": [...], "pre": [[slot, "PAR", value], ...], "steps": [...]} as eqrender.py (values may be KSP
constants such as "$EFFECT_TYPE_LIMITER"), plus "stimulus": one of the builders below.  Step 0 is the
bypassed chain.  One note per step whose length equals the stimulus; steps 1 s apart plus the stimulus.
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

RATE = 48000
WORK = Path("/tmp/w3/audit")


def stim_limiter():
    """0.3 s DC at 0.6, 0.3 s silence, 0.3 s 1 kHz sine ramping 0 -> 1.0 (static curve)."""
    n = int(0.3 * RATE)
    t = np.arange(n) / RATE
    return np.concatenate([np.full(n, 0.6), np.zeros(n), np.sin(2 * np.pi * 1000 * t) * np.linspace(0, 1.0, n)])


def stim_ramp():
    """0.6 s 200 Hz sine ramping 0 -> 1.0."""
    n = int(0.6 * RATE)
    t = np.arange(n) / RATE
    return np.sin(2 * np.pi * 200 * t) * np.linspace(0, 1.0, n)


def stim_release():
    """0.2 s DC at 0.6 (drives gain reduction), then 0.7 s of a quiet 2 kHz probe sine (0.02) to watch the recovery."""
    n1, n2 = int(0.2 * RATE), int(0.7 * RATE)
    return np.concatenate([np.full(n1, 0.6), 0.02 * np.sin(2 * np.pi * 2000 * np.arange(n2) / RATE)])


STIMULI = {"limiter": stim_limiter, "ramp": stim_ramp, "release": stim_release}


def write_wav(path: Path, x, right=False):
    data = np.stack([np.zeros_like(x), x] if right else [x, np.zeros_like(x)], 1).astype("<f4").tobytes()
    path.write_bytes(b"RIFF" + (36 + len(data)).to_bytes(4, "little") + b"WAVEfmt " +
                     (16).to_bytes(4, "little") + (3).to_bytes(2, "little") + (2).to_bytes(2, "little") +
                     RATE.to_bytes(4, "little") + (RATE * 8).to_bytes(4, "little") + (8).to_bytes(2, "little") +
                     (32).to_bytes(2, "little") + b"data" + len(data).to_bytes(4, "little") + data)


def make_midi(nsteps, dur, path: Path):
    ev, period = [], dur + 1.0
    for i in range(nsteps):
        t = 2.0 + i * period
        ev += [(t, bytes([0xB0, 20, i])), (t + 0.4, bytes([0x90, 60, 100])), (t + 0.4 + dur, bytes([0x80, 60, 0]))]
    trk, last = b"\x00\xff\x51\x03" + (500000).to_bytes(3, "big"), 0
    for t, m in sorted(ev, key=lambda e: e[0]):
        tick = round(t * 960)
        trk += eqrender.vlq(tick - last) + m
        last = tick
    trk += b"\x00\xff\x2f\x00"
    path.write_bytes(b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1]) + (480).to_bytes(2, "big") +
                     b"MTrk" + len(trk).to_bytes(4, "big") + trk)
    return period


def main():
    spec = json.load(open(sys.argv[1]))
    prefix = Path(sys.argv[2])
    x = STIMULI[spec["stimulus"]]()
    dur = len(x) / RATE
    WORK.mkdir(parents=True, exist_ok=True)
    stim = WORK / "stimulus.wav"
    write_wav(stim, x)
    ref = WORK / "stimulus_ref.wav"      # same stimulus, right channel, on a second instrument without effects:
    write_wav(ref, x, right=True)        # the same note plays it, so R is a latency-free reference for L
    extra = (f"local inst2 = kt.add_instrument()\nlocal z2 = kt.add_zone(inst2, 0, {json.dumps(eqrender.winpath(ref))})\n"
             "kt.set_zone_geometry(inst2, z2, { root_key = 60, low_key = 0, high_key = 127 })\nkt.set_instrument_volume(inst2, 0)\n"
             "log('{\"midi\":\"' .. tostring(kt.get_instrument_midi_channel(inst)) .. ' ' .. "
             "tostring(kt.get_instrument_midi_channel(inst2)) .. '\"}')\n"
             "log('{\"setmidi\":' .. tostring(pcall(kt.set_instrument_midi_channel, inst2, "
             "kt.get_instrument_midi_channel(inst))) .. '}')\n")
    lua = eqrender.make_lua(spec, WORK, stim, extra)
    (WORK / "stim_gen.lua").write_text(lua)
    nsteps = len(spec["steps"]) + 1
    period = make_midi(nsteps, dur, WORK / "stim.mid")
    wav = prefix.with_suffix(".wav")
    subprocess.run([sys.executable, str(HERE / "render.py"), str(WORK / "stim_gen.lua"), str(wav),
                    str((2.0 + nsteps * period) * 1.05 + 6), str(WORK / "stim.mid")], check=True)
    _, rate, (L, R) = wavutil.read(wav)
    L, R = np.array(L), np.array(R)
    mag = np.maximum(np.abs(L), np.abs(R))
    T0 = int(np.nonzero(mag > 1e-4)[0][0])
    n_seg = len(x) + int(0.3 * RATE)
    ys, rs, lags = [], [], []
    for i in range(nsteps):
        lo = T0 + int((i * period - 0.25) * RATE)
        hit = np.nonzero(mag[lo:lo + int(0.6 * RATE)] > 1e-4)[0]
        if not len(hit):
            ys.append(np.full(n_seg, np.nan))
            rs.append(np.full(n_seg, np.nan))
            lags.append(None)
            continue
        s = lo + int(hit[0])
        ys.append(L[s - 64:s - 64 + n_seg])
        rs.append(R[s - 64:s - 64 + n_seg])
        lags.append(s - (T0 + int(i * period * RATE)))
    np.savez(prefix.with_suffix(".npz"), x=x, y=np.array(ys), r=np.array(rs),
             names=np.array(["bypass"] + [s["name"] for s in spec["steps"]]))
    wav.unlink()
    print(prefix.with_suffix(".npz"), "missing:", [i for i, l in enumerate(lags) if l is None])


if __name__ == "__main__":
    main()
