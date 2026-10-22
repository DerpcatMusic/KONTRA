"""Process-level native vectors for the Kontakt 8 Phaser filter (saved type 13) with live cutoff and
resonance lanes, under kemu (original code, no host).

The object is the real BFilterPhaser class: vtable VT 0x144f0fa38 (MSVC RTTI, `docs/re/kontakt-rdata`),
slot 1 prepare 0x1408de8a0 (stores the rate, sets obj+0x200), slot 5 reset 0x14094a720, slot 23 clock
0x140ae9f90 (the wrapper shared with the Vowel, Formant and BFilter LP classes), slot 33 block kernel
0x140909810 (four first-order allpass stages per channel), slot 36 coefficient law 0x14095d770
(cutoff, resonance -> allpass coefficient at obj+0x408, feedback at obj+0x40c).

Each host call writes its cutoff goal (obj+0x1dc) and resonance goal (obj+0x1e0), the frame count
(obj+0x120) and the stimulus, then runs the clock once. The 32-frame countdown (obj+0x1bc) carries
across calls. Dirty flag (obj+0x1a1) is set once, before the first call, so the first call snaps the
knobs to their goals. The coefficient constants at obj+0x410..0x430 are written after the reset, as the
constructor does (the reset zeroes obj+0x410..0x41c).

    python3 -I tools/re/phaser_kemu_vectors.py KONTAKT8_EXE OUT_JSON

Mono object, overwrite output (kernel flag r8 = 1). Per case: 'rate' (Hz), 'calls' ([cutoff, resonance,
frames]), 'stim' (mono input), 'out' (mono output). Values are rounded to 9 significant digits.
"""
import hashlib
import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

VT = 0x144F0FA38
PREPARE, RESET, CLOCK, KERNEL, SETTER = 0x1408DE8A0, 0x14094A720, 0x140AE9F90, 0x140909810, 0x14095D770
CLOCK_BRANCH = 0x144FD4C54
OBJ_BYTES, MAX_FRAMES = 0x2000, 128
# Taylor and ln2 constants the constructor stores at obj+0x410 (f32 bit patterns, FUN_1407c9a70).
CONSTANTS = [(0x410, 0x3F000000), (0x414, 0x3E2AAAAB), (0x418, 0x3D2AAAAB), (0x41C, 0x3C088889),
             (0x420, 0x3AB60B61), (0x424, 0x39500D01), (0x428, 0x37D00D01), (0x42C, 0x3F317218),
             (0x430, 0x3FB17218)]


def sweep(lo, hi, period, n):
    return [lo + (hi - lo) * (0.5 + 0.5 * math.sin(2 * math.pi * k / period)) for k in range(n)]


def cases():
    out = []

    def add(name, calls, rate=48000.0, seed=0):
        out.append({"name": name, "rate": rate, "calls": calls, "seed": seed})

    add("static_mid", [[0.30, 0.50, 64]] * 8, seed=1)
    add("static_high_res", [[0.60, 0.90, 64]] * 8, seed=2)
    add("cut_sweep_32", [[c, 0.50, 32] for c in sweep(0.05, 0.95, 9, 24)], seed=3)
    sizes = [48, 17, 61, 64, 40, 32, 17, 64, 33, 64]
    add("cut_sweep_mixed", [[c, 0.30, n] for c, n in zip(sweep(0.1, 0.9, 7, len(sizes)), sizes)], seed=4)
    add("res_sweep_32", [[0.40, r, 32] for r in sweep(0.10, 0.95, 11, 22)], seed=5)
    add("both_live_44k", [[c, r, n] for c, r, n in zip(sweep(0.2, 0.8, 6, 14), sweep(0.2, 0.9, 5, 14), [64, 32] * 7)],
        rate=44100.0, seed=6)
    add("step_up", [[0.10, 0.50, 32]] * 4 + [[0.90, 0.50, 32]] * 12, seed=7)
    add("overrange_then_back", [[1.60, 0.50, 64]] * 2 + [[0.20, 0.50, 64]] * 4, seed=8)
    add("negative_goal", [[-0.40, 0.25, 48]] * 2 + [[0.70, 0.25, 48]] * 4, seed=9)
    add("live_96k", [[c, 0.60, n] for c, n in zip(sweep(0.3, 0.7, 5, 8), [32, 64] * 4)], rate=96000.0, seed=10)
    return out


def complete(e, *args, **kwargs):
    e.call(*args, limit=2_000_000, **kwargs)


def new_object(e, inb, outb):
    obj = e.alloc(OBJ_BYTES)
    e.write(obj, "Q", VT)
    e.write(obj + 0x124, "I", 1)  # mono
    e.write(obj + 0x20, "Q", inb)  # input channel 0
    e.write(obj + 0xA0, "Q", outb)  # output channel 0
    complete(e, RESET, obj)  # native reset: state block, sentinels -1 at obj+0x1e4/0x1e8
    for offset, bits in CONSTANTS:  # after the reset, which zeroes obj+0x410..0x41c
        e.write(obj + offset, "I", bits)
    return obj


def run_case(e, case, inb, outb):
    obj = new_object(e, inb, outb)
    # Prepare takes the host rate as a float argument (xmm1); it stores it at obj+0x14.
    complete(e, PREPARE, obj, f={1: float(case["rate"])})
    assert abs(e.read(obj + 0x14, "f") - float(case["rate"])) < 1e-3
    rng = random.Random(case["seed"])
    e.write(obj + 0x1A1, "B", 1)  # dirty once: the first call snaps the knobs to their goals
    stim, out = [], []
    for cut, res, frames in case["calls"]:
        e.write(obj + 0x120, "I", frames)
        e.write(obj + 0x1DC, "f", cut)
        e.write(obj + 0x1E0, "f", res)
        block = [rng.uniform(-0.3, 0.3) for _ in range(frames)]
        e.write(inb, "f", *block)
        complete(e, CLOCK, obj, 0, 1)
        stim.extend(block)
        out.extend(e.read(outb, "f", frames))
    return stim, out


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe, heap_mb=64)
    assert e.read(CLOCK_BRANCH, "I") == 0  # the clock's alternative branch is dead in this image
    assert e.read(VT + 1 * 8, "Q") == PREPARE
    assert e.read(VT + 5 * 8, "Q") == RESET
    assert e.read(VT + 23 * 8, "Q") == CLOCK
    assert e.read(VT + 33 * 8, "Q") == KERNEL
    assert e.read(VT + 36 * 8, "Q") == SETTER
    inb, outb = e.alloc(MAX_FRAMES * 4), e.alloc(MAX_FRAMES * 4)
    results = []
    for case in cases():
        stim, out = run_case(e, case, inb, outb)
        results.append({
            "name": case["name"], "rate": case["rate"],
            "calls": [[float(f"{c:.9g}"), float(f"{r:.9g}"), n] for c, r, n in case["calls"]],
            "stim": [float(f"{v:.9g}") for v in stim],
            "out": [float(f"{v:.9g}") for v in out],
        })
        print(case["name"], "frames", len(out), "peak", max(abs(v) for v in out), flush=True)
    data = {
        "native_sha256": hashlib.sha256(open(exe, "rb").read()).hexdigest(),
        "scope": "Kontakt 8 Phaser filter (type 13, vtable 0x144f0fa38): reset 0x14094a720, prepare 0x1408de8a0, "
                 "clock 0x140ae9f90 per host call, kernel slot 33 (0x140909810), coefficient slot 36 (0x14095d770); "
                 "mono; overwrite",
        "cases": results,
    }
    with open(dst, "w") as f:
        json.dump(data, f, separators=(",", ":"))


if __name__ == "__main__":
    main()
