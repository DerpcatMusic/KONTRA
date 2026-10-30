#!/usr/bin/env python3
"""Choral (0x5d) voice bank: the impulse response of each mode.

    python3 tools/re/choral_modes.py [--frames N] [--mode K]

The four mode tables are not stored data: `MODES` at VA 0x14193cd70 in Kontakt
8.13.1 is inside `.text`, so the bank is built by code. Rather than read that
code, this measures what it produces: an impulse through the emulated kernel
with the modulation held still, one run per mode, and every non-zero frame of
the result. The taps it prints are the voice delays, their left/right signs the
pans.

Needs KONTAKT_EXE pointing at a Kontakt 8.13.1 binary (the emulator is bound to
that build's addresses) and `unicorn`.
"""
import argparse
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import kemu_timemod  # noqa: E402

# amount, rate, delay, voices, width, feedback, mix -- modulation held still so
# each voice stays at its own delay for the whole window.
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
