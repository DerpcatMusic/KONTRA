"""Native Formant I/II (saved types 90/91) vectors under the kemu emulator (original code, no host).

Runs the unchanged Formant core: sample-rate setup FUN_140afd4f0 (rate, quantum 32; 15 ms ramp
length and rate from it), control setter FUN_140b05ac0 (index 0 Talk, 1 Size, 2 Sharp, 3 second
table flag), the 32-frame ramp tick FUN_140b03690 (-> FUN_140acbcb0 on the ramp block at +0x4a0)
and the audio loop FUN_140b00750, stereo.

    python3 -I tools/re/formant_kemu_vectors.py KONTAKT8_EXE OUT_JSON

Host cadence assumed for a live (enabled) lane: every 32-frame chunk writes the setter, then the
tick, then the loop. A static lane is written once, before prepare. Per case: 'rate' (Hz),
'enabled' (per-index bool: written every chunk when true), 'schedule' ([talk, size, sharp, flag]
per 32-frame block, raw control values), 'stim' (one channel, mirrored to both), 'outL' (the right is asserted equal). Values are rounded to 9 digits.
"""
import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

PREPARE, SETTER, TICK, PROCESS = 0x140AFD4F0, 0x140B05AC0, 0x140B03690, 0x140B00750
OBJ_BYTES = 0x2000
BLOCK = 32
QUANTUM = 32
BLOCKS = 40
N = BLOCKS * BLOCK


def complete(e, *args, **kwargs):
    e.call(*args, limit=4_000_000, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


def sweep(lo, hi, period):
    return [lo + (hi - lo) * (0.5 + 0.5 * math.sin(2 * math.pi * b / period)) for b in range(BLOCKS)]


def hold(values, n=BLOCKS):
    return values + [values[-1]] * (n - len(values))


def cases():
    constant = lambda v: [v] * BLOCKS  # noqa: E731
    out = []

    def add(name, rate=48000, enabled=(False, False, False, False), talk=0.5, size=0.5, sharp=0.5, flag=0.0):
        seq = lambda v: v if isinstance(v, list) else constant(v)  # noqa: E731
        talk, size, sharp, flag = seq(talk), seq(size), seq(sharp), seq(flag)
        out.append({
            "name": name,
            "rate": rate,
            "enabled": list(enabled),
            "schedule": [[t, s, h, f] for t, s, h, f in zip(talk, size, sharp, flag)],
        })

    add("static_formant_i")
    add("static_formant_ii", flag=1.0)
    add("static_sharp_size_edges", size=1.0, sharp=0.0, talk=1.0)
    add("live_talk_sweep", enabled=(True, False, False, False), talk=sweep(0.0, 1.0, 9))
    add("live_size_sweep", enabled=(False, True, False, False), size=sweep(0.0, 1.0, 7))
    add("live_sharp_sweep", enabled=(False, False, True, False), sharp=sweep(0.0, 1.0, 5))
    add("live_all_sweep", enabled=(True, True, True, False),
        talk=sweep(0.1, 0.9, 11), size=sweep(0.3, 0.8, 6), sharp=sweep(0.2, 0.9, 4))
    # A live route that moves then holds: the native setter restarts the ramp every chunk.
    add("live_hold_after_step", enabled=(True, False, False, False), talk=hold([0.2, 0.9, 0.9]))
    add("live_all_44k", rate=44100, enabled=(True, True, True, True),
        talk=sweep(0.2, 0.9, 6), size=sweep(0.1, 0.9, 5), sharp=sweep(0.1, 0.9, 7), flag=[1.0] * BLOCKS)
    return out


def run_case(e, case, buffers):
    rate = float(case["rate"])
    in_table, out_table, left, right, out_left, out_right = buffers
    obj = e.alloc(OBJ_BYTES)
    schedule = [list(map(float, k)) for k in case["schedule"]]
    rng = random.Random(90)
    stim = [rng.uniform(-0.3, 0.3) for _ in range(N)]
    enabled = case["enabled"]
    # The first prepare sets the rate; the setters then compute the targets; the second prepare
    # snaps the current values to them (a valid prepared state with no start-up ramp).
    e.write(obj + 0xC, "i", 2)  # two channels (stereo host)
    complete(e, PREPARE, obj, 0, QUANTUM, f={1: rate})
    for index, value in enumerate(schedule[0]):
        complete(e, SETTER, obj, index, f={2: value})
    complete(e, PREPARE, obj, 0, QUANTUM, f={1: rate})
    outL = []
    for block, values in enumerate(schedule):
        for index in range(4):
            if enabled[index]:  # a live route writes its target on every control chunk
                complete(e, SETTER, obj, index, f={2: values[index]})
        span = slice(block * BLOCK, (block + 1) * BLOCK)
        e.write(left, "f", *stim[span.start:span.stop])
        e.write(right, "f", *stim[span.start:span.stop])
        complete(e, TICK, obj)
        complete(e, PROCESS, obj, in_table, out_table, BLOCK)
        left_block = list(e.read(out_left, "f", BLOCK))
        if left_block != list(e.read(out_right, "f", BLOCK)):
            raise RuntimeError(f"{case['name']}: channels differ at block {block}")
        outL.extend(left_block)
    case["stim"] = [float(f"{v:.9g}") for v in stim]
    case["outL"] = [float(f"{v:.9g}") for v in outL]
    return case


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe)
    in_table = e.alloc(16)
    out_table = e.alloc(16)
    left, right = e.alloc(BLOCK * 4), e.alloc(BLOCK * 4)
    out_left, out_right = e.alloc(BLOCK * 4), e.alloc(BLOCK * 4)
    e.write(in_table, "Q", left, right)
    e.write(out_table, "Q", out_left, out_right)
    buffers = (in_table, out_table, left, right, out_left, out_right)
    results = [run_case(e, case, buffers) for case in cases()]
    with open(dst, "w") as f:
        json.dump(results, f, separators=(",", ":"))
    for c in results:
        peak = max(abs(v) for v in c["outL"])
        print(c["name"], "peak", peak, flush=True)


if __name__ == "__main__":
    main()
