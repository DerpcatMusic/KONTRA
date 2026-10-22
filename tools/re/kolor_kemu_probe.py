"""Kolor (BEffectKolor, module 0x79) under the kemu emulator: construction, apply, prepare, mode seed, block.

Status: the dry path is native and correct (gain ramp, stereo copy); the wet path (shaper + biquad
chain) is silent in every mode, so this is NOT yet a vector oracle. See docs/re/engine/k-effects-batch-a.md.

    python3 -I tools/re/kolor_kemu_probe.py KONTAKT8_EXE

Call order as the host does it (from the block driver 140900eb0 and the apply 140abae20):
  1. BEffectKolor ctor 1407c7950 (holder = {ptr, size}); the allocator 140711a10 is intercepted.
  2. apply 140abae20 (vtable slot 29) with a host struct whose +0x38 is the block size.
  3. core prepare = core vtable slot 20, 141979410(this, rate f32); core = obj + 0x1c0.
  4. per-mode seed 14197acc0(core, mode), mode in 0..9 (stored at core+0x7dc).
  5. block = core vtable slot 19, 14197a2c0(core, in_chans, out_chans, count); in_chans/out_chans are
     arrays of two channel pointers. The driver passes the same array for both, so the kernel reads
     the dry sample from in[k] and writes out[k].
"""
import math
import struct
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RAX, UC_X86_REG_RDX, UC_X86_REG_RIP, UC_X86_REG_RSP  # noqa: E402

CTOR, APPLY, PREPARE_SLOT, SEED, BLOCK_SLOT, ALLOC = 0x1407C7950, 0x140ABAE20, 20, 0x14197ACC0, 19, 0x140711A10
CORE_OFF = 0x1C0
BLOCK = 32


def make(exe):
    e = Emu(exe)

    def alloc_hook(uc, addr, size, _):  # the game's allocator needs runtime globals; use the kemu heap
        n = uc.reg_read(UC_X86_REG_RDX)
        uc.reg_write(UC_X86_REG_RAX, e.alloc(n))
        sp = uc.reg_read(UC_X86_REG_RSP)
        uc.reg_write(UC_X86_REG_RIP, struct.unpack('<Q', bytes(uc.mem_read(sp, 8)))[0])
        uc.reg_write(UC_X86_REG_RSP, sp + 8)

    e.uc.hook_add(UC_HOOK_CODE, alloc_hook, begin=ALLOC, end=ALLOC)
    return e


def run(e, fn, *args, f=None):
    e.call(fn, *args, f=f, limit=50_000_000)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError('instruction budget exhausted')


def vslot(e, obj, slot):
    return e.read(e.read(obj, 'Q') + 8 * slot, 'Q')


def build(e, rate=48000.0, mode=9):
    holder = e.alloc(16)
    run(e, CTOR, holder)
    obj = e.read(holder, 'Q')
    core = obj + CORE_OFF
    host = e.alloc(0x100)
    e.write(host + 0x38, 'i', BLOCK)
    run(e, APPLY, obj, host)
    e.write(core + 0x515, 'B', 1)
    run(e, vslot(e, core, PREPARE_SLOT), core, f={1: rate})
    run(e, SEED, core, mode)
    return core


def block(e, core, left, right):
    """Process one block of `left`/`right` samples (floats); returns the two output lists."""
    n = len(left)
    inl, inr, outl, outr = (e.alloc(n * 4) for _ in range(4))
    chans_in, chans_out = e.alloc(16), e.alloc(16)
    e.write(chans_in, 'Q', inl, inr)
    e.write(chans_out, 'Q', outl, outr)
    e.write(inl, 'f', *left)
    e.write(inr, 'f', *right)
    run(e, vslot(e, core, BLOCK_SLOT), core, chans_in, chans_out, n)
    return list(e.read(outl, 'f', n)), list(e.read(outr, 'f', n))


def main():
    e = make(sys.argv[1])
    core = build(e)
    sig = [0.5 * math.sin(2 * math.pi * i / 16) for i in range(BLOCK)]
    e.write(core + 0x78, 'f', 0.0)
    left, right = block(e, core, sig, sig)
    print('dry path (wet=0): peak', max(map(abs, left)))
    e.write(core + 0x78, 'f', 1.0)
    left, right = block(e, core, sig, sig)
    print('wet path (wet=1): peak', max(map(abs, left)), '(silent = blocker, see batch-a doc)')


if __name__ == '__main__':
    main()
