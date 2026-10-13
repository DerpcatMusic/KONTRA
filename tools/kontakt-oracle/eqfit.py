#!/usr/bin/env python3
"""Fit RBJ-cookbook peaking / shelf biquads to measured dB curves (eqrender.py *.h.npy).

    eqfit.py CURVES.h.npy STEPNAME_TSV           -> one row per LL step: fitted f0, gain_dB, Q, rms error

Peaking: RBJ with design gain G dB and Q.  Shelf: RBJ low/high shelf with S=1 equivalent (fits Q).
The fit uses 40 Hz..18 kHz (the linear-phase artefacts of the 16384-point FFT are below that).
"""
import csv
import sys

import numpy as np

RATE = 48000
N = 16384
FREQS = np.fft.rfftfreq(N, 1 / RATE)
SEL = (FREQS > 40) & (FREQS < 18000)
W = 2 * np.pi * FREQS[SEL] / RATE


def biquad_db(b, a):
    z = np.exp(-1j * W)
    num = b[0] + b[1] * z + b[2] * z * z
    den = a[0] + a[1] * z + a[2] * z * z
    return 20 * np.log10(np.abs(num / den))


def peak(f0, g, q):
    A = 10 ** (g / 40)
    w0 = 2 * np.pi * f0 / RATE
    al = np.sin(w0) / (2 * q)
    c = np.cos(w0)
    return biquad_db([1 + al * A, -2 * c, 1 - al * A], [1 + al / A, -2 * c, 1 - al / A])


def shelf(kind, f0, g, q):
    A = 10 ** (g / 40)
    w0 = 2 * np.pi * f0 / RATE
    al = np.sin(w0) / (2 * q)
    c = np.cos(w0)
    r = 2 * np.sqrt(A) * al
    if kind == "low":
        b = [A * ((A + 1) - (A - 1) * c + r), 2 * A * ((A - 1) - (A + 1) * c), A * ((A + 1) - (A - 1) * c - r)]
        a = [(A + 1) + (A - 1) * c + r, -2 * ((A - 1) + (A + 1) * c), (A + 1) + (A - 1) * c - r]
    else:
        b = [A * ((A + 1) + (A - 1) * c + r), -2 * A * ((A - 1) + (A + 1) * c), A * ((A + 1) + (A - 1) * c - r)]
        a = [(A + 1) - (A - 1) * c + r, 2 * ((A - 1) - (A + 1) * c), (A + 1) - (A - 1) * c - r]
    return biquad_db(b, a)


def fit(curve, kind):
    """Coarse-to-fine search over (f0, gain, Q); returns (f0, gain, Q, rms)."""
    y = curve[SEL]
    pk = y[np.argmax(np.abs(y))]
    model = (lambda f, g, q: peak(f, g, q)) if kind == "peak" else (lambda f, g, q: shelf(kind, f, g, q))
    best = (1e9, 0, 0, 0)
    f_grid = np.geomspace(30, 17000, 90)
    g_grid = pk * np.linspace(0.7, 1.4, 15)
    q_grid = np.geomspace(0.15, 6, 40)
    for f in f_grid:
        for g in g_grid:
            for q in q_grid:
                e = np.sqrt(np.mean((model(f, g, q) - y) ** 2))
                if e < best[0]:
                    best = (e, f, g, q)
    e, f, g, q = best
    step = [0.1, 0.05, 0.1]
    for _ in range(40):
        moved = False
        for i in range(3):
            for s in (1 + step[i], 1 / (1 + step[i])):
                c = [f, g, q]
                c[i] *= s
                ee = np.sqrt(np.mean((model(*c) - y) ** 2))
                if ee < e:
                    e, f, g, q, moved = ee, *c, True
        if not moved:
            step = [x / 2 for x in step]
    return f, g, q, e


def main():
    h = np.load(sys.argv[1])
    names = [r for r in csv.reader(open(sys.argv[2]), delimiter="\t")][1:]
    names = [r for r in names if r[2] == "LL"]
    kinds = {}
    for s in sys.argv[3:]:   # NAME_SUBSTRING=kind
        k, v = s.split("=")
        kinds[k] = v
    print("step\tname\tf0_hz\tgain_db\tq\trms_db")
    for i, r in enumerate(names):
        kind = next((v for k, v in kinds.items() if k in r[1]), "peak")
        f, g, q, e = fit(h[i], kind)
        print(f"{r[0]}\t{r[1]}\t{f:.1f}\t{g:.2f}\t{q:.3f}\t{e:.3f}\t{kind}")


if __name__ == "__main__":
    main()
