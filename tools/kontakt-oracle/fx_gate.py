#!/usr/bin/env python3
"""The effect gate: a model's render against the oracle's, per 1/3-octave band and per 10 ms frame.

    fx_gate.py DIR/NAME ...     prints max and rms dB error of each measure (NAME.model.f32 against NAME.oracle.f32)

The oracle plays each instance's note from a live MIDI port, so its render starts up to ~90 ms off the reference's
timeline (Kontakt 8.13.1 under Wine, 2026-11-14): the oracle is first moved onto the model's timeline by the lag (within 150 ms) that best matches the two (`lag`). A delay's own time is checked apart from this gate (its echo against
the dry signal in the same render).

Band error: the level of each 1/3-octave band (25 Hz .. 16 kHz) over the whole signal. Frame error: the
RMS of each 10 ms frame, frames both renders hold above -70 dB re the oracle's peak; `settled_*` leaves out the first
30 ms of sound (the rig sets knobs from the note's callback, so those frames hear the knobs land). Both are model minus oracle, dB.
Bands holding less than -60 dB re the loudest band of the oracle are not compared (noise floor).
"""
import sys
from pathlib import Path

import numpy as np

ONSET_FRAMES = 3  # the first 30 ms of sound, left out of the settled frame measure
CENTRES = [25 * 2 ** (k / 3) for k in range(0, 29)]  # 25 Hz .. 16 kHz


def bands(x, rate):
    spec = np.abs(np.fft.rfft(x * np.hanning(len(x)))) ** 2
    fr = np.fft.rfftfreq(len(x), 1 / rate)
    out = []
    for c in CENTRES:
        m = (fr >= c / 2 ** (1 / 6)) & (fr < c * 2 ** (1 / 6))
        out.append(10 * np.log10(spec[m].sum() + 1e-30))
    return np.array(out)


def gate(model, oracle, rate=48000, mask=None):
    """`mask`: one byte per 10 ms frame, 0 for frames left out of both measures (fx_export.py: oracle dropouts)."""
    n = min(len(model), len(oracle))
    model, oracle = model[:n].copy(), oracle[:n].copy()
    p = int(0.01 * rate)
    good = np.ones(n // p + 1, bool) if mask is None else np.resize(mask.astype(bool), n // p + 1)
    # Masked frames are faded out over 5 ms either side for the bands, so their edges add no splatter.
    keep = np.repeat(good.astype(float), p)[:n]
    fade = np.hanning(int(0.005 * rate) * 2 + 1)
    keep = np.minimum(keep, np.convolve(keep, fade / fade.sum(), "same"))
    bm, bo = bands(model * keep, rate), bands(oracle * keep, rate)
    for f in np.nonzero(~good)[0]:
        model[f * p:(f + 1) * p] = 0.0
        oracle[f * p:(f + 1) * p] = 0.0
    keep = bo > bo.max() - 60
    be = (bm - bo)[keep]
    peak = np.abs(oracle).max() + 1e-30
    fe, settled = [], []
    for i in range(0, n - p, p):
        if not good[i // p]:
            continue
        a, b = (np.sqrt(np.mean(v[i:i + p] ** 2)) / peak for v in (model, oracle))
        if b > 10 ** (-70 / 20) and a > 10 ** (-70 / 20):
            fe.append(20 * np.log10(a / b))
            # The knobs are set from the note's callback, so the first frames of a render hear them land.
            if len(fe) > ONSET_FRAMES:
                settled.append(fe[-1])
    fe = np.array(fe) if fe else np.zeros(1)
    settled = np.array(settled) if settled else np.zeros(1)
    return {"band_max": float(np.abs(be).max()), "band_rms": float(np.sqrt((be ** 2).mean())),
            "frame_max": float(np.abs(fe).max()), "frame_rms": float(np.sqrt((fe ** 2).mean())),
            "settled_max": float(np.abs(settled).max()), "settled_rms": float(np.sqrt((settled ** 2).mean())),
            "worst_band_hz": float(np.array(CENTRES)[keep][np.abs(be).argmax()]),
            "frames": len(fe), "masked": int((~good).sum())}


def xcorr(a, b, most):
    """The shift of `b` (within `most` samples) that best correlates it with `a`."""
    n = min(len(a), len(b))
    size = 1 << int(np.ceil(np.log2(2 * n)))
    c = np.fft.irfft(np.fft.rfft(a[:n], size) * np.conj(np.fft.rfft(b[:n], size)), size)
    near = np.concatenate([c[-most:], c[: most + 1]])
    return int(np.argmax(near)) - most


def frames_db(model, oracle, rate):
    """RMS dB error of the 10 ms frames both hold above -70 dB re the oracle's peak."""
    p = int(0.01 * rate)
    n = (min(len(model), len(oracle)) // p) * p
    a, b = (np.sqrt(np.mean(v[:n].reshape(-1, p) ** 2, axis=1)) for v in (model, oracle))
    peak = np.abs(oracle).max() + 1e-30
    keep = (a > peak * 10 ** (-70 / 20)) & (b > peak * 10 ** (-70 / 20))
    return float(np.sqrt(np.mean((20 * np.log10(a[keep] / b[keep])) ** 2))) if keep.any() else np.inf


def lag(model, oracle, rate, most=0.15):
    """The shift (samples) of `oracle` that best matches it to `model`, within `most` seconds. Two candidates: the
    waveforms' best correlation, and the best correlation of the rises of their 1 ms envelopes refined by the
    waveforms within 1 ms. A constant-level sweep has no envelope to place and a slow tone ramp's cycles all look
    alike, so the one whose frames agree best is kept."""
    ms = int(0.001 * rate)
    whole = xcorr(model, oracle, int(most * rate))
    env = [np.diff(np.sqrt(np.convolve(v * v, np.ones(ms) / ms, "same"))[::ms]) for v in (model, oracle)]
    coarse = xcorr(env[0], env[1], int(most * 1000)) * ms
    refined = coarse + xcorr(model, np.roll(oracle, coarse), ms)
    return min((whole, refined), key=lambda s: frames_db(model, np.roll(oracle, s), rate))


def load(name, rate=48000):
    """(model, oracle on the model's timeline, frame mask)."""
    m = np.fromfile(f"{name}.model.f32", dtype="<f4").astype(float)
    o = np.fromfile(f"{name}.oracle.f32", dtype="<f4").astype(float)
    shift = lag(m, o, rate)
    o = np.roll(o, shift)
    good = None
    for path, moved in ((Path(f"{name}.mask.u8"), shift), (Path(f"{name}.inmask.u8"), 0)):
        if path.exists():
            k = np.roll(np.fromfile(path, dtype=np.uint8), int(round(moved / (0.01 * rate))))
            good = k if good is None else good[: len(k)] & k[: len(good)]
    if good is not None:
        # The shift moves the mask by whole frames: widen each gap a frame either side.
        good = good & np.roll(good, 1) & np.roll(good, -1)
    return m, o, good


if __name__ == "__main__":
    for name in sys.argv[1:]:
        m, o, mask = load(name)
        print(Path(name).name, {k: round(v, 2) if isinstance(v, float) else v for k, v in gate(m, o, 48000, mask).items()})
