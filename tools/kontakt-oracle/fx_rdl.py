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


def wet_track(ref, x, rate, f0=1000.0, a=0.4, b=4.4):
    """(delay ms, instantaneous frequency Hz, amplitude) of the wet part of a steady f0 tone through a modulated
    delay. The dry path is the steady f0 line: it is cut from the spectrum (bins within 0.3 Hz of f0), what is left is
    the wet signal, whose analytic phase against an ideal f0 tone gives the delay, delay(t) = -dphi / (2 pi f0) (the
    constant part meaningless, mod 1/f0), and whose phase slope gives the pitch the modulation draws."""
    i0 = int(np.argmax(np.abs(ref) > 1e-4))
    seg = x[i0 + int(a * rate): i0 + int(b * rate)]
    n = len(seg)
    fr = np.fft.fftfreq(n, 1 / rate)
    keep = (np.abs(fr) > f0 - 120) & (np.abs(fr) < f0 + 120) & (np.abs(np.abs(fr) - f0) > 0.3)
    h = np.zeros(n)
    h[0], h[1:n // 2], h[n // 2] = 1, 2, 1
    an = np.fft.ifft(np.fft.fft(seg * np.hanning(n)) * h * keep)
    ph = np.unwrap(np.angle(an)) - 2 * np.pi * f0 * np.arange(n) / rate
    tau = -(ph - ph.mean()) / (2 * np.pi * f0) * 1000.0
    inst = f0 + np.gradient(ph) * rate / (2 * np.pi)
    c = slice(n // 8, -n // 8)
    return tau[c], inst[c], np.abs(an)[c], (a + np.arange(n) / rate)[c]


def lfo_start(tau, t, f):
    """Seconds of LFO run the engine's delay LFO (delay +A sin(2 pi f t): it grows first) needs before t = 0 to match
    tau(t); the caller gives t on its own clock."""
    m = np.stack([np.sin(2 * np.pi * f * t), np.cos(2 * np.pi * f * t), np.ones_like(t)], 1)
    a, b, _ = np.linalg.lstsq(m, tau, rcond=None)[0]
    return (np.arctan2(b, a) / (2 * np.pi * f)) % (1 / f)


def mod_stats(tau, rate):
    """(peak-to-peak ms, dominant rate Hz) of a delay track."""
    t = tau - tau.mean()
    spec = np.abs(np.fft.rfft(t * np.hanning(len(t))))
    fr = np.fft.rfftfreq(len(t), 1 / rate)
    m = (fr > 0.05) & (fr < 30)
    return float(tau.max() - tau.min()), float(fr[m][np.argmax(spec[m])])


def energy_by_ms(ref, x, rate, win_ms=10.0, floor_db=-90.0):
    """(ms, dB re the dry impulse) of every 10 ms frame of x - ref that is above floor_db: where the effect's energy sits."""
    n0 = min(len(x), len(ref))
    w = x[:n0] - ref[:n0]
    t0 = int(np.argmax(np.abs(ref) > 0.5 * np.abs(ref).max()))
    p = int(win_ms / 1000 * rate)
    top = np.abs(ref).max()
    out = []
    for j in range(0, n0 - t0 - p, p):
        e = 10 * np.log10(np.sum(w[t0 + j:t0 + j + p] ** 2) / top ** 2 + 1e-30)
        if e > floor_db:
            out.append((j / rate * 1000, e))
    return out


if __name__ == "__main__":
    mode, path = sys.argv[1:3]  # modes: echoes, mod, energy, sweep PATH DELAY_MS, clicks PATH DELAY_MS
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
        elif mode == "mod":
            tau, inst, amp, _ = wet_track(z["x0"], z[f"x{k}"], rate)
            ptp, f = mod_stats(tau, rate)
            lo, hi = np.percentile(inst, [2, 98])
            print(k, f"delay swing {ptp:.2f} ms  rate {f:.2f} Hz  pitch {lo - 1000:+.1f}..{hi - 1000:+.1f} Hz  wet amp {amp.mean():.3f}")
        elif mode == "energy":
            e = energy_by_ms(z["x0"], z[f"x{k}"], rate)
            tot = 10 * np.log10(sum(10 ** (v / 10) for _, v in e) + 1e-30)
            print(k, f"total {tot:.1f} dB;", " ".join(f"{t:.0f}:{v:.0f}" for t, v in e[:40]))
        elif mode == "clicks":
            d = click_delays(z["x0"], z[f"x{k}"], rate, float(sys.argv[3]))
            spec = np.abs(np.fft.rfft(d - d.mean()))
            fr = np.fft.rfftfreq(len(d), 0.025)
            print(k, f"delay mean {d.mean():.2f} swing {d.max() - d.min():.2f} ms  rate {fr[1:][np.argmax(spec[1:])]:.2f} Hz")
