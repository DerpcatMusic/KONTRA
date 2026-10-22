"""Native vectors for the Kontakt Stereo Tune core (BEffectStereoTune, vtable 0x144fc2868), under kemu.

    python3 -I tools/re/stereotune_kemu_vectors.py KONTAKT8_EXE OUT_JSON

Call order follows the host: ctor FUN_140a86e50, prepare = slot 1 FUN_140a9cae0 (init, slot-5 reset,
window table, ring allocation, rate fields; `rate` in xmm1), apply = slot 29 FUN_140abbce0 (four
knobs at param+8..0x14), then the driver FUN_140aa7320 without its post stage: the 32-sample countdown
at +0x1bc drives the boundary smoother FUN_140acb6c0 and the core FUN_140aaf640 is called per chunk
(the post stage FUN_140ac21f0 is the shared host mix into the output, not part of the core).
Core outputs are read from the scratch that the core writes (L at ctx+0x9c010, R at ctx+0x9d010, f32).
Inputs are read from the object's channel pointers (+0x20 L, +0x28 R).

The +0x1c0 rate scale is not written by the host: the ctor FUN_140a86e50 stores 0x40c90fdb (2*pi, f32) at
+0x1c0 and nothing in the reset, prepare or apply path overwrites it. The harness keeps the ctor value, so the
one-pole coefficient is (900*a+100)*2*pi/rate.
"""
import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

CTOR, PREPARE, APPLY, SMOOTH, CORE = 0x140A86E50, 0x140A9CAE0, 0x140ABBCE0, 0x140ACB6C0, 0x140AAF640
BLOCK = 32
BLOCKS = 8
N = BLOCKS * BLOCK
RATE = 48000.0
# byte offsets: (pos + 0x27004) * 4 for L; R is the second stride (0x400 floats) further on
CTX_L, CTX_R = 0x9C010, 0x9D010


def complete(e, *args, **kwargs):
    e.call(*args, limit=8_000_000, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


def run_case(exe_emu, knobs, stim, rate):
    e = exe_emu
    obj = e.alloc(0x4000)
    ctx = e.alloc(0xA4000)
    inL, inR = e.alloc(4 * N), e.alloc(4 * N)
    e.write(inL, "f", *stim[0])
    e.write(inR, "f", *stim[1])
    complete(e, CTOR, obj)
    assert abs(e.read(obj + 0x1C0, "f") - 2 * math.pi) < 1e-6, "ctor must leave the native 2*pi rate scale at +0x1c0"
    complete(e, PREPARE, obj, 0, f={1: rate})
    e.write(obj + 0x20, "Q", inL)
    e.write(obj + 0x28, "Q", inR)
    params = e.alloc(0x40)
    e.write(params + 8, "f", *knobs[:3], knobs[3])
    complete(e, APPLY, obj, params)
    # driver countdown (+0x1bc): smoother at each block boundary, core per chunk
    pos, remaining = 0, N
    while remaining > 0:
        c = e.read(obj + 0x1BC, "i")
        k = min(c, remaining)
        e.write(obj + 0x1BC, "i", c - k)
        if c - k < 1:
            complete(e, SMOOTH, obj + 0x1E0)
            e.write(obj + 0x1BC, "i", 0x20)
        if k > 0:
            complete(e, CORE, obj, ctx, pos, k)
        pos += k
        remaining -= k
    left = e.read(ctx + CTX_L, "f", N)
    right = e.read(ctx + CTX_R, "f", N)
    return list(left), list(right)


def main():
    exe, dst = sys.argv[1:3]
    rng = random.Random(7)
    stim = [[rng.uniform(-0.3, 0.3) for _ in range(N)], [rng.uniform(-0.3, 0.3) for _ in range(N)]]
    cases = [
        ("mid", [0.5, 0.5, 0.5, 0.5], RATE),
        ("low_knobs", [0.1, 0.2, 0.3, 0.25], RATE),
        ("high_knobs", [0.9, 0.8, 0.7, 1.0], RATE),
        ("sweep_b", [0.5, 0.9, 0.1, 0.7], RATE),
        ("rate_44100", [0.3, 0.6, 0.4, 0.8], 44100.0),
    ]
    out = []
    for name, knobs, rate in cases:
        e = Emu(exe)
        l, r = run_case(e, knobs, stim, rate)
        out.append({"name": name, "knobs": knobs, "rate": rate, "stim": stim, "outL": l, "outR": r})
        peak = max(max(map(abs, l)), max(map(abs, r)))
        print(name, "peak", peak, flush=True)
    with open(dst, "w") as f:
        json.dump(out, f, separators=(",", ":"))


if __name__ == "__main__":
    main()
