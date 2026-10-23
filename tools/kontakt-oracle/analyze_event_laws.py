#!/usr/bin/env python3
"""Measure event_bridge_laws.lua's recording (render.py output, 48 kHz float32 stereo).

    analyze_event_laws.py RECORDING.wav

Tick k (1-based) plays one note per second, so tick k's window starts at the first onset
plus (k - 1) seconds. Each tick is compared against tick 1 (plain note). Prints one line per
tick: pitch offset in semitones, level in dB relative to tick 1, L/R level in dB, and the
fade measurements for ticks 11 (fade_in) and 13 (fade_out).
"""
import sys

import numpy as np

SR = 48000
FRAME = SR // 100  # 10 ms envelope frames


def load(path):
    data = np.fromfile(path, dtype="<f4")
    return data.reshape(-1, 2)


def envelope(mono):
    n = len(mono) // FRAME
    return np.sqrt((mono[: n * FRAME].reshape(n, FRAME) ** 2).mean(axis=1))


def db(x):
    return 20 * np.log10(max(x, 1e-12))


def frequency(mono):
    # Zero-crossing estimate over the steady window; the window is long enough (0.6 s)
    # for better than 0.5 % resolution.
    signs = np.signbit(mono)
    rising = np.count_nonzero(~signs[:-1] & signs[1:])
    return rising * SR / len(mono)


def main():
    stereo = load(sys.argv[1])
    mono = stereo.mean(axis=1)
    env = envelope(mono)
    onset = int(np.argmax(env > 0.1 * env.max())) * FRAME
    rows = {}
    base_level = None
    for tick in range(1, 14):
        start = onset + (tick - 1) * SR
        hold = 0.9 if tick == 12 else 0.7
        win = slice(start + int(0.05 * SR), start + int(hold * SR))
        seg = mono[win]
        seg_st = stereo[win]
        if len(seg) == 0:
            continue
        level = np.sqrt((seg ** 2).mean())
        f = frequency(seg[int(0.05 * SR) :])
        st = 12 * np.log2(f / 440.0) if f > 0 else float("nan")
        lr = db(np.sqrt((seg_st[:, 0] ** 2).mean())) - db(np.sqrt((seg_st[:, 1] ** 2).mean()))
        if tick == 1:
            base_level = level
            base_db = db(level)
        rows[tick] = (st, db(level) - base_db, lr)
        print(f"tick {tick:2d}: pitch {st:+.3f} st  level {db(level) - base_db:+.2f} dB  L-R {lr:+.2f} dB")

    # fade_in on tick 11: time from onset until the envelope reaches 90 % of its steady level.
    s11 = onset + 10 * SR
    e = env[s11 // FRAME : (s11 + int(0.8 * SR)) // FRAME]
    steady = e[int(0.6 * 100) : int(0.7 * 100)].mean()
    reach = int(np.argmax(e >= 0.9 * steady)) * 10
    print(f"fade_in 0.5 s (tick 11): envelope reaches 90 % of steady at {reach} ms")

    # fade_out on tick 13 of the held note from tick 12: time from the fade to -20 dB and -40 dB.
    s13 = onset + 12 * SR
    e = env[s13 // FRAME : (s13 + int(1.0 * SR)) // FRAME]
    before = env[(s13 - SR // 10) // FRAME : s13 // FRAME].mean()
    for label, ratio in (("-20 dB", 0.1), ("-40 dB", 0.01)):
        idx = np.nonzero(e <= ratio * before)[0]
        ms = int(idx[0]) * 10 if len(idx) else -1
        print(f"fade_out 0.5 s (tick 13): envelope {label} at {ms} ms")
    return 0


if __name__ == "__main__":
    sys.exit(main())
