"""Native Gater per-frame kernel vectors under the kemu emulator (original code, no host).

Runs the unchanged kernel FUN_141ae2f60 on a voice struct (the block core's +0x200 object, offsets are
the native ones). The clock/segment code (FUN_141a9d870, FUN_141acb6f0, FUN_141a98d20) is NOT run here:
its output enters as the per-frame level at +0x50 (written by the block core from FUN_141a98d20), and
the control tick (+0x70 period, +0x74 flag bits, ramps) is driven by the case.

    python3 -I tools/re/gater_kernel_kemu.py KONTAKT8_EXE OUT_JSON

Per case: 'ctrl' (+0x70, a tick every ctrl+1 frames), 'mode' (+0x68), 'flags' (+0x74 bits 1,2,4,8,16),
'step' (+0x78), 'ramp' ([state, slope, offset] for bits 1,2,4,8,16 at their native fields), 'scale'
(+0x110), 'delay' (+0x134), 'seed' (+0x108), 'gain' (per-frame +0x50 level), 'stim' (L, R inputs),
'outL', 'outR'. The kernel keeps its own state across frames, so outputs are per frame.
"""
import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

KERNEL = 0x141AE2F60
OBJ_BYTES = 0x4100
RING = 0x138
FRAMES = 4200  # covers a full wrap of the 4000-entry ring

# Native field offsets (FUN_141ae2f60). Ramp state, slope and offset per flag bit.
RAMP_STATE = (0xB8, 0xC8, 0xD8, 0xE8, 0x100)
RAMP_SLOPE = (0xB4, 0xC4, 0xD4, 0xE4, 0xFC)
RAMP_OFFSET = (0xB0, 0xC0, 0xD0, 0xE0, 0xF8)


def cases():
    out = []

    def add(name, ctrl=31, mode=0, flags=0x1F, step=1 / 64, ramp=None, scale=0.8, delay=0, seed=0x12345,
            frames=8 * 32, gain=None, inputs=0.5, sweep_gain=True):
        ramp = ramp or [[0.0, 0.5, 0.25]] * 5
        out.append({
            "name": name, "ctrl": ctrl, "mode": mode, "flags": flags, "step": step,
            "ramp": [list(r) for r in ramp], "scale": scale, "delay": delay, "seed": seed,
            "frames": frames, "gain_mode": "sweep" if sweep_gain else "fixed",
            "gain": gain, "inputs": inputs,
        })

    add("all_ramps_mode0")
    add("all_ramps_mode1", mode=1)
    add("no_flags_static", flags=0)
    add("bit1_only", flags=0x1, ramp=[[0.1, 0.9, 0.0]] + [[0.0, 0.0, 0.0]] * 4)
    add("bit8_mode0_only", flags=0x8, mode=0, ramp=[[0, 0, 0]] * 3 + [[0.0, 1.0, 0.0], [0, 0, 0]])
    add("bit8_mode1_only", flags=0x8, mode=1, ramp=[[0, 0, 0]] * 3 + [[0.0, 1.0, 0.0], [0, 0, 0]])
    add("fast_ctrl_ctrl3", ctrl=3, step=0.2, scale=0.5)
    add("long_delay_wrap", delay=3000, frames=FRAMES, ctrl=15)
    add("delay_1000_seed_b", delay=1000, seed=0x7777, ctrl=7, step=1 / 32, scale=0.6)
    return out


def run_case(e, obj, case):
    f = lambda off, v: e.write(obj + off, "f", v)  # noqa: E731
    i = lambda off, v: e.write(obj + off, "i", v)  # noqa: E731
    e.uc.mem_write(obj, b"\0" * OBJ_BYTES)
    i(0x68, case["mode"])
    i(0x6C, 0)
    i(0x70, case["ctrl"])
    e.write(obj + 0x74, "I", case["flags"])
    f(0x78, case["step"])
    for k in range(5):
        f(RAMP_STATE[k], case["ramp"][k][0])
        f(RAMP_SLOPE[k], case["ramp"][k][1])
        f(RAMP_OFFSET[k], case["ramp"][k][2])
    f(0x110, case["scale"])
    i(0x134, case["delay"])
    i(0x130, 0)
    i(0x108, case["seed"])

    rng = random.Random(case["seed"])
    frames = case["frames"]
    stim = [[rng.uniform(-0.8, 0.8) for _ in range(frames)] for _ in range(2)]
    gain = []
    for n in range(frames):
        if case["gain_mode"] == "sweep":
            gain.append(0.5 + 0.5 * math.sin(2 * math.pi * n / 97.0))
        else:
            gain.append(case["inputs"])
    out = [[], []]
    for n in range(frames):
        f(0x50, gain[n])
        f(0xC, stim[0][n])
        f(0x58, stim[1][n])
        e.call(KERNEL, obj)
        out[0].append(e.read(obj + 0x54, "f"))
        out[1].append(e.read(obj + 0x5C, "f"))
    return {"stim": stim, "gain": gain, "outL": out[0], "outR": out[1]}


def main():
    path, out_path = sys.argv[1], sys.argv[2]
    e = Emu(path)
    obj = e.alloc(OBJ_BYTES, align=64)
    results = []
    for case in cases():
        res = run_case(e, obj, case)
        case.update(res)
        results.append(case)
        print(f"{case['name']}: {len(case['outL'])} frames, peak L {max(abs(v) for v in case['outL']):.6g}")
    with open(out_path, "w") as fh:
        json.dump(results, fh)
    print("wrote", out_path, "cases", len(results), "ring", RING)


if __name__ == "__main__":
    main()
