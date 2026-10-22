"""Native vectors for the Tape Saturator slew tick (Kontakt 8.13.1, run under kemu).

    python3 tools/re/tape_slew_kemu.py

`FUN_140ae1b40(obj)` is the per-tick step of the four-lane slew (group B: current
`+0x2220`, increment `+0x2230`, target `+0x2240`, flag `+0x2250`, sticky `+0x2251`,
counter `+0x2254` reloaded from `+0x2258`, rate `+0x225c`, state `+0x2260`). Group A
(`+0x21d0..`) is parked (counter huge, flag 0) so only group B moves. Each tick's
result (current, increment, counter, flag, sticky) is printed for the Rust lane in
`crates/sampler-core/src/dsp/color/tape_slew.rs`. Per-sample lane advance (`cur += inc`)
happens in the process loop `FUN_140ae1190` and is not run here.
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

EXE = os.environ.get('KONTAKT8_EXE', '/data/dev/workspace/re-binaries/Kontakt8.exe')
TICK = 0x140AE1B40
CUR, INC, TGT = 0x2220, 0x2230, 0x2240
FLAG, STICKY, CNT, RELOAD, RATE, STATE = 0x2250, 0x2251, 0x2254, 0x2258, 0x225C, 0x2260
A_CNT, A_FLAG = 0x2204, 0x2200


def run(emu, obj, cur, tgt, reload, sr, ticks):
    emu.write(obj + CUR, 'f', *cur)
    emu.write(obj + INC, 'f', 0.0, 0.0, 0.0, 0.0)
    emu.write(obj + TGT, 'f', *tgt)
    emu.write(obj + FLAG, 'B', 1)
    emu.write(obj + STICKY, 'B', 0)
    emu.write(obj + CNT, 'i', 0)
    emu.write(obj + RELOAD, 'i', reload)
    emu.write(obj + RATE, 'f', 1.0 / (reload * sr))
    emu.write(obj + STATE, 'I', 1)
    emu.write(obj + A_CNT, 'i', 1 << 30)
    emu.write(obj + A_FLAG, 'B', 0)
    for n in range(ticks):
        emu.call(TICK, obj)
        cur_v = emu.read(obj + CUR, 'f', 4)
        inc_v = emu.read(obj + INC, 'f', 4)
        cnt = emu.read(obj + CNT, 'i')
        flag = emu.read(obj + FLAG, 'B')
        sticky = emu.read(obj + STICKY, 'B')
        print(f'  tick {n + 1}: cur={list(cur_v)} inc={list(inc_v)} cnt={cnt} flag={flag} sticky={sticky}')


def main():
    emu = Emu(EXE)
    obj = emu.alloc(0x2300)
    sr = 48000
    print('case A: reload 1, lanes ramp to 1.0 from 0.0 (flag set, one-tick hold)')
    run(emu, obj, [0.0, 0.0, 0.0, 0.0], [1.0, 0.5, -0.25, 2.0], 1, sr, 4)
    print('case B: reload 3, lanes ramp from 0.75 to 1.41253757')
    run(emu, obj, [0.75] * 4, [1.41253757] * 4, 3, sr, 6)


if __name__ == '__main__':
    main()
