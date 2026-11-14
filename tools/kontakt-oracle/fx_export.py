#!/usr/bin/env python3
"""Cuts oracle renders into gate cases: fx_export.py DIR NPZ EFFECT PREFIX I:K=V,K=V [I:K=V ...]

Writes DIR/<PREFIX><i>.in.f32 (the bypassed instance, the effect's input) and .oracle.f32 (instance i) from 0.1 s
before the reference's first sound, and appends `<PREFIX><i>|EFFECT|k=v,...` to DIR/cases.txt. Each instance comes
as an argument `i:k=v,k=v` (the settings that model needs; run `cargo test -p kontra-format-kontakt fit -- --ignored`
with KONTRA_FIT=DIR, then tools/kontakt-oracle/fx_gate.py DIR/<name>). A delay renders wet only
(`$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL` 0).

The oracle drops audio now and then (runs of exact zeros). A run of a frame (10 ms) or more inside an instance's
sound is a capture gap: the sound carries on after it where it left off (Kontakt 8.13.1, 2026-11-14: instance 1 of
the DStortion sweeps, 0.37 s), so it is cut out. For the shorter runs left, `.mask.u8` marks, one byte per 10 ms frame of the
instance, 0 where it dropped out; `.inmask.u8` likewise for the reference, on whose timeline the model plays, with
the HOLD after each dropout masked too (a delay hears it that much later).
"""
import sys
from pathlib import Path

import numpy as np

FRAME = 480
HOLD = 0.6
RUN = 32


def dropouts(x):
    """Frames (index) holding a run of RUN exact zeros between the signal's first and last sound."""
    loud = np.nonzero(np.abs(x) > 1e-6)[0]
    if not len(loud):
        return np.zeros(0, int)
    z = x == 0.0
    z[: loud[0]] = False
    z[loud[-1]:] = False
    c = np.convolve(z.astype(np.int32), np.ones(RUN, np.int32), "same") >= RUN
    return np.unique(np.nonzero(c)[0] // FRAME)


def close_gaps(x):
    """`x` without its runs of FRAME or more exact zeros between its first and last sound."""
    loud = np.nonzero(np.abs(x) > 1e-6)[0]
    if not len(loud):
        return x
    keep = np.ones(len(x), bool)
    zero = np.concatenate([[False], x[loud[0]:loud[-1]] == 0.0, [False]])
    edges = np.flatnonzero(np.diff(zero.astype(np.int8)))
    for start, end in zip(edges[::2], edges[1::2]):
        if end - start >= FRAME:
            keep[loud[0] + start:loud[0] + end] = False
    return x[keep]


def mask(x, hold_frames):
    m = np.ones(len(x) // FRAME + 1, np.uint8)
    for f in dropouts(x):
        m[f:f + hold_frames + 1] = 0
    return m


if __name__ == "__main__":
    out, npz, effect, prefix = Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
    z = np.load(npz)
    rate = int(z["rate"])
    ref = z["x0"]
    i0 = max(int(np.argmax(np.abs(ref) > 1e-5)) - int(0.1 * rate), 0)
    out.mkdir(parents=True, exist_ok=True)
    with open(out / "cases.txt", "a") as cases:
        for arg in sys.argv[5:]:
            i, keys = arg.split(":", 1)
            name = f"{prefix}{i}"
            ref[i0:].astype("<f4").tofile(out / f"{name}.in.f32")
            sound = close_gaps(z[f"x{i}"][i0:])
            sound.astype("<f4").tofile(out / f"{name}.oracle.f32")
            mask(ref[i0:], int(HOLD * rate / FRAME)).tofile(out / f"{name}.inmask.u8")
            mask(sound, 0).tofile(out / f"{name}.mask.u8")
            cases.write(f"{name}|{effect}|{keys}\n")
