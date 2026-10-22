"""Kolor (BEffectKolor, module 0x79) under the kemu emulator: construction, apply, prepare, control tick, block.

Status: the full native path runs for all 10 modes (nonzero output). This is the native oracle for a
port, not a port; wet output is verified only for modes 2 and 5. See docs/re/engine/k-effects-batch-a.md.

    python3 -I tools/re/kolor_kemu_probe.py KONTAKT8_EXE [OUT_JSON]

Call order as the host does it (measured, not guessed):
  1. BEffectKolor ctor 1407c7950 (holder = {ptr, size}); the allocator 140711a10 is intercepted.
  2. apply 140abae20 (BEffect vtable slot 29) with the reader record. Record layout (from the apply
     disassembly, 140abae20): floats at +0x08..+0x30 feed knobs 3,0,2,5,6,8,9,10,11,12,7; byte +0x34
     feeds knob 1; int mode at +0x38 feeds knob 4 as mode/9; knob 13 is the constant 0.5. Every knob
     goes through the wrapper setter 1409aaa90 (vtable slot 32). Knob i lives at core+0x5c+4*i.
     The setter's notify lambdas (wrapper +0x290+64*i) run the per-knob control code; knob 4 runs
     the per-mode seed 14197acc0.
  3. core prepare = core vtable slot 20, 141979410(this, rate f32); core = obj + 0x1c0.
  4. block: the wrapper driver 140900eb0 runs the control tick 14197ab90 when wrapper+0x1bc < 1 and
     then processes min(count, 32) samples with core slot 19, 14197a2c0(core, in_chans, out_chans, n).
     This harness does the same per 32 samples. Driver pre/post bus stages 140ac28f0 / 140ac21f0 are
     NOT replayed (they mix the bus into the buffers; not measured here).
  Knob 7 is the wet mix: it lives at core+0x78 (= core+0x5c+4*7), fed from reader float +0x30.
"""
import json
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RAX, UC_X86_REG_RDX, UC_X86_REG_RIP, UC_X86_REG_RSP  # noqa: E402

CTOR, APPLY, SETTER, TICK = 0x1407C7950, 0x140ABAE20, 0x1409AAA90, 0x14197AB90
PREPARE_SLOT, BLOCK_SLOT, ALLOC = 20, 19, 0x140711A10
CORE_OFF = 0x1C0
SEGMENT = 32  # driver control-tick period (wrapper +0x1bc)
RATE = 48000.0


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


# Knob id -> reader float index (record offset 0x08 + 4*k), measured from the apply disassembly.
RECORD_FLOAT = {3: 0, 0: 1, 2: 2, 5: 3, 6: 4, 8: 5, 9: 6, 10: 7, 11: 8, 12: 9, 7: 10}


def record(e, mode, knobs, mono=0):
    """Reader record for apply. `knobs` maps knob id (setter index) to a value; default 0.5, mono byte at +0x34."""
    rec = e.alloc(0x40)
    for knob, k in RECORD_FLOAT.items():
        e.write(rec + 0x08 + 4 * k, 'f', knobs.get(knob, 0.5))
    e.write(rec + 0x34, 'B', mono)
    e.write(rec + 0x38, 'i', mode)
    return rec


def build(e, mode=9, knobs=None, mono=0):
    """Construct, apply the record, prepare at 48 kHz. Returns the core pointer (obj + 0x1c0)."""
    holder = e.alloc(16)
    run(e, CTOR, holder)
    obj = e.read(holder, 'Q')
    core = obj + CORE_OFF
    run(e, APPLY, obj, record(e, mode, knobs or {}, mono))
    for idx, value in (knobs or {}).items():  # apply's unforced setter skips values equal to the cache
        set_knob(e, obj, idx, value)
    run(e, vslot(e, core, PREPARE_SLOT), core, f={1: RATE})
    return obj, core


def set_knob(e, obj, idx, value, force=1):
    """Host parameter change: the wrapper setter (vtable slot 32). The setter skips a write equal to its
    cached value unless param_4 (r9) is set, so `force` defaults on (apply passes 0)."""
    run(e, SETTER, obj, idx, 0, force, f={2: value})


def block(e, core, left, right):
    """Process `left`/`right` (floats) as the driver does: tick, then up to 32 samples per call."""
    out_l, out_r = [], []
    for start in range(0, len(left), SEGMENT):
        seg_l, seg_r = left[start:start + SEGMENT], right[start:start + SEGMENT]
        n = len(seg_l)
        run(e, TICK, core)
        inl, inr, outl, outr = (e.alloc(n * 4) for _ in range(4))
        chans_in, chans_out = e.alloc(16), e.alloc(16)
        e.write(chans_in, 'Q', inl, inr)
        e.write(chans_out, 'Q', outl, outr)
        e.write(inl, 'f', *seg_l)
        e.write(inr, 'f', *seg_r)
        run(e, vslot(e, core, BLOCK_SLOT), core, chans_in, chans_out, n)
        out_l += list(e.read(outl, 'f', n))
        out_r += list(e.read(outr, 'f', n))
    return out_l, out_r


def signal(n):
    left = [0.5 * math.sin(2 * math.pi * 220 * i / RATE) + 0.2 * math.sin(2 * math.pi * 3100 * i / RATE) for i in range(n)]
    right = [0.45 * math.sin(2 * math.pi * 330 * i / RATE + 0.3) for i in range(n)]
    return left, right


def main():
    exe = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else None
    cases = []
    for mode in range(10):
        e = make(exe)
        obj, core = build(e, mode=mode)
        assert e.read(core + 0x7DC, 'i') == mode, 'per-mode seed did not run'
        left, right = signal(512)
        steady_l, steady_r = block(e, core, left, right)
        e = make(exe)
        obj, core = build(e, mode=mode)
        # moving knob: knob 0 (core+0x5c) steps at each 128-sample boundary, as a host automation would
        moving_l, moving_r = [], []
        for seg in range(4):
            set_knob(e, obj, 0, 0.2 + 0.2 * seg)
            l, r = block(e, core, left[seg * 128:(seg + 1) * 128], right[seg * 128:(seg + 1) * 128])
            moving_l += l
            moving_r += r
        peak = max(map(abs, steady_l))
        print(f'mode {mode}: steady peak {peak:.6f}, moving peak {max(map(abs, moving_l)):.6f}')
        cases.append({'mode': mode, 'steady_L': steady_l, 'steady_R': steady_r,
                      'moving_L': moving_l, 'moving_R': moving_r})
    if out:
        with open(out, 'w') as fh:
            json.dump({'rate': RATE, 'block': SEGMENT, 'input': 'signal(512) in tools/re/kolor_kemu_probe.py',
                       'cases': cases}, fh)
        print('wrote', out)


if __name__ == '__main__':
    main()
