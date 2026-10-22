"""Chainsaw kernel probe: call HM2 (`hm2::dsp::effektrig::HM2`) slot 19 directly under kemu.

Status (2026-10-23, pass 4): BLOCKED, not a vector. The knob setter 1409a9ec0 takes its value in
xmm2 (float), so the knobs are passed with f={2: v}. After that fix the gain at +0x40 is 0.5 (set
directly by the apply's else branch through +0x34[i]); the dirty flag +0x215 is NOT the gain gate.
The output stays exactly zero because the per-sample solver chain reads a coefficient block at
+0x520..+0x5cc that the prepare order never fills. See docs/re/engine/k-effects-batch-a.md, pass 4.

    python3 -I tools/re/chainsaw_hm2_kemu.py

Sequence (the wrapper's order for the core at +0x1340, from 140970bb0 and 1409a9ec0):
  ctor 14196fd00; slot 5 (14099c970); slot 3 (14099a700, rate, 32); slot 6 (1419709e0);
  slot 20 (141970b50, rate); knobs via 1409a9ec0 (index 0..3); slot 19 (141971820) on a 32-frame impulse.
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RAX, UC_X86_REG_RCX, UC_X86_REG_RIP, UC_X86_REG_RSP  # noqa: E402

EXE = "/data/dev/workspace/re-binaries/Kontakt8.exe"
TEB = 0x7D0000000000
CTOR, KNOB, PROCESS = 0x14196FD00, 0x1409A9EC0, 0x141971820
TLS_INDEX_SITE, HEAP_ALLOC = 0x141951F97 + 6 + 0x94655F3, 0x1443C9194
BLOCK = 32


def hook_alloc(e):
    """Engine allocator (not an import): return zeroed kemu heap memory."""

    def cb(uc, addr, size, _):
        p = e.alloc(max(int(uc.reg_read(UC_X86_REG_RCX)), 16))
        uc.reg_write(UC_X86_REG_RAX, p)
        sp = uc.reg_read(UC_X86_REG_RSP)
        uc.reg_write(UC_X86_REG_RIP, struct.unpack("<Q", bytes(uc.mem_read(sp, 8)))[0])
        uc.reg_write(UC_X86_REG_RSP, sp + 8)

    e.uc.hook_add(UC_HOOK_CODE, cb, begin=HEAP_ALLOC, end=HEAP_ALLOC)


def main():
    e = Emu(EXE)
    hook_alloc(e)
    idx = e.read(TLS_INDEX_SITE, "I")
    tls, blk = e.alloc(8 * (idx + 2)), e.alloc(0x1000)
    e.write(tls + 8 * idx, "Q", blk)
    e.write(TEB + 0x58, "Q", tls)

    obj = e.alloc(0x2000)
    e.call(CTOR, obj)
    vt = e.read(obj, "Q")
    slot = lambda n: e.read(vt + 8 * n, "Q")  # noqa: E731
    rate = 48000.0
    e.call(slot(5), obj)
    e.call(slot(3), obj, 0, BLOCK, f={1: rate})
    e.call(slot(6), obj)
    e.call(slot(20), obj, f={1: rate})
    for i in range(4):
        e.call(KNOB, obj, i, f={2: 0.5})  # value is float in xmm2

    ins, outs = e.alloc(16), e.alloc(16)
    chans = [e.alloc(4 * BLOCK) for _ in range(2)]
    for c in range(2):
        e.write(ins + 8 * c, "Q", chans[c])
        e.write(outs + 8 * c, "Q", e.alloc(4 * BLOCK))
    for c in range(2):
        e.write(chans[c], "f", 1.0, *([0.0] * (BLOCK - 1)))
    e.call(PROCESS, obj, ins, outs, BLOCK)
    out = e.read(e.read(outs, "Q"), "f", 8)
    print("out L", out)
    print("gain +0x40 =", e.read(obj + 0x40, "f"))


if __name__ == "__main__":
    main()
