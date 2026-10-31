#!/usr/bin/env python3
"""Choral (0x5d) voice bank: the impulse response of each mode.

    python3 tools/re/choral_modes.py [--frames N] [--mode K]

The four mode tables are not stored data: `MODES` at VA 0x14193cd70 in Kontakt
8.13.1 is inside `.text`, so the bank is built by code. Rather than read that
code, this measures what it produces: an impulse through the emulated kernel at
the lowest modulation the parameters allow, one run per mode, and every
non-zero frame of the result. A tap is where one voice's read pointer sat
during that run.

Lowest is not still. `amount = 0` still leaves a nonzero depth (357 samples at
rate 0.5, 48 kHz; `modulation_depth` in `crates/dsp/delay/src/chorus.rs`), and
`rate = 0` still runs the LFO at 0.25 Hz, so every tap is a modulated position
rather than a fixed bank delay: read a run as a spread, and compare modes to
each other instead of counting taps as a table. The left/right values say
nothing about the bank's `pan` field either -- `pan` is the per-voice LFO phase
offset, and the stereo rotation comes from `width`.

Needs KONTAKT_EXE pointing at a Kontakt 8.13.1 binary (the emulator is bound to
that build's addresses) and `unicorn`.
"""
import argparse
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import kemu_timemod  # noqa: E402

# amount, rate, delay, voices, width, feedback, mix -- the lowest modulation the
# parameters allow. Not zero modulation: amount 0 still has depth and rate 0 is
# a 0.25 Hz LFO, so a voice wanders around its delay instead of sitting on it.
STILL = [0.0, 0.0, 0.5, 1.0, 1.0, 0.0, 1.0]


def impulse(n: int) -> list[tuple[float, float]]:
    return [(1.0, 0.0)] + [(0.0, 0.0)] * (n - 1)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--frames", type=int, default=2400)
    ap.add_argument("--mode", type=int, action="append", choices=range(4))
    ap.add_argument("--threshold", type=float, default=1e-4)
    args = ap.parse_args()
    if not os.environ.get("KONTAKT_EXE"):
        sys.exit("set KONTAKT_EXE to a Kontakt 8.13.1 binary")
    for mode in args.mode if args.mode is not None else range(4):
        out = kemu_timemod.choral(STILL, False, False, mode, impulse(args.frames))
        taps = [
            (i, l, r)
            for i, (l, r) in enumerate(out)
            if abs(l) > args.threshold or abs(r) > args.threshold
        ]
        print(f"mode {mode}: {len(taps)} non-zero frames")
        for i, l, r in taps:
            print(f"  f{i:5d}  L {l:+.6f}  R {r:+.6f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
