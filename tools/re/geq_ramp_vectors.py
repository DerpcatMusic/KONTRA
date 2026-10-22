"""geq_ramp_vectors.py OUT.json: native Solid G-EQ knob-glide schedule, run under kemu.

    python3 tools/re/geq_ramp_vectors.py crates/sampler-core/src/dsp/eq_family/geq_ramp_vectors.json

Drives the real live-control path of the Kontakt 8 G_EQ object, one 32-frame grid block at a
time, exactly as the caller 0x1408fbd60 does: setter 0x141ac70f0 for each control that moved,
block start 0x141ab4d70, then the frame loop 0x141a595d0 (called here with one frame at a time so
every frame's lanes can be read). Reset and ramp length: 0x141a9eac0, 0x141aa1fb0 (N = int(rate *
0.02 / 32 + 0.5), increment 1 / (N * 32)). Lanes 0..11 are read at 0x13db0 (current); the increments
(0x13df0) are not stored (the Rust side recomputes them from the same law); the gate and the countdown are at 0x13e71 and 0x13e74.

Two steps on the LMF gain (lane 2): 0.5 -> 0.75 at block 1, and 0.75 -> 0.25 at block 16, while
the first ramp is still moving. The second step re-times every moving lane from where it is.
Audio is not compared: the native design writes sit in the cascade records, which need the
object's loader state that kemu does not reproduce (eq-kontakt.md sec. 8).
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

EXE = "/data/dev/workspace/re-binaries/Kontakt8.exe"
SET, BLOCK_START, FRAMES, RAMP_LEN, RESET = 0x141AC70F0, 0x141AB4D70, 0x141A595D0, 0x141AA1FB0, 0x141A9EAC0
# Setter index of each of the 16 lanes (lane 12 is the 0x21 path, lanes 13..15 are unused here).
LANE_INDEX = [0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13]
NEUTRAL = {0: 0.5, 1: 0.5, 3: 0.5, 4: 0.5, 5: 0.5, 6: 0.5, 7: 0.5, 8: 0.5, 9: 0.5, 10: 0.5, 12: 0.0, 13: 1.0}
BLOCKS = 64
STEPS = {1: (3, 0.75), 16: (3, 0.25)}  # block -> (setter index, value)


def run(rate):
    e = Emu(EXE)
    obj = e.alloc(0x20000)
    ptr = e.alloc(64)
    bufs = [e.alloc(4 * 2048), e.alloc(4 * 2048)]
    e.call(RESET, obj)
    e.write(obj + 0x14, "f", float(rate))
    e.write(obj + 0xC, "I", 2)
    e.write(obj + 0x10, "I", 2)
    e.call(RAMP_LEN, obj, 0, 32, f={1: float(rate)})
    n = e.read(obj + 0x13E78, "i")
    e.write(obj + 0x13DA0, "B", 1)  # live
    # Steady state before the first step: lanes and targets equal the knobs.
    lanes = [0.0] * 16
    for k, v in NEUTRAL.items():
        lanes[LANE_INDEX.index(k)] = v
    for i, v in enumerate(lanes):
        e.write(obj + 0x13DB0 + 4 * i, "f", v)
        e.write(obj + 0x13E30 + 4 * i, "f", v)
    frames = []
    for blk in range(BLOCKS):
        if blk in STEPS:
            idx, value = STEPS[blk]
            e.call(SET, obj, idx, 0, f={2: value})
        e.call(BLOCK_START, obj)
        for j in range(32):
            f = blk * 32 + j
            e.write(ptr, "Q", bufs[0] + 4 * j, bufs[1] + 4 * j)
            e.call(FRAMES, obj, ptr, ptr, 1)
            frames.append({
                "frame": f,
                "lanes": [e.read(obj + 0x13DB0 + 4 * i, "f") for i in range(12)],
                "gate": e.read(obj + 0x13E71, "B"),
                "counter": e.read(obj + 0x13E74, "i"),
            })
    return {"rate": rate, "blocks_n": n, "frames": frames}


def main():
    out = {
        "source": "Kontakt 8 G_EQ live path under kemu: setter 0x141ac70f0, block start 0x141ab4d70, frame loop 0x141a595d0",
        "steps": {str(b): {"setter_index": i, "value": v} for b, (i, v) in STEPS.items()},
        "runs": [run(48000), run(44100)],
    }
    with open(sys.argv[1], "w") as fh:
        json.dump(out, fh)
        fh.write("\n")
    for r in out["runs"]:
        print("rate", r["rate"], "N", r["blocks_n"], "frames", len(r["frames"]))


if __name__ == "__main__":
    main()
