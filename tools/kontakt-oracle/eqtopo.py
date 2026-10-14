#!/usr/bin/env python3
"""Solid G-EQ: fit RBJ sections to the dense eqrender runs and compare with the RE spline tables.

    eqtopo.py dlf dhf ...      (prefixes in /tmp/w3/audit: PREFIX.tsv + PREFIX.h.npy)   -> TSV on stdout
"""
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eqfit  # noqa: E402

# Core spline tables (RE: initializer 0x141a9a510, ~/.cache/kontakto-wave3/specs/effects.md "Solid EQ parameter splines").
T = {
    "LF_GAIN": [-20.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 20.5],
    "HF_FREQ": [1539, 1710, 2400, 3980, 5460, 6820, 9160, 13100, 17813, 21000, 21800],
    "HF_GAIN": [-20.0, -17.5, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 17.5, 20.0],
    "HF_GAIN@16": [-20.0, -16.8, -9.0, -4.5, -1.8, 0.0, 1.8, 4.5, 9.0, 16.8, 20.0],
    "HMF_FREQ": [692.48, 730.72, 1022.48, 1614.67, 2112.54, 2619.29, 3291.53, 4544.29, 5634.35, 6444.74, 6892.65],
    "HMF_Q": [0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.31, 0.26, 0.22, 0.22],
    "HMF_GAIN": [-21.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 21.5],
    "HMF_GAIN@16": [-21.5, -18.0, -9.2, -5.0, -1.8, 0.0, 1.8, 5.0, 9.2, 18.0, 21.5],
    "LMF_FREQ": [223, 239, 323, 494, 668, 809, 1050, 1479, 1841, 2083, 2200],
    "LMF_Q": [0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.3, 0.25, 0.21, 0.2],
    "LMF_GAIN": [-19.5, -16.0, -10.0, -5.3, -2.0, 0.0, 2.0, 5.3, 10.0, 16.0, 19.5],
    "LF_FREQ": [40.43, 44.64, 63.44, 101.36, 140.21, 183.73, 245.15, 351.55, 513.3, 614.68, 697.34],
    "HP_FREQ": [14.5, 16.0, 23.0, 38.0, 57.0, 74.0, 100.0, 162.0, 280.0, 380.0, 460.0],
    "LP_FREQ": [31000, 29000, 25200, 19400, 15500, 12300, 10500, 8100, 4800, 3450, 3300],
}


def spline(y, u):
    """Cubic spline, zero slope at the left end, zero curvature at the right end (RE evaluator 0x141a98340)."""
    x = np.arange(11) / 10
    h = np.diff(x)
    n = 11
    A = np.zeros((n, n))
    r = np.zeros(n)
    A[0, 0], A[0, 1] = 2 * h[0], h[0]
    r[0] = 6 * ((y[1] - y[0]) / h[0])
    for i in range(1, n - 1):
        A[i, i - 1], A[i, i], A[i, i + 1] = h[i - 1], 2 * (h[i - 1] + h[i]), h[i]
        r[i] = 6 * ((y[i + 1] - y[i]) / h[i] - (y[i] - y[i - 1]) / h[i - 1])
    A[-1, -1] = 1
    m = np.linalg.solve(A, r)
    i = min(int(u * 10), 9)
    a, b = (x[i + 1] - u) / h[i], (u - x[i]) / h[i]
    return a * y[i] + b * y[i + 1] + ((a ** 3 - a) * m[i] + (b ** 3 - b) * m[i + 1]) * h[i] ** 2 / 6


def load(prefix):
    rows = [l.split("\t") for l in open(f"/tmp/w3/audit/{prefix}.tsv").read().splitlines()[1:]]
    rows = [r for r in rows if r[2] == "LL"]
    return list(zip(rows, np.load(f"/tmp/w3/audit/{prefix}.h.npy")))


def main():
    print("run\tstep\tname\tkind\tfit_f0_hz\tfit_gain_db\tfit_q\tfit_rms_db\ttable_f0_hz\ttable_gain_db\ttable_gain_bank16_db\ttable_q")
    for prefix in sys.argv[1:]:
        for r, h in load(prefix):
            name = r[1]
            sec = name.split()[0]
            kv = dict(p.split("=") for p in name.split() if "=" in p)
            bell = "bell" in name
            kind = "peak" if sec in ("LMF", "HMF") or bell else ("low" if sec == "LF" else "high")
            u_f = int(kv["freq"]) / 1e6 if kv.get("freq", "mid") != "mid" else 0.5
            u_g = 1.0 if kv.get("gain") == "max" else int(kv["gain"]) / 1e6 if "gain" in kv else 0.5
            u_q = int(kv["q"]) / 1e6 if "q" in kv else 0.5
            if abs(u_g - 0.5) < 1e-9:
                continue  # flat
            f, g, q, e = eqfit.fit(h, kind)
            tf = spline(T[f"{sec}_FREQ"], u_f)
            tg = spline(T[f"{sec}_GAIN"], u_g)
            tg16 = spline(T.get(f"{sec}_GAIN@16", T[f"{sec}_GAIN"]), u_g)
            tq = spline(T[f"{sec}_Q"], u_q) if f"{sec}_Q" in T else float("nan")
            print(f"{prefix}\t{r[0]}\t{name}\t{kind}\t{f:.1f}\t{g:.2f}\t{q:.3f}\t{e:.3f}\t{tf:.1f}\t{tg:.2f}\t{tg16:.2f}\t{tq:.3f}", flush=True)


if __name__ == "__main__":
    main()
