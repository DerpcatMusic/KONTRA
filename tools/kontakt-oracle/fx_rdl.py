#!/usr/bin/env python3
"""Replika Delay measurements from fx_lab.py renders (Kontakt 8.13.1, factory `Effects/RDelay`, 2026-11-14).

    fx_rdl.py echoes IMPULSE.npz     per instance: every echo's time and level against the dry impulse
    fx_rdl.py sweep  SWEEP.npz       per instance: level against frequency of the delayed sweep, dB vs the bypassed one
    fx_rdl.py clicks CLICKS.npz DELAY_MS   per instance: delay swing (ms, peak to peak) and rate (Hz) from the clicks'
                                     echo arrival times (the echo of a click every 25 ms, TIME DELAY_MS)

Instance 0 is the bypassed reference. Settings are in the matching .json (see docs/re/engine/fx-measured/replika.md).
"""
import sys

import numpy as np

PROBES = [50, 100, 200, 500, 1000, 2000, 4000, 8000, 12000, 16000]


def load(path):
    z = np.load(path)
    return int(z["rate"]), z


def echoes(x, rate, thr_db=-40.0, gap_ms=8.0, limit=10):
    """(ms from the first sound, peak) of each cluster of samples above thr_db re the loudest."""
    a = np.abs(x)
    top = a.max()
    t0 = int(np.argmax(a > 0.5 * top)) if top > 0 else 0
    out, i = [], 0
    while i < len(a) and len(out) < limit:
        if a[i] > top * 10 ** (thr_db / 20):
            j = i + int(0.003 * rate)
            m = i + int(np.argmax(a[i:j]))
            out.append((m / rate * 1000, a[m]))
            i = m + int(gap_ms / 1000 * rate)
        else:
            i += 1
    return out


def sweep_response(ref, x, rate, delay_ms, probes=PROBES, sweep_s=5.0):
    """dB of x against ref along the echo's trajectory of the log sweep 20 Hz..20 kHz: at probe f the sweep passes at
    t_f in ref and at t_f + delay in x; each is the peak of a 60 ms-windowed spectrum near f."""
    k = np.log(1000.0) / sweep_s
    start = int(np.argmax(np.abs(ref) > 1e-4))
    win = int(0.06 * rate)
    def level(v, t, f):
        i = start + int(t * rate) - win // 2
        seg = v[i:i + win] * np.hanning(win)
        spec = np.abs(np.fft.rfft(seg, 8 * win))
        fr = np.fft.rfftfreq(8 * win, 1 / rate)
        m = (fr > f * 0.9) & (fr < f * 1.1)
        return 20 * np.log10(spec[m].max() + 1e-12)
    out = []
    for f in probes:
        t = np.log(f / 20.0) / k
        out.append(level(x, t + delay_ms / 1000, f) - level(ref, t, f))
    return out


def click_delays(ref, x, rate, delay_ms, period_ms=25.0, skip=20):
    """The echo's delay (ms) at each click: the peak within half a period of the click + delay_ms."""
    n = min(len(x), len(ref))
    a = np.abs(x[:n] - ref[:n])  # the dry clicks sit on the 25 ms grid the echo also falls on; drop them
    start = int(np.argmax(np.abs(ref) > 0.2))
    p = int(period_ms / 1000 * rate)
    out = []
    for i in range(skip, int(4.5 * rate / p)):
        c = start + i * p + int(delay_ms / 1000 * rate)
        lo = c - p // 2
        j = lo + int(np.argmax(a[lo:lo + p]))
        out.append((j - (start + i * p)) / rate * 1000)
    return np.array(out)


def delay_track(x, rate, f0=1000.0, a=0.6, b=4.6):
    """Delay (ms) against time of a steady tone from its analytic phase."""
    i = int(np.argmax(np.abs(x) > 1e-4))
    seg = x[i + int(a * rate): i + int(b * rate)]
    n = len(seg)
    fr = np.fft.fftfreq(n, 1 / rate)
    h = np.zeros(n)
    h[0], h[1:n // 2], h[n // 2] = 1, 2, 1
    an = np.fft.ifft(np.fft.fft(seg) * h * ((np.abs(fr) > 0.7 * f0) & (np.abs(fr) < 1.3 * f0)))
    ph = np.unwrap(np.angle(an)) - 2 * np.pi * f0 * np.arange(n) / rate
    tau = -(ph - ph.mean()) / (2 * np.pi * f0) * 1000.0
    return tau[3000:-3000]


def mod_stats(tau, rate):
    t = tau - tau.mean()
    spec = np.abs(np.fft.rfft(t * np.hanning(len(t))))
    fr = np.fft.rfftfreq(len(t), 1 / rate)
    m = (fr > 0.1) & (fr < 40)
    return float(tau.max() - tau.min()), float(fr[m][np.argmax(spec[m])])


if __name__ == "__main__":
    mode, path = sys.argv[1:3]  # modes: echoes, sweep PATH DELAY_MS, clicks PATH DELAY_MS
    rate, z = load(path)
    n = len([k for k in z.files if k.startswith("x")])
    for k in range(1, n):
        if mode == "echoes":
            ref = z["x0"]
            r0 = np.abs(ref).max()
            t0 = int(np.argmax(np.abs(ref) > 0.5 * r0)) / rate * 1000
            print(k, " ".join(f"{t - t0:.1f}ms/{20 * np.log10(v / r0):.1f}dB" for t, v in echoes(z[f"x{k}"], rate)))
        elif mode == "sweep":
            r = sweep_response(z["x0"], z[f"x{k}"], rate, float(sys.argv[3]))
            print(k, " ".join(f"{f}:{v:+.1f}" for f, v in zip(PROBES, r)))
        elif mode == "clicks":
            d = click_delays(z["x0"], z[f"x{k}"], rate, float(sys.argv[3]))
            spec = np.abs(np.fft.rfft(d - d.mean()))
            fr = np.fft.rfftfreq(len(d), 0.025)
            print(k, f"delay mean {d.mean():.2f} swing {d.max() - d.min():.2f} ms  rate {fr[1:][np.argmax(spec[1:])]:.2f} Hz")
