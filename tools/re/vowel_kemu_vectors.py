"""Process-level native vectors for Kontakt 8 Vowel A (saved type 14) and Vowel B (type 15) with
live cutoff and resonance lanes, under kemu (original code, no host).

The object is the real vowel class: vtable VT (slot 1 prepare FUN_1408db380, slot 23 clock
0x140ae9f90, slot 33 kernel thunk 0x140909be0 -> FUN_140914a20, slot 36 setter FUN_14095dd00 /
FUN_14095e250). Each host call writes its cutoff goal (obj+0x1dc) and resonance goal (obj+0x1e0),
the frame count (obj+0x120) and the stimulus, then runs the clock wrapper once. The wrapper's 32-frame
countdown (obj+0x1bc) carries across calls, as in the legacy LP1 harness.

    python3 -I tools/re/vowel_kemu_vectors.py KONTAKT8_EXE OUT_JSON

Mono object, overwrite output (kernel flag r8 = 1). Dirty flag (obj+0x1a1) is set once, before the
first call, so the first call snaps the knobs to their goals; later goals move through the clock.
The raw cutoff goal (obj+0x1f8) stays 0 (the host writes it only for the modulation path, which
is not covered here). Per case: 'class' ("a"/"b"), 'rate' (Hz), 'calls' ([cutoff, resonance, frames]),
'stim' (mono input), 'out' (mono output). Values are rounded to 9 significant digits.
"""
import hashlib
import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

PREPARE, CLOCK, KERNEL, CLOCK_BRANCH = 0x1408DB380, 0x140AE9F90, 0x140909BE0, 0x144FD4C54
CLASSES = {"a": (0x144F12F90, 0x14095DD00), "b": (0x144F13128, 0x14095E250)}
OBJ_BYTES, MAX_FRAMES = 0x2000, 128


def sweep(lo, hi, period, n):
    return [lo + (hi - lo) * (0.5 + 0.5 * math.sin(2 * math.pi * k / period)) for k in range(n)]


def cases():
    out = []

    def add(name, cls, calls, rate=48000.0, seed=0):
        out.append({"name": name, "class": cls, "rate": rate, "calls": calls, "seed": seed})

    add("a_static_mid", "a", [[0.30, 0.50, 64]] * 8, seed=1)
    add("b_static_high_res", "b", [[0.60, 0.90, 64]] * 8, seed=2)
    add("a_cut_sweep_32", "a", [[c, 0.50, 32] for c in sweep(0.05, 0.95, 9, 24)], seed=3)
    sizes = [48, 17, 61, 64, 40, 32, 17, 64, 33, 64]
    add("b_cut_sweep_mixed", "b", [[c, 0.30, n] for c, n in zip(sweep(0.1, 0.9, 7, len(sizes)), sizes)], seed=4)
    add("a_res_sweep_32", "a", [[0.40, r, 32] for r in sweep(0.10, 0.95, 11, 22)], seed=5)
    add("b_both_live_44k", "b", [[c, r, n] for c, r, n in zip(sweep(0.2, 0.8, 6, 14), sweep(0.2, 0.9, 5, 14), [64, 32] * 7)],
        rate=44100.0, seed=6)
    add("a_step_up", "a", [[0.10, 0.50, 32]] * 4 + [[0.90, 0.50, 32]] * 12, seed=7)
    add("a_overrange_then_back", "a", [[1.60, 0.50, 64]] * 2 + [[0.20, 0.50, 64]] * 4, seed=8)
    add("b_negative_goal", "b", [[-0.40, 0.25, 48]] * 2 + [[0.70, 0.25, 48]] * 4, seed=9)
    add("a_96k_live", "a", [[c, 0.60, n] for c, n in zip(sweep(0.3, 0.7, 5, 8), [32, 64] * 4)], rate=96000.0, seed=10)
    return out


def run_case(e, case, inb, outb):
    vt, _ = CLASSES[case["class"]]
    obj = e.alloc(OBJ_BYTES)
    e.write(obj, "Q", vt)
    e.write(obj + 0x124, "I", 1)  # mono
    e.write(obj + 0x1E4, "f", -123.0)
    e.write(obj + 0x20, "Q", inb)  # input channel 0
    e.write(obj + 0xA0, "Q", outb)  # output channel 0
    # Prepare takes the host rate as a float argument (xmm1); FUN_140a9a300 stores it at obj+0x14.
    e.call(PREPARE, obj, f={1: float(case["rate"])}, limit=400000)
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
        e.call(CLOCK, obj, 0, 1, limit=2_000_000)
        stim.extend(block)
        out.extend(e.read(outb, "f", frames))
    return stim, out


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe, heap_mb=64)
    assert e.read(CLOCK_BRANCH, "I") == 0  # the clock's alternative branch is dead in this image
    for key, (vt, setter) in CLASSES.items():
        assert e.read(vt + 1 * 8, "Q") == PREPARE, key
        assert e.read(vt + 23 * 8, "Q") == CLOCK, key
        assert e.read(vt + 33 * 8, "Q") == KERNEL, key
        assert e.read(vt + 36 * 8, "Q") == setter, key
    inb, outb = e.alloc(MAX_FRAMES * 4), e.alloc(MAX_FRAMES * 4)
    results = []
    for case in cases():
        stim, out = run_case(e, case, inb, outb)
        results.append({
            "name": case["name"], "class": case["class"], "rate": case["rate"],
            "calls": [[float(f"{c:.9g}"), float(f"{r:.9g}"), n] for c, r, n in case["calls"]],
            "stim": [float(f"{v:.9g}") for v in stim],
            "out": [float(f"{v:.9g}") for v in out],
        })
        print(case["name"], "frames", len(out), "peak", max(abs(v) for v in out), flush=True)
    data = {
        "native_sha256": hashlib.sha256(open(exe, "rb").read()).hexdigest(),
        "scope": "Vowel A (vtable 0x144f12f90) and B (0x144f13128): prepare 0x1408db380, clock 0x140ae9f90 per "
                 "host call, kernel slot 33 (0x140909be0 -> 0x140914a20), setter slot 36; mono; overwrite",
        "cases": results,
    }
    with open(dst, "w") as f:
        json.dump(data, f, separators=(",", ":"))


if __name__ == "__main__":
    main()
