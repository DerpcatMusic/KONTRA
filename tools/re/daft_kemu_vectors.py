"""Native Daft (FilterDaft, stored 70/71) vectors under the kemu emulator (original code, no host).

Runs the unchanged core: constructor FUN_140af4550, prepare FUN_140afd210 (rate, quantum 32), setter
dispatch FUN_140b05900 (index 0 gain, 1 cutoff, 2 resonance, 3 response), 32-frame ramp tick
FUN_140b03620 (vtable slot 8) and the loop FUN_140affa30 (vtable slot 7), stereo.

    python3 -I tools/re/daft_kemu_vectors.py KONTAKT8_EXE OUT_JSON

Host cadence (FUN_1408fa270, the BFilterDaft slot-23 driver): every 32 host frames it calls the
setters of the enabled modulation routes (flags at +0x49090..93), then the tick, then the loop. An
enabled route is therefore written on every chunk, including unchanged values; a disabled index is
written once, before prepare. Enabled routes are listed per case in 'enabled'.
Per case: 'rate' (Hz), 'response' (0..1, read as int(2x): below 0.5 low pass), 'enabled' (4 flags),
'schedule' ([gain, cutoff, resonance] per 32-frame block, unity-based gain; the response is fixed),
'stim' (left, right), 'outL', 'outR'.
"""
import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

TABLE_INIT, CTOR, PREPARE, SETTER, TICK, PROCESS = 0x1404D14D0, 0x140AF4550, 0x140AFD210, 0x140B05900, 0x140B03620, 0x140AFFA30
OBJ_BYTES = 0x4A000
BLOCK = 32
QUANTUM = 32
BLOCKS = 8
N = BLOCKS * BLOCK


def complete(e, *args, **kwargs):
    e.call(*args, limit=4_000_000, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


def sweep(lo, hi, period):
    return [lo + (hi - lo) * (0.5 + 0.5 * math.sin(2 * math.pi * b / period)) for b in range(BLOCKS)]


def cases():
    constant = lambda v: [v] * BLOCKS  # noqa: E731
    out = []

    def add(name, rate=48000, response=0.2, enabled=(False, False, False, False), gain=0.5, cutoff=0.5, resonance=0.5):
        out.append({
            "name": name,
            "rate": rate,
            "response": response,
            "enabled": list(enabled),
            "schedule": [[g, c, r] for g, c, r in zip(
                gain if isinstance(gain, list) else constant(gain),
                cutoff if isinstance(cutoff, list) else constant(cutoff),
                resonance if isinstance(resonance, list) else constant(resonance),
            )],
        })

    add("lp_steady")
    add("hp_steady", response=0.8)
    add("lp_negative_gain", gain=-0.5)
    add("lp_self_oscillating", cutoff=0.8, resonance=0.95)
    add("lp_moving_cutoff", enabled=(False, True, False, False), cutoff=sweep(0.2, 0.9, 5))
    add("hp_moving_cutoff", response=0.8, enabled=(False, True, False, False), cutoff=sweep(0.2, 0.9, 5))
    add("lp_moving_resonance", enabled=(False, False, True, False), resonance=sweep(0.1, 0.9, 3))
    add("lp_moving_gain", enabled=(True, False, False, False), gain=sweep(0.0, 1.0, 4))
    add("lp_moving_all", enabled=(True, True, True, False),
        gain=sweep(0.1, 0.9, 4), cutoff=sweep(0.3, 0.8, 6), resonance=sweep(0.2, 0.7, 3))
    add("lp_moving_cutoff_44k", rate=44100, enabled=(False, True, False, False), cutoff=sweep(0.2, 0.9, 5))
    # An enabled route that moves then holds: native rewrites the same target every chunk.
    hold = [0.2, 0.5, 0.6] + [0.6] * (BLOCKS - 3)
    add("lp_enabled_cutoff_hold", enabled=(False, True, False, False), cutoff=hold)
    add("lp_enabled_resonance_hold", enabled=(False, False, True, False), resonance=[0.1, 0.8] + [0.8] * (BLOCKS - 2))
    return out


def run_case(e, case, buffers):
    rate = float(case["rate"])
    in_table, out_table, left, right, out_left, out_right = buffers
    obj = e.alloc(OBJ_BYTES)
    complete(e, CTOR, obj)
    schedule = [list(map(float, k)) for k in case["schedule"]]
    stim = [[0.0] * N, [0.0] * N]
    rng = random.Random(70)
    stim = [[rng.uniform(-0.3, 0.3) for _ in range(N)], [rng.uniform(-0.3, 0.3) for _ in range(N)]]
    enabled = case["enabled"]
    first = schedule[0]
    for index, value in enumerate(first + [case["response"]]):
        complete(e, SETTER, obj, index, f={2: value})
    complete(e, PREPARE, obj, 0, QUANTUM, f={1: rate})
    outL, outR = [], []
    for block, knobs in enumerate(schedule):
        values = knobs + [case["response"]]
        for index in range(4):
            if enabled[index]:  # a modulation route writes its target on every control chunk
                complete(e, SETTER, obj, index, f={2: values[index]})
        span = slice(block * BLOCK, (block + 1) * BLOCK)
        e.write(left, "f", *stim[0][span])
        e.write(right, "f", *stim[1][span])
        complete(e, TICK, obj)
        complete(e, PROCESS, obj, in_table, out_table, BLOCK)
        outL.extend(e.read(out_left, "f", BLOCK))
        outR.extend(e.read(out_right, "f", BLOCK))
    case["stim"] = stim
    case["outL"], case["outR"] = outL, outR
    return case


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe)
    complete(e, TABLE_INIT)  # exponential table, filled at load in the original
    n = N
    in_table = e.alloc(16)
    out_table = e.alloc(16)
    left, right = e.alloc(n * 4), e.alloc(n * 4)
    out_left, out_right = e.alloc(n * 4), e.alloc(n * 4)
    e.write(in_table, "Q", left, right)
    e.write(out_table, "Q", out_left, out_right)
    buffers = (in_table, out_table, left, right, out_left, out_right)
    results = [run_case(e, case, buffers) for case in cases()]
    with open(dst, "w") as f:
        json.dump(results, f, separators=(",", ":"))
    for c in results:
        peak = max(abs(v) for v in c["outL"] + c["outR"])
        print(c["name"], "peak", peak, flush=True)


if __name__ == "__main__":
    main()
