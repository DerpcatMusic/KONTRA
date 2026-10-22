"""Native vectors for the Kontakt 8 Replika head delay caller (`141abd290`, 569 B).

`141abd290(C)` sets the delay positions of the three heads (`C+0xac0`, `C+0x6380`, `C+0xbc40`) through the leaf
`141abd0b0` and sends parameter 1 to the limiter `C+0x13f80` (a virtual call, slot +0x60, value in xmm2). The spread
inputs come from the live source (`C+0x938` zero: the floats at `*(C+0xc8)+0x2c/0x30/0x34/0x38`) or from the stored
doubles (`C+0x938` nonzero: `C+0x14210`, `C+0x14260`, `C+0x14238`, `C+0x14288`). The bounds are `C+0xad8` (lower)
and `C+0xad0` (upper), the path is `C+0x604` and the flag is `C+0x600`. Paths 2 and 3 also call `141aca380` and
`141acb160`, which are not ported, so they are not vectored.

The limiter is a stub: its vtable slot points at a `ret` on the heap, and a code hook there reads xmm2.

    nice -n 19 python3 -I tools/re/replika_delay_caller_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_XMM2  # noqa: E402

CALLER = 0x141ABD290
LEAF_OFFSETS = (0xAC0, 0x6380, 0xBC40)
CORE_BYTES = 0x14400
HEAD_BYTES = 0x100
SEEDS = tuple(range(1, 13))
PATHS = (0, 1, 4)


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def head_init(e, c, off, rnd):
    p = c + off
    e.write(p + 0x08, "i", rnd.randrange(0, 200))
    e.write(p + 0x0C, "i", rnd.randrange(200, 800))
    e.write(p + 0x28, "i", rnd.randrange(1, 9))
    e.write(p + 0x48, "d", rnd.uniform(1.0, 100.0))
    e.write(p + 0x58, "d", rnd.uniform(0.0, 3.0))
    e.write(p + 0x60, "d", rnd.uniform(0.0, 3.0))
    e.write(p + 0xC8, "d", rnd.uniform(1.0, 500.0))
    e.write(p + 0xD0, "d", rnd.uniform(1.0, 500.0))
    e.write(p + 0x90, "B", 0)
    return {
        "lo": e.read(p + 0x08, "i"),
        "hi": e.read(p + 0x0C, "i"),
        "mode": e.read(p + 0x28, "i"),
        "base": e.read(p + 0x48, "d"),
        "r58": e.read(p + 0x58, "d"),
        "r60": e.read(p + 0x60, "d"),
        "a": e.read(p + 0xC8, "d"),
        "b": e.read(p + 0xD0, "d"),
        "flag": e.read(p + 0x90, "B"),
    }


def head_state(e, c, off):
    p = c + off
    return {
        "a": e.read(p + 0xC8, "d"),
        "b": e.read(p + 0xD0, "d"),
        "r58": e.read(p + 0x58, "d"),
        "r60": e.read(p + 0x60, "d"),
        "flag": e.read(p + 0x90, "B"),
    }


def case(e, seed, rnd, limiter_slot, limiter_log):
    c = e.alloc(CORE_BYTES)
    e.uc.mem_write(c, bytes(CORE_BYTES))
    live = e.alloc(0x100)
    e.write(c + 0xC8, "Q", live)
    src = rnd.randrange(2)
    e.write(c + 0x938, "B", src)
    if rnd.random() < 0.15:
        spread = rnd.uniform(0.0, 1e-5)  # the zero branch
    else:
        spread = rnd.uniform(0.0, 5000.0)
    skew = rnd.uniform(0.0, 1.0)
    depth = rnd.uniform(-1.0, 1.0)
    ratio = rnd.uniform(-1.0, 1.0)
    for off, v in ((0x2C, spread), (0x30, ratio), (0x34, skew), (0x38, depth)):
        e.write(live + off, "f", f32(v))
    e.write(c + 0x14210, "d", spread)
    e.write(c + 0x14260, "d", ratio)
    e.write(c + 0x14238, "d", skew)
    e.write(c + 0x14288, "d", depth)
    if src == 0:  # the live floats are the source: take their f32 values
        spread, ratio, skew, depth = (f32(spread), f32(ratio), f32(skew), f32(depth))
    lo = rnd.uniform(0.0, 100.0)
    hi = lo + rnd.uniform(0.5, 3000.0)
    e.write(c + 0xAD8, "d", lo)
    e.write(c + 0xAD0, "d", hi)
    ctl600 = rnd.randrange(5)
    path = rnd.choice(PATHS)
    e.write(c + 0x600, "i", ctl600)
    e.write(c + 0x604, "i", path)
    e.write(c + 0x610, "f", f32(rnd.uniform(44100.0, 96000.0)))
    vt = e.alloc(0x100)
    e.write(vt + 0x60, "Q", limiter_slot)
    e.write(c + 0x13F80, "Q", vt)
    heads = [head_init(e, c, off, rnd) for off in LEAF_OFFSETS]
    limiter_log.clear()
    e.call(CALLER, c)
    return {
        "seed": seed,
        "src": src,
        "spread": spread,
        "skew": skew,
        "depth": depth,
        "ratio": ratio,
        "bounds": [lo, hi],
        "ctl600": ctl600,
        "path": path,
        "heads_in": heads,
        "heads": [head_state(e, c, off) for off in LEAF_OFFSETS],
        "limiter": limiter_log[0] if limiter_log else None,
        "limiter_calls": len(limiter_log),
    }


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    ret_at = e.alloc(16)
    e.uc.mem_write(ret_at, b"\xc3")
    log = []

    def on_limiter(uc, addr, size, _):
        raw = uc.reg_read(UC_X86_REG_XMM2).to_bytes(16, "little")
        log.append(struct.unpack("<f", raw[:4])[0])

    e.uc.hook_add(UC_HOOK_CODE, on_limiter, begin=ret_at, end=ret_at)
    rnd_cases = [case(e, s, random.Random(s * 7919), ret_at, log) for s in SEEDS]
    doc = {"name": "replika-delay-caller", "units": "141abd290 (141abd0b0 leaf)", "cases": rnd_cases}
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    print("%d cases; limiter calls: %s" % (len(rnd_cases), sorted({c["limiter_calls"] for c in rnd_cases})))


if __name__ == "__main__":
    main()
