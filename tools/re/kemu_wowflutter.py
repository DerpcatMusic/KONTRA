"""Wow/Flutter (Kontakt 0x6A) run as the native wrapper runs it, under kemu.

    python3 tools/re/kemu_wowflutter.py [EXE]

Replays the real sequence: wrapper ctor 0x140a842b0 (kernel at wrapper+0x1c0, vtable
WowFlutter::vftable), setSampleRate 0x140971930 (kernel prepare 0x14099aac0), then the
per-block process 0x14099f000 -> virtual slot 19 0x14199d080. Nothing is poked except
the raw inputs the native code reads.
"""
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kemu  # noqa: E402
from kemu import Emu, f32  # noqa: E402

EXE = sys.argv[1] if len(sys.argv) > 1 else '/data/dev/workspace/re-binaries/Kontakt8.exe'
CTOR, SET_RATE, PROCESS = 0x140A842B0, 0x140971930, 0x14099F000
KERNEL = 0x1C0
SIZE = 0x10000


def boot(fs=48000.0, block=32):
    e = Emu(EXE)
    wrap = e.alloc(SIZE)
    cfg = e.alloc(16)
    e.write(cfg, 'i', 0)
    e.call(CTOR, wrap, cfg)
    e.call(SET_RATE, wrap, d={1: fs})
    return e, wrap


def run(e, wrap, x, block=32):
    """Process `x` (list of (l, r) f32 frames) in `block`-frame calls; returns (l, r) lists."""
    n = len(x)
    ins = [e.alloc(4 * n) for _ in range(2)]
    outs = [e.alloc(4 * n) for _ in range(2)]
    for c in range(2):
        e.write(ins[c], 'f', *[f[c] for f in x])
    ip, op = e.alloc(16), e.alloc(16)
    for at in range(0, n, block):
        m = min(block, n - at)
        e.write(ip, 'Q', ins[0] + 4 * at, ins[1] + 4 * at)
        e.write(op, 'Q', outs[0] + 4 * at, outs[1] + 4 * at)
        e.call(PROCESS, wrap + KERNEL, ip, op, m)
    return [e.read(outs[c], 'f', n) for c in range(2)]


if __name__ == '__main__':
    e, wrap = boot()
    impulse = [(1.0 if i == 0 else 0.0, 0.0) for i in range(2000)]
    l, r = run(e, wrap, impulse)
    print('impulse peak frames', [(i, round(v, 4)) for i, v in enumerate(l) if abs(v) > 1e-4][:10])
