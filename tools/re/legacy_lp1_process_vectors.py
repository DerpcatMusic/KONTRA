"""Process-level native vectors for legacy LP1 (type 2) and HP1 (type 3) with a moving cutoff.

Runs the original object under kemu with its real vtable (0x144e95210): the clock
`0x140ae9f90` (per host call), the kernel (vtable slot 33 `0x140909100`) and the setter
(vtable slot 36 `0x14095a5a0`). Each host call has its own frame count and cutoff goal (obj+0x1dc),
as the host does. Output: per-frame output per case.

    python3 -I tools/re/legacy_lp1_process_vectors.py KONTAKT8_EXE OUT_JSON

The kernel's flag argument (`r8`) selects overwrite (1) or accumulate into the output (0).

Scope: mono object (channel count 1), 48 kHz, resonance 0.25, no modulation. Stimulus is
seeded uniform noise in [-0.3, 0.3]. Each case lists (goal, frames) per host call.
"""
import hashlib
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

PREPARE, CLOCK, KERNEL = 0x1408DE6E0, 0x140AE9F90, 0x140909100
# Class vtables (image addresses; slot 36 is the setter, slot 33 the kernel, slot 23 the clock).
CLASSES = {"lp1": (0x144E95210, 0x14095A5A0), "hp1": (0x144E953A8, 0x14095A200)}
RATE, RESONANCE, MAX_FRAMES = 48000.0, 0.25, 64
# Each case: (class, [(goal, frames), ...]). Host calls are at most 64 frames (the engine's block).
# Sizes that are not multiples of 32 leave the native 32-frame countdown mid-segment.
CASES = {
    "lp1_step_up_hold": ("lp1", [(8.5, 64), (0.5, 64), (0.5, 64), (0.5, 64), (0.3, 64)]),
    "lp1_mixed_sizes": ("lp1", [(8.5, 48), (0.5, 32), (0.5, 17), (0.5, 61), (0.5, 64), (0.5, 40)]),
    "lp1_up_down": ("lp1", [(0.5, 64), (8.5, 48), (8.5, 64), (0.5, 32), (0.5, 40)]),
    "lp1_snap_small_move": ("lp1", [(8.5, 32), (8.45, 64), (8.45, 32), (8.45, 17)]),
    "lp1_partial_then_move": ("lp1", [(8.5, 64), (8.0, 17), (8.0, 48), (0.2, 64), (0.2, 33)]),
    "hp1_mixed_sizes": ("hp1", [(8.5, 48), (0.5, 32), (0.5, 17), (0.5, 61), (0.5, 64), (0.5, 40)]),
    "hp1_up_down": ("hp1", [(0.5, 64), (8.5, 48), (8.5, 64), (0.5, 32), (0.5, 40)]),
}


def run(exe, cls, calls, seed):
    VT, SETTER = CLASSES[cls]
    e = Emu(exe, heap_mb=64)
    assert e.read(VT + 36 * 8, "Q") == SETTER and e.read(VT + 33 * 8, "Q") == KERNEL
    obj = e.alloc(0x2000)
    e.write(obj, "Q", VT)
    e.write(obj + 0x1E4, "f", -123.0)
    e.call(PREPARE, obj, f={1: RATE}, limit=200000)
    e.write(obj + 0x124, "I", 1)
    e.write(obj + 0x1A1, "B", 1)
    e.write(obj + 0x1E0, "f", RESONANCE)
    inb, outb = e.alloc(MAX_FRAMES * 4), e.alloc(MAX_FRAMES * 4)
    e.write(obj + 0x20, "Q", inb)
    e.write(obj + 0xA0, "Q", outb)
    rng = random.Random(seed)
    stim, out = [], []
    for goal, frames in calls:
        e.write(obj + 0x120, "I", frames)
        e.write(obj + 0x1DC, "f", goal)
        block = [rng.uniform(-0.3, 0.3) for _ in range(frames)]
        e.write(inb, "f", *block)
        # r8 = 1: the kernel overwrites the output (flag 0 would add to the buffer's old contents).
        e.call(CLOCK, obj, 0, 1, limit=500000)
        stim.extend(block)
        out.extend(e.read(outb, "f", frames))
    return stim, out


def main():
    exe, dst = sys.argv[1:3]
    cases = []
    for seed, (name, (cls, calls)) in enumerate(CASES.items(), start=1):
        stim, out = run(exe, cls, calls, seed)
        cases.append({"name": name, "class": cls, "calls": [[g, n] for g, n in calls], "stim": stim, "out": out})
    data = {
        "native_sha256": hashlib.sha256(open(exe, "rb").read()).hexdigest(),
        "scope": "LP1 and HP1 real vtables (0x144e95210, 0x144e953a8); clock 0x140ae9f90 per host call, "
                 "kernel slot 33, setter slot 36; mono; 48 kHz; resonance .25; cutoff goal and frame count per call",
        "rate": RATE,
        "resonance": RESONANCE,
        "cases": cases,
    }
    with open(dst, "w") as f:
        json.dump(data, f, separators=(",", ":"))
    for c in cases:
        print(c["name"], "frames", len(c["out"]), "peak", max(abs(v) for v in c["out"]), flush=True)


if __name__ == "__main__":
    main()
