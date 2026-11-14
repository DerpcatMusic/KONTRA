#!/usr/bin/env python3
"""The effect gate: a model's render against the oracle's, per 1/3-octave band and per 10 ms frame.

    fx_gate.py MODEL.f32 ORACLE.f32 [RATE]     prints max and rms dB error of each measure

Band error: the level of each 1/3-octave band (25 Hz .. 16 kHz) over the whole signal. Frame error: the
RMS of each 10 ms frame, frames both renders hold above -70 dB re the oracle's peak. Both are model minus oracle, dB.
Bands holding less than -60 dB re the loudest band of the oracle are not compared (noise floor).
"""
import sys

import numpy as np

CENTRES = [25 * 2 ** (k / 3) for k in range(0, 30)]  # 25 Hz .. 16 kHz


def bands(x, rate):
    spec = np.abs(np.fft.rfft(x * np.hanning(len(x)))) ** 2
    fr = np.fft.rfftfreq(len(x), 1 / rate)
    out = []
    for c in CENTRES:
        m = (fr >= c / 2 ** (1 / 6)) & (fr < c * 2 ** (1 / 6))
        out.append(10 * np.log10(spec[m].sum() + 1e-30))
    return np.array(out)


def gate(model, oracle, rate=48000):
    n = min(len(model), len(oracle))
    model, oracle = model[:n], oracle[:n]
    bm, bo = bands(model, rate), bands(oracle, rate)
    keep = bo > bo.max() - 60
    be = (bm - bo)[keep]
    p = int(0.01 * rate)
    peak = np.abs(oracle).max() + 1e-30
    fe = []
    for i in range(0, n - p, p):
        a, b = (np.sqrt(np.mean(v[i:i + p] ** 2)) / peak for v in (model, oracle))
        if b > 10 ** (-70 / 20) and a > 10 ** (-70 / 20):
            fe.append(20 * np.log10(a / b))
    fe = np.array(fe) if fe else np.zeros(1)
    return {"band_max": float(np.abs(be).max()), "band_rms": float(np.sqrt((be ** 2).mean())),
            "frame_max": float(np.abs(fe).max()), "frame_rms": float(np.sqrt((fe ** 2).mean())),
            "worst_band_hz": float(np.array(CENTRES)[keep][np.abs(be).argmax()])}


if __name__ == "__main__":
    m = np.fromfile(sys.argv[1], dtype="<f4").astype(float)
    o = np.fromfile(sys.argv[2], dtype="<f4").astype(float)
    print(gate(m, o, int(sys.argv[3]) if len(sys.argv) > 3 else 48000))
