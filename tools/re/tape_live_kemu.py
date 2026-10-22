"""Native vectors for the Tape Saturator (0x42) live setter, run from Kontakt 8.13.1 with kemu.

    python3 tools/re/tape_live_kemu.py

The setter `FUN_140ae1e10(obj, param, value)` takes the stored parameter index: 1 is
warmth. It writes `obj+0x3c = 10^(0.05 * (6w - 3))` (the same law `tape.rs` uses at init)
and the ramp target `obj+0x2240`, then raises the slew flag `obj+0x2250` and state
`obj+0x2260`. Prints the native gain and target at fixed warmth values, to be compared
with `Tape::warmth` in `crates/sampler-core/src/dsp/color/tape.rs`.
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu, f32  # noqa: E402

EXE = os.environ.get('KONTAKT8_EXE', '/data/dev/workspace/re-binaries/Kontakt8.exe')
SETTER, WARMTH = 0x140AE1E10, 1
WARMTH_AT = 0x3C
TARGET_AT = 0x2240
FLAG_AT = 0x2250
STATE_AT = 0x2260


def main():
    emu = Emu(EXE)
    obj = emu.alloc(0x2300)
    for warmth in (0.0, 0.25, 0.5, 1.0):
        emu.call(SETTER, obj, WARMTH, 0, f={2: warmth})
        gain = emu.read(obj + WARMTH_AT, 'f')
        target = emu.read(obj + TARGET_AT, 'f')
        flag = emu.read(obj + FLAG_AT, 'B')
        state = emu.read(obj + STATE_AT, 'I')
        want = f32(math.pow(10.0, 0.05 * (warmth * 6.0 - 3.0)))
        print(f'warmth={warmth} gain={gain!r} target={target!r} flag={flag} state={state} want={want!r}')


if __name__ == '__main__':
    main()
