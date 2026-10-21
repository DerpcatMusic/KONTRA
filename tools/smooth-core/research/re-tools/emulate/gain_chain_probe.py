"""Entry and call-chain probe for the gain helper region (0x8e04c0..0x8e4be2).

Runs one emulated process block (48 kHz, stand-in state, same setup as
process_oracle.py) and records, for each probed RVA:
  - how many times it executes,
  - the return address on the stack at the first hit (a `call` leaves one; a `jmp` into
    a function does not, and shows as `-` with the instruction history instead),
  - the last few executed image PCs before the first hit.

Used for section 19 of ../../re-latency-fft-scale.md: the gain body is entered at
0x8e04c0 (not 0x8e0858, which is an interior label of the same function), four times per
block, from 0x8d9bf1, inside 0x8d98a0, called from 0x8d9520 inside 0x8d929e.

Evidence scope: isolated emulation, synthetic host, stand-in state. It shows the path this
state takes, not every path the plug-in can take.

Run with: python -I gain_chain_probe.py
"""

import collections
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from process_lib import PROCESS_THUNK, Session, add_probe, alloc_frames  # noqa: E402
from pe_emu import IMAGE_BASE, image_va  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RSP  # noqa: E402

N = 512
PROBES = [
    ("8d9230", 0x8D9230),
    ("8d929e", 0x8D929E),
    ("8d98a0", 0x8D98A0),
    ("8e04c0", 0x8E04C0),  # gain body entry (the 49-byte prologue)
    ("8e0858", 0x8E0858),  # interior label inside the body (magnitude calls follow it)
]


def main():
    sess = Session(48000)
    emu = sess.emu
    sess.prepare()
    ring = collections.deque(maxlen=12)
    emu.trace_insns(ring)
    hits = {}

    def make(name):
        def rec(e):
            ret = e.r64(e.reg(UC_X86_REG_RSP)) - IMAGE_BASE
            tail = [hex(a - IMAGE_BASE) for a in ring][-4:]
            hits.setdefault(name, []).append((hex(ret) if 0 < ret < e.image_size else "-", tail))
            return 0

        return rec

    for name, rva in PROBES:
        add_probe(emu, image_va(rva), name, make(name), [])

    tin, _ = alloc_frames(emu, 2, N, [[math.sin(0.05 * i) * 0.5 for i in range(N)]] * 2)
    tout, _ = alloc_frames(emu, 2, N, [[0.0] * N, [0.0] * N])
    emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, N, max_insns=400_000_000)
    for name, _rva in PROBES:
        seen = hits.get(name, [])
        first_ret, tail = seen[0] if seen else ("-", [])
        print(f"{name}: hits={len(seen)} first_return={first_ret} tail={' '.join(tail)}")


if __name__ == "__main__":
    main()
