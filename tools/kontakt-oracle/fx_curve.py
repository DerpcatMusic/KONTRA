#!/usr/bin/env python3
"""A static curve from a ramp render: fx_curve.py NPZ:I [NPZ:I ...]

The `ramp` stimulus (fx_lab.py) is a 1 kHz sine rising to full scale; the bypassed instance x0 is the effect's input.
For each two-cycle window the input's peak `a` and the instance's highest and lowest values are the curve's values at
+a and -a less the curve's mean over that cycle (a DC blocker follows the curve). Both are linear in the curve's
values at 2K+1 points over -1..1, so one least-squares solve (a light second-difference penalty, f(0) = 0) gives the
curve. The instance is placed against the input by the rises of their 1 ms envelopes; capture gaps are closed first
(fx_export.close_gaps). Prints the curve every fourth point and writes curve_<npz stem>_<I>.npy.
"""
import sys
from pathlib import Path

import numpy as np

from fx_export import close_gaps

RATE = 48000
K = 48
WINDOW = 96
KNOTS = np.linspace(-1, 1, 2 * K + 1)
MAX_LAG = 500  # ms: a delay's echo (TIME) plus the oracle's start jitter
THETA = np.linspace(0, 2 * np.pi, 256, endpoint=False)


def hat(x):
    """Linear-interpolation weights of `x` on KNOTS, one row per value."""
    x = np.atleast_1d(x)
    t = (np.clip(x, -1, 1) + 1) * K
    i = np.minimum(t.astype(int), 2 * K - 1)
    f = t - i
    b = np.zeros((len(x), 2 * K + 1))
    r = np.arange(len(x))
    b[r, i] = 1 - f
    b[r, i + 1] = f
    return b


def envelope(v):
    return np.sqrt(np.convolve(v * v, np.ones(48) / 48, "same"))[::48]


def peaks(npz, k):
    z = np.load(npz)
    x = z["x0"].astype(float)
    y = close_gaps(z[f"x{k}"].astype(float))
    ex, ey = envelope(x), envelope(y)
    n = min(len(ex), len(ey))
    dx, dy = np.diff(ex[:n]), np.diff(ey[:n])
    c = [np.dot(dx[max(0, -l):n - 1 - max(0, l)], dy[max(0, l):n - 1 - max(0, -l)]) for l in range(-MAX_LAG, MAX_LAG + 1)]
    lag = (int(np.argmax(c)) - MAX_LAG) * 48
    on = int(np.argmax(np.abs(x) > 1e-4))
    rows = []
    for s in range(on + int(0.25 * RATE), on + int(4.95 * RATE), WINDOW):
        a = np.abs(x[s:s + WINDOW]).max()
        w = y[s + lag:s + lag + WINDOW]
        if len(w) == WINDOW and np.count_nonzero(w == 0) <= 4:
            rows.append((a, w.max(), w.min()))
    return np.array(rows)


def fit(rows):
    a_rows, t = [], []
    for a, hi, lo in rows:
        mean = hat(a * np.sin(THETA)).mean(0)
        a_rows += [hat(a)[0] - mean, hat(-a)[0] - mean]
        t += [hi, lo]
    a_rows, t = np.array(a_rows), np.array(t)
    keep = [j for j in range(2 * K + 1) if j != K]
    smooth = np.diff(np.eye(2 * K + 1), 2, axis=0)[:, keep] * 0.02
    v, *_ = np.linalg.lstsq(np.vstack([a_rows[:, keep], smooth]), np.concatenate([t, np.zeros(len(smooth))]), rcond=None)
    curve = np.zeros(2 * K + 1)
    curve[keep] = v
    err = a_rows @ curve - t
    return curve, np.sqrt(np.mean(err ** 2)), np.abs(err).max()


if __name__ == "__main__":
    for spec in sys.argv[1:]:
        npz, k = spec.rsplit(":", 1)
        curve, rms, most = fit(peaks(npz, int(k)))
        print(f"{spec} error rms {rms:.4f} max {most:.4f}")
        print("  ", " ".join(f"{x:+.2f}:{y:+.3f}" for x, y in zip(KNOTS[::4], curve[::4])))
        np.save(f"curve_{Path(npz).stem}_{k}.npy", curve)
