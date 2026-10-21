"""Differential fixture for the parameter setter (0x894370, entered through thunk 0x88f4e0).

Runs the setParam thunk under Unicorn on one zeroed DSP object per sequence and records, for
each call, the return value (rax) and every byte the call changed. A sequence is 1..8 calls on
the same object, so later calls see state left by earlier ones (dirty flags, old values,
early-return paths). `reference::params` replays the same sequences and must match byte for byte.

Calling convention (from the disassembly, not the decompile):
  thunk 0x88f4e0:  rcx = wrapper; obj = [wrapper]; xmm2 = (double) value; jmp 0x894370
  0x894370:        rdx = descriptor: u32 index at +0, f64 min at +0x28, f64 max at +0x30,
                   function pointer at +0x50, called as f64 fn(desc, f64 x) -> f64 in xmm0.

The descriptor's mapping function is host-owned. The fixture uses the identity map, so the
clamp, select, round and case logic are tested without guessing the host's range law.

Output: tools/smooth-core/research/oracle/params_setter_fixture.tsv, one line per call:
  seq  index  value_bits  min_bits  max_bits  rax  changes
where changes is `off:hexbytes` runs joined by ',' (bytes the call wrote, new values, including zeros).
Run from this directory:  python -I setter_oracle.py [--seqs N] [--seed S]
"""

import argparse
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from unicorn.x86_const import UC_X86_REG_XMM0, UC_X86_REG_XMM1, UC_X86_REG_XMM2  # noqa: E402

from pe_emu import Emulator, image_va  # noqa: E402

THUNK = image_va(0x88F4E0)
OBJ_SIZE = 0xC1A8  # same object size the constructor harness allocates (fake_host.OBJ_SIZE)
OUT = Path(__file__).resolve().parent.parent.parent / "oracle" / "params_setter_fixture.tsv"

EDGE_VALUES = [0.0, -0.0, 1.0, 0.5, 0.49999997, 0.5000001, 0.999, 0.9999999, 1.0000001, 1e-30,
               -1e-9, 1.5, -2.0, float("inf"), float("-inf"), float("nan"), 0.25, 0.75]


def _xmm_raw(emu, reg):
    raw = emu.uc.reg_read(reg)
    if isinstance(raw, int):
        raw = raw.to_bytes(16, "little")
    return bytes(raw)


def _bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def _value(rng, pool):
    if pool and rng.random() < 0.5:
        return rng.choice(pool)
    if rng.random() < 0.3:
        return rng.choice(EDGE_VALUES)
    return rng.uniform(-0.25, 1.25)


def _range(rng):
    if rng.random() < 0.5:
        return 0.0, 1.0
    if rng.random() < 0.8:
        lo = rng.uniform(-3.0, 3.0)
        return lo, lo + rng.uniform(0.0, 4.0)
    return rng.uniform(0.5, 2.0), rng.uniform(-2.0, 0.4)  # reversed: min > max


def build(emu):
    obj = emu.arena(OBJ_SIZE)
    wrapper = emu.arena(0x58)
    emu.w64(wrapper, obj)

    def identity(emu_, ev):
        # movaps xmm0, xmm1: the host map returns its input (full 16 bytes).
        emu_.uc.reg_write(UC_X86_REG_XMM0, int.from_bytes(_xmm_raw(emu_, UC_X86_REG_XMM1), "little"))
        return None

    map_fn = emu.stub("descriptor::identity_map", identity)
    desc = emu.arena(0x60)
    return obj, wrapper, desc, map_fn


def call_once(emu, obj, wrapper, desc, map_fn, index, value, lo, hi):
    """One setParam call; returns (rax, changes) where changes is [(offset, new_bytes)]."""
    before = emu.rd(obj, OBJ_SIZE)
    emu.w32(desc, index)
    emu.wf64(desc + 0x28, lo)
    emu.wf64(desc + 0x30, hi)
    emu.w64(desc + 0x50, map_fn)
    emu.uc.reg_write(UC_X86_REG_XMM2, int.from_bytes(struct.pack("<d", value).ljust(16, b"\0"), "little"))
    rax = emu.call(THUNK, wrapper, desc, max_insns=200_000)
    after = emu.rd(obj, OBJ_SIZE)
    changes = []
    i = 0
    while i < OBJ_SIZE:
        if after[i] != before[i]:
            j = i
            while j < OBJ_SIZE and after[j] != before[j]:
                j += 1
            changes.append((i, after[i:j]))
            i = j
        else:
            i += 1
    return rax, changes


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seqs", type=int, default=1200)
    ap.add_argument("--seed", type=int, default=20261021)
    args = ap.parse_args()
    rng = random.Random(args.seed)
    emu = Emulator()
    obj, wrapper, desc, map_fn = build(emu)
    lines = []
    ncalls = 0
    for seq in range(args.seqs):
        emu.wr(obj, bytes(OBJ_SIZE))  # every sequence starts from the all-zero state
        # A small index pool per sequence makes repeats, and so the no-change early returns, likely.
        pool_size = rng.choice([1, 2, 3, 12, 40, 143])
        pool = [rng.randrange(0, 0x8F) for _ in range(pool_size)]
        vpool = [_value(rng, None) for _ in range(3)]
        for _ in range(rng.randint(1, 8)):
            if rng.random() < 0.95:
                index = rng.choice(pool)
            else:
                index = rng.randrange(0x8F, 0x100)  # out of range: the function returns early
            value = _value(rng, vpool)
            lo, hi = _range(rng)
            rax, changes = call_once(emu, obj, wrapper, desc, map_fn, index, value, lo, hi)
            runs = ",".join(f"{off}:{data.hex()}" for off, data in changes)
            lines.append(f"{seq}\t{index}\t{_bits(value):016x}\t{_bits(lo):016x}\t{_bits(hi):016x}\t{rax:x}\t{runs}")
            ncalls += 1
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("# soothe3 0x88f4e0 -> 0x894370 differential fixture (setter_oracle.py, identity map)\n"
                   + "\n".join(lines) + "\n")
    print(f"wrote {ncalls} calls in {args.seqs} sequences to {OUT}")


if __name__ == "__main__":
    main()
