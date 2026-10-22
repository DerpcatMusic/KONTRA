"""WORK IN PROGRESS: native Chainsaw harness via kemu (not yet producing a vector).

Verified: effect vtable 0x144f15e68 slot 28 (offset 0xe0) = apply 0x140aba230 (reads the record,
sets 4 knobs, mono byte); prepare (wrapper vtable 0x144f15d58 slot 2 = 0x140970bb0) takes the sample
rate as a float in xmm1 and must run after the two cores at +0x200/+0x1340 are constructed; process
= wrapper slot 23 = 0x1408fe290 faults at 0x140ac2610 (ring buffer pointer null, the factory's
allocator path not yet emulated).

Original notes: ctor (0x1407c6e40 body), apply (vtable slot 28 = 0x140aba230),
prepare (wrapper slot 2 = 0x140970bb0), process (wrapper slot 23 = 0x1408fe290)."""
import sys, math, struct
import os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu
EXE = "/data/dev/workspace/re-binaries/Kontakt8.exe"  # a copy of Kontakt 8.13.1 (kemu needs the PE)
TEB = 0x7D0000000000
W = 0x144f15d58
C = 0x144f15e68
BLOCK = 32

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_RSP, UC_X86_REG_RAX, UC_X86_REG_RDX, UC_X86_REG_RCX, UC_X86_REG_RIP


def hook_alloc(e, fn, size_reg):
    """Internal engine allocators (not imports) return zeroed kemu heap memory."""
    def cb(uc, addr, size, _):
        n = uc.reg_read(size_reg)
        p = e.alloc(max(int(n), 16))
        uc.reg_write(UC_X86_REG_RAX, p)
        sp = uc.reg_read(UC_X86_REG_RSP)
        ret = struct.unpack('<Q', bytes(uc.mem_read(sp, 8)))[0]
        uc.reg_write(UC_X86_REG_RSP, sp + 8)
        uc.reg_write(UC_X86_REG_RIP, ret)
    e.uc.hook_add(UC_HOOK_CODE, cb, begin=fn, end=fn)


def make(distortion, low, high, level, mono=0, rate=48000):
    e = Emu(EXE)
    hook_alloc(e, 0x1443c9194, UC_X86_REG_RCX)
    idx = e.read(0x141951f97 + 6 + 0x94655f3, 'I')
    tls = e.alloc(8 * (idx + 2)); blk = e.alloc(0x1000)
    e.write(tls + 8 * idx, 'Q', blk); e.write(TEB + 0x58, 'Q', tls)
    obj = e.alloc(0x2400)
    e.call(0x140a856c0, obj)
    e.write(obj + 0x38, 'B', 1)
    e.call(0x14196fd00, obj + 0x40); e.call(0x14099a0d0, obj + 0x40)
    e.write(obj, 'Q', W)
    e.call(0x14196fd00, obj + 0x268)
    e.write(obj + 0x450, 'B', 0)
    e.call(0x14099a0d0, obj + 0x268); e.call(0x14099a0d0, obj + 0x40)
    e.write(obj, 'Q', C)
    rec = e.alloc(0x20)
    e.write(rec + 8, 'f', distortion, low, high, level); e.write(rec + 0x18, 'B', mono)
    # knobs via the effect's apply (vtable slot 28)
    e.write(obj + 0x120, 'I', BLOCK); e.write(obj + 0x124, 'I', 2)
    for off in (0x1340, 0x200):   # cores built by the factory's sub-object constructors
        e.call(0x14196fd00, obj + off); e.call(0x14099a0d0, obj + off)
    e.call(0x140970bb0, obj, f={1: float(rate)})  # prepare: builds the cores, sets the prepared flag
    e.call(0x140aba230, obj, rec)   # apply: knobs
    ctx = e.alloc(0xA0000)
    bufs = [ctx + 0x9c010 + c * 0x1000 for c in range(2)]
    for c in range(2):
        e.write(obj + 0x20 + 8 * c, 'Q', bufs[c])
    return e, obj, ctx, bufs

def run(x, distortion, low, high, level, mono=0):
    e, obj, ctx, bufs = make(distortion, low, high, level, mono)
    out = []
    for start in range(0, len(x), BLOCK):
        blk = x[start:start + BLOCK]
        blk = blk + [0.0] * (BLOCK - len(blk))
        for c in range(2):
            e.write(bufs[c], 'f', *blk)
        e.call(0x1408fe290, obj, ctx, 0)
        out += e.read(bufs[0], 'f', BLOCK)
    return out[:len(x)]

if __name__ == "__main__":
    n = 192
    x = [0.4 * math.sin(2 * math.pi * 200 * i / 48000) for i in range(n)]
    for k in [(0.0, 0.5, 0.5, 0.5), (1.0, 0.5, 0.5, 0.5)]:
        y = run(x, *k)
        print(k, "max|y|=%.5f" % max(abs(v) for v in y), [round(v, 4) for v in y[60:68]])
