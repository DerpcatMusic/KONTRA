"""Block-level native vectors for the Kontakt Surround Panner driver (BEffectSurroundPanner, under kemu).

    python3 -I tools/re/surround_panner_driver_kemu.py KONTAKT8_EXE OUT_JSON

Replays the wrapper sequence on one object of the 0x2670-byte class, in the order the factory and the
host use it (no flag is poked; only the parameter state that the host's parameter copy writes):
  1. factory body (0x1407c8720 after its pool allocation: LFE filter ctor 0x1407f0a50, base 0x140a856c0,
     vtable stores, BHeapMem 0x140702fe0 + its vslot 0x38, the 1.0 defaults of the two tables),
  2. 0x140a9cb40 global sin/asin tables,
  3. slot 12 (0x140abd610) output count, slot 6 (0x140ac17a0) source input pointers,
     slot 7 (0x140ab9a30) output pointers, block length +0x120,
  4. parameters: source count is the speaker count (+0x280), +0x284 sources, +0x1c0 speakers,
     +0x348 scale, +0x34c exponent, +0x394 gain mode, +0x1a4/+0x1a8 output gain,
  5. slot 5 (0x14094cde0) reset, then one slot 23 (0x140905fa0) call per block (mode 0),
     with the output buffers cleared by the host before each block.

Scenarios (all stereo output, two speakers, N = 64, six blocks):
  stereo_fixed   two sources, fixed positions
  stereo_moving  source 0 moves along x across blocks
  distance       scale (+0x348) steps across blocks (1.0, 1.7, 0.6)
  mono           one source, fixed position
Each scenario stores its input, parameters per block, and the per-block outputs for both outputs.
"""
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

VT_SURR, VT_LFE, VT_HEAP_FN = 0x144E94E68, 0x144E94528, 0x1407F0A50
CTOR_LFE, CTOR_BASE, CTOR_HEAP, HEAP_INIT = 0x1407F0A50, 0x140A856C0, 0x140702FE0, None
TABLES, SET_COUNT, SET_INPUT, SET_OUTPUT, RESET, DRIVER = (
    0x140A9CB40, 0x140ABD610, 0x140AC17A0, 0x140AB9A30, 0x14094CDE0, 0x140905FA0)
OBJ_SIZE, SCRATCH = 0x2670, 0x100000
N, BLOCKS = 64, 6
SPEAKERS = [(0.1, 0.9), (0.9, 0.9)]


def build(e):
    """Factory body for the object (pool allocation replaced by e.alloc)."""
    obj = e.alloc(OBJ_SIZE)
    e.call(CTOR_LFE, obj + 0x7A0, limit=2_000_000)
    e.call(CTOR_BASE, obj, limit=2_000_000)
    e.write(obj, "Q", VT_SURR)
    e.write(obj + 0x7A0, "Q", VT_LFE)
    for i in range(256):
        e.write(obj + 0xC98 + 24 * i + 16, "f", 1.0)
    for i in range(17):
        e.write(obj + 0x24A0 + 24 * i + 16, "f", 1.0)
    e.call(CTOR_HEAP, obj + 0x2648, 1, limit=2_000_000)
    heap_vt = e.read(obj + 0x2648, "Q")
    e.call(e.read(heap_vt + 0x38, "Q"), obj + 0x2648, 0, limit=2_000_000)
    return obj


def sine(n, blk, src, rate=48000.0):
    return [0.5 * math.sin(2 * math.pi * (220.0 + 110.0 * src) * (blk * n + i) / rate) for i in range(n)]


def scenario(e, name, sources, scales, moving=None, silent_second=False):
    obj = build(e)
    e.call(SET_COUNT, obj, len(SPEAKERS), limit=2_000_000)
    ins = []
    for s in range(len(SPEAKERS)):
        buf = e.alloc(4 * N)
        e.call(SET_INPUT, obj, buf, s, limit=2_000_000)
        ins.append(buf)
    outs = []
    for k in range(len(SPEAKERS)):
        buf = e.alloc(4 * N)
        e.call(SET_OUTPUT, obj, buf, k, limit=2_000_000)
        outs.append(buf)
    scratch = e.alloc(SCRATCH)
    e.write(obj + 0x120, "i", N)
    for i, (x, y) in enumerate(SPEAKERS):
        e.write(obj + 0x1C0 + 12 * i, "f", x, y, 0.0)
        e.write(obj + 0x1C8 + 12 * i, "B", 0)
    e.write(obj + 0x280, "i", len(SPEAKERS))
    e.write(obj + 0x34C, "f", 1.0)
    e.write(obj + 0x394, "i", 0)
    e.write(obj + 0x1A4, "f", 1.0)
    e.write(obj + 0x1A8, "f", 1.0)
    e.call(RESET, obj, limit=2_000_000)

    row = {"name": name, "n": N, "speakers": SPEAKERS, "blocks": []}
    src_pos = [list(p) for p in sources] + [[0.5, 0.5]] * (len(SPEAKERS) - len(sources))
    for blk in range(BLOCKS):
        if moving:
            src_pos = moving(blk, src_pos)
        scale = scales[blk]
        for s, (x, y) in enumerate(src_pos):
            e.write(obj + 0x284 + 12 * s, "f", x, y, 0.0)
        e.write(obj + 0x348, "f", scale)
        # The source count is shared with the output count (+0x124 and +0x128 are both set by slot 12),
        # so a mono scenario keeps a silent second source.
        inputs = [sine(N, blk, s) if s < len(sources) else [0.0] * N for s in range(len(SPEAKERS))]
        if silent_second:
            inputs = [[1.0] * N, [0.0] * N]
        for s, buf in enumerate(ins):
            e.write(buf, "f", *inputs[s])
        for buf in outs:
            e.write(buf, "f", *([0.0] * N))
        e.call(DRIVER, obj, scratch, 0, limit=5_000_000)
        row["blocks"].append({
            "sources": src_pos,
            "scale": scale,
            "input": inputs,
            "out": [list(e.read(buf, "f", N)) for buf in outs],
        })
    return row


def dc_step(e):
    """Constant 1.0 into source 0 for 40 blocks; scale 1.0 for two blocks, then 1.7 (the step at sample 128)."""
    global N, BLOCKS
    saved = N, BLOCKS
    N, BLOCKS = 64, 40
    try:
        row = scenario(e, "dc_step", [(0.8, 0.3), (0.3, 0.6)], [1.0, 1.0] + [1.7] * 38,
                       silent_second=True)
    finally:
        N, BLOCKS = saved
    return row


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe)
    e.call(TABLES, limit=2_000_000)
    fixed = [(0.8, 0.3), (0.3, 0.6)]
    cases = [
        scenario(e, "stereo_fixed", fixed, [1.0] * BLOCKS),
        scenario(e, "stereo_moving", fixed, [1.0] * BLOCKS,
                 moving=lambda b, s: [[0.8 - 0.1 * b, 0.3], s[1]]),
        scenario(e, "distance", fixed, [1.0, 1.0, 1.7, 1.7, 0.6, 0.6]),
        scenario(e, "mono", fixed[:1], [1.0] * BLOCKS),
        dc_step(e),
    ]
    with open(dst, "w") as f:
        json.dump({"exe_note": "Kontakt8.exe via kemu; see module docstring", "cases": cases}, f,
                  separators=(",", ":"))
    for c in cases:
        print(c["name"], "blocks", len(c["blocks"]), "out0[:4]", [round(v, 4) for v in c["blocks"][0]["out"][0][:4]], flush=True)


if __name__ == "__main__":
    main()
