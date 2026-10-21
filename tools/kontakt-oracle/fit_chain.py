#!/usr/bin/env python3
"""Fit the shared amp chain (pre one-pole, input gain, shaper table, post one-pole, delay)
to one native Kontakt trace (stimrender.py .npz step), by linear least squares.

    fit_chain.py jump.npz STEP_INDEX OUT.json

For a fixed (input gain, pre high-pass, post low-pass, delay) the shaper table is linear in
its knot weights, so each candidate is one least-squares solve. Output: the best candidate
with its normalised RMS error, as JSON for the Rust preset.
"""
import json
import sys

import numpy as np

RATE = 48000
KNOTS = 17  # piecewise-linear table over [-1, 1], odd-symmetric by construction


def one_pole_lp(x, fc):
    """y[n] = y[n-1] + a (x[n] - y[n-1]), as a truncated impulse-response convolution."""
    a = 1.0 - np.exp(-2 * np.pi * fc / RATE)
    taps = int(np.ceil(np.log(1e-7) / np.log(1.0 - a))) + 1
    h = a * (1.0 - a) ** np.arange(taps)
    return np.convolve(x, h)[: len(x)]


def one_pole_hp(x, fc):
    return x - one_pole_lp(x, fc)


def basis(u):
    """Odd-symmetric hat basis on [-1, 1]: column k is the hat at knot k (mirrored)."""
    u = np.clip(u, -1.0, 1.0)
    centres = np.linspace(-1.0, 1.0, KNOTS)
    h = centres[1] - centres[0]
    cols = [np.maximum(0.0, 1.0 - np.abs(u - c) / h) for c in centres]
    return np.stack(cols, axis=1)


def design(x, pre_fc, post_fc, gain):
    """Basis columns after the input gain, pre high-pass, shaper hats and post low-pass."""
    u = gain * one_pole_hp(x, pre_fc) if pre_fc else gain * x
    phi = basis(u)
    return np.stack([one_pole_lp(phi[:, k], post_fc) for k in range(KNOTS)], axis=1)


def solve(phi_lp, y, delay):
    target = np.roll(y, -delay)
    w, *_ = np.linalg.lstsq(phi_lp, target, rcond=None)
    err = phi_lp @ w - target
    return w, np.sqrt(np.mean(err ** 2)) / np.sqrt(np.mean(target ** 2))


def main():
    d = np.load(sys.argv[1])
    step = int(sys.argv[2])
    x = d["x"]
    y = d["y"][step][: len(x)]
    best = None
    for pre_fc in (0.0, 80.0, 160.0):
        for post_fc in (3000.0, 5000.0, 8000.0, 12000.0, 16000.0):
            for gain in (1.0, 2.0, 4.0, 8.0, 16.0):
                phi_lp = design(x, pre_fc, post_fc, gain)
                for delay in range(0, 240, 4):
                    w, e = solve(phi_lp, y, delay)
                    if best is None or e < best[0]:
                        best = (e, pre_fc, post_fc, gain, delay, w)
    e, pre_fc, post_fc, gain, delay, w = best
    for dd in range(max(0, delay - 4), delay + 5):
        w2, e2 = solve(design(x, pre_fc, post_fc, gain), y, dd)
        if e2 < e:
            e, delay, w = e2, dd, w2
    out = {"pre_fc": pre_fc, "post_fc": post_fc, "gain": gain, "delay": delay,
           "table": [float(v) for v in w], "rel_rms": float(e)}
    json.dump(out, open(sys.argv[3], "w"), indent=1)
    print(json.dumps({k: v for k, v in out.items() if k != "table"}))


if __name__ == "__main__":
    main()
