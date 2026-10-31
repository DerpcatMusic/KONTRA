#!/usr/bin/env python3
"""Hot Solo's measured tone stage: the curve, and the band fit `ir::Amp` carries.

    tools/dsp-research/hot_solo_fit.py

No arguments: the measured curve below is Hot Solo's own response at Electric
Mint's saved record, taken from two Kontakt-versus-KONTRA pairs (see
docs/re/engine/k-hotsolo.md) and reproducible from the captures with
tools/oracle/compare.py's band_levels. Printing the fit is how the four band
constants in crates/format/kontakt/src/hot_solo.rs are audited.

Needs numpy and scipy (research tooling; the engine does not).
"""
import numpy as np
from scipy.optimize import least_squares

RATE = 48_000

# 1/3-octave centres and the measured difference (Kontakt − KONTRA without Hot
# Solo), dB. Median of the curve is +9.08 dB; the stage's own gain carries it.
MEASURED = [
    (40, 0.00), (50, 3.23), (63, 4.92), (80, 5.52), (101, 5.58), (127, 3.21),
    (160, 5.79), (202, 3.93), (254, 4.40), (320, 5.50), (403, 5.49), (508, 7.70),
    (640, 8.64), (806, 9.08), (1016, 11.87), (1280, 13.83), (1613, 16.45),
    (2032, 17.44), (2560, 18.40), (3225, 19.44), (4064, 20.15), (5120, 20.66),
    (6451, 20.93), (8127, 19.46), (10240, 17.85), (12902, 15.55),
]


def biquad_db(f, kind, f0, q, gain_db):
    """RBJ's cookbook band sampled at `f`; a shelf's `q` is its slope as the
    engine's `q` is (`sin(w)/2·√2` at slope 1)."""
    w0 = 2 * np.pi * f0 / RATE
    a = 10 ** (gain_db / 40)
    alpha = np.sin(w0) / (2 * q)
    z = np.exp(-1j * 2 * np.pi * f / RATE)
    if kind == "peak":
        b = [1 + alpha * a, -2 * np.cos(w0), 1 - alpha * a]
        d = [1 + alpha / a, -2 * np.cos(w0), 1 - alpha / a]
    else:
        s = 2 * np.sqrt(a) * alpha
        if kind == "highshelf":
            b = [a * ((a + 1) + (a - 1) * np.cos(w0) + s), -2 * a * ((a - 1) + (a + 1) * np.cos(w0)),
                 a * ((a + 1) + (a - 1) * np.cos(w0) - s)]
            d = [(a + 1) - (a - 1) * np.cos(w0) + s, 2 * ((a - 1) - (a + 1) * np.cos(w0)),
                 (a + 1) - (a - 1) * np.cos(w0) - s]
        else:
            b = [a * ((a + 1) - (a - 1) * np.cos(w0) + s), 2 * a * ((a - 1) - (a + 1) * np.cos(w0)),
                 a * ((a + 1) - (a - 1) * np.cos(w0) - s)]
            d = [(a + 1) + (a - 1) * np.cos(w0) + s, -2 * ((a - 1) + (a + 1) * np.cos(w0)),
                 (a + 1) + (a - 1) * np.cos(w0) - s]
    return 20 * np.log10(np.abs((b[0] + b[1] * z + b[2] * z**2) / (d[0] + d[1] * z + d[2] * z**2)))


def model(p, f):
    """Low shelf (40 Hz fixed: the fit sits on its bound), a broad peak, a high
    shelf, and a constant gain — the four numbers hot_solo.rs carries."""
    f_ls, g_ls, f_pk, q_pk, g_pk, f_hs, g_hs, g_out = p
    return (biquad_db(f, "lowshelf", f_ls, 0.707, g_ls) + biquad_db(f, "peak", f_pk, q_pk, g_pk)
            + biquad_db(f, "highshelf", f_hs, 0.707, g_hs) + g_out)


def main() -> int:
    centres = np.array([hz for hz, _ in MEASURED], dtype=float)
    target = np.array([db for _, db in MEASURED])
    lo = [40, -12, 500, 0.2, -20, 3000, -20, -12]
    hi = [400, 12, 12000, 6.0, 30, 16000, 30, 20]
    best = None
    for trial in range(80):
        rng = np.random.default_rng(trial)
        start = [rng.uniform(a, b) for a, b in zip(lo, hi)]
        res = least_squares(lambda p: model(p, centres) - target, start, bounds=(lo, hi),
                            max_nfev=6000)
        best = res if best is None or res.cost < best.cost else best
    p = best.x
    names = ["low shelf hz", "low shelf dB", "peak hz", "peak Q", "peak dB", "high shelf hz",
             "high shelf dB", "gain dB"]
    for name, value in zip(names, p):
        print(f"  {name:14} {value:9.3f}")
    err = model(p, centres) - target
    print(f"residual: RMS {np.sqrt(np.mean(err ** 2)):.2f} dB, worst {np.abs(err).max():.2f} dB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
