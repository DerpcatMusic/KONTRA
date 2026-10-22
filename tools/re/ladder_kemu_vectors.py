"""Native Ladder core vectors under the kemu emulator (original code, no Kontakt host).

Runs the unchanged Ladder core (process 0x140b00b80, setters 0x140b05c70, prepare 0x140b06180,
32-frame control tick 0x140b036a0)
on the cases of an existing vector file and writes every case with its native output:

    python3 -I tools/re/ladder_kemu_vectors.py KONTAKT8_EXE IN_JSON OUT_JSON

Run it on the original three-case vector file (the derived cases are regenerated, not kept).
Per case: 'rate' (Hz), 'mode' (taps selector, core+0x44: 3 = LP4, 9 = BP4), 'flag49' (cutoff path,
core+0x49), 'hq' (quality, setter index 3), 'schedule' ([gain, cutoff, resonance] per 32-frame
block), 'stim' (left, right). Setter order and fields follow the probe in
PluginArtifacts/kontra-native-ladder-20261015 (native_audio_probe.py). Cases in IN_JSON keep their
stimuli and schedules; new cases are derived from them with the fields listed in NEW_CASES.
"""
import copy
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

TABLE_INIT, SETTER, PREPARE, TICK, PROCESS = 0x1404D14D0, 0x140B05C70, 0x140B06180, 0x140B036A0, 0x140B00B80
OBJ_BYTES = 0x6000
BLOCK = 32

# Derived cases: (name, base case name, overrides).
NEW_CASES = [
    ("impulse_flag1_steady", "impulse_steady", {"flag49": 1}),
    ("noise_flag1_steady", "noise_steady", {"flag49": 1}),
    ("noise_flag1_moving_cutoff", "noise_moving_cutoff", {"flag49": 1}),
    ("noise_bp4_steady", "noise_steady", {"mode": 9}),
    ("noise_bp4_moving_cutoff", "noise_moving_cutoff", {"mode": 9}),
    ("noise_bp4_flag1_moving_cutoff", "noise_moving_cutoff", {"mode": 9, "flag49": 1}),
    ("noise_hq_steady", "noise_steady", {"hq": 1}),
    ("noise_hq_moving_cutoff", "noise_moving_cutoff", {"hq": 1}),
    ("noise_hq_flag1_moving_cutoff", "noise_moving_cutoff", {"hq": 1, "flag49": 1}),
]


def complete(e, *args, **kwargs):
    e.call(*args, limit=2_000_000, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


def run_case(e, case, buffers):
    rate = float(case["rate"])
    mode = int(case["mode"])
    flag = int(case.get("flag49", 0))
    hq = int(case.get("hq", 0))
    in_table, out_table, left, right, out_left, out_right = buffers
    obj = e.alloc(OBJ_BYTES)
    e.write(obj + 0xC, "I", 2)  # channels
    e.write(obj + 0x44, "I", mode)  # taps selector
    e.write(obj + 0x4FA0, "f", rate)  # host rate
    e.write(obj + 0x4FB0, "I", BLOCK)  # quantum
    e.write(obj + 0x49, "B", flag)  # cutoff path (flag 0x49)
    schedule = [list(map(float, k)) for k in case["schedule"]]
    stim = [list(map(float, c)) for c in case["stim"]]
    gain, cutoff, resonance = schedule[0]
    for index, value in ((0, gain), (1, cutoff), (2, resonance), (3, float(hq))):
        complete(e, SETTER, obj, index, f={2: value})
    complete(e, PREPARE, obj)
    outL, outR = [], []
    previous = schedule[0]
    for block, knobs in enumerate(schedule):
        for index, (value, before) in enumerate(zip(knobs, previous)):
            if value != before:  # only the lanes that moved, as the host's control path does
                complete(e, SETTER, obj, index, f={2: value})
        previous = knobs
        span = slice(block * BLOCK, (block + 1) * BLOCK)
        e.write(left, "f", *stim[0][span])
        e.write(right, "f", *stim[1][span])
        complete(e, TICK, obj)  # 32-frame control clock: ramps advance here, before the block
        complete(e, PROCESS, obj, in_table, out_table, BLOCK)
        outL.extend(e.read(out_left, "f", BLOCK))
        outR.extend(e.read(out_right, "f", BLOCK))
    return outL, outR


def main():
    exe, src, dst = sys.argv[1:4]
    base = json.load(open(src))
    by_name = {c["name"]: c for c in base}
    e = Emu(exe)
    complete(e, TABLE_INIT)  # exponential table, filled at load in the original
    n = 128
    in_table = e.alloc(16)
    out_table = e.alloc(16)
    left, right = e.alloc(n * 4), e.alloc(n * 4)
    out_left, out_right = e.alloc(n * 4), e.alloc(n * 4)
    e.write(in_table, "Q", left, right)
    e.write(out_table, "Q", out_left, out_right)
    buffers = (in_table, out_table, left, right, out_left, out_right)
    # Re-running on a file that already holds the derived cases must not duplicate them.
    derived_names = {name for name, _, _ in NEW_CASES}
    cases = [copy.deepcopy(c) for c in base if c["name"] not in derived_names]
    for name, parent, overrides in NEW_CASES:
        derived = copy.deepcopy(by_name[parent])
        derived.update(overrides)
        derived["name"] = name
        cases.append(derived)
    for case in cases:
        outL, outR = run_case(e, case, buffers)
        if case["name"] in by_name:
            # Regeneration check against the stored vector (same stimulus and schedule).
            worst = max(
                abs(a - b)
                for got, want in ((outL, by_name[case["name"]]["outL"]), (outR, by_name[case["name"]]["outR"]))
                for a, b in zip(got, want)
            )
            print(case["name"], "regenerated, max abs diff", worst, flush=True)
        else:
            print(case["name"], "new", flush=True)
        case["outL"], case["outR"] = outL, outR
    with open(dst, "w") as f:
        json.dump(cases, f, separators=(",", ":"))


if __name__ == "__main__":
    main()
