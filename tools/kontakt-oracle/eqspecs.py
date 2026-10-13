#!/usr/bin/env python3
"""Write eqrender.py specs.   eqspecs.py geq|legacy OUT.json"""
import json
import sys

M = 1000000
NEUTRAL = {"SEQ_LF_GAIN": M // 2, "SEQ_LF_FREQ": M // 2, "SEQ_LF_BELL": 0,
           "SEQ_LMF_GAIN": M // 2, "SEQ_LMF_FREQ": M // 2, "SEQ_LMF_Q": M // 2,
           "SEQ_HMF_GAIN": M // 2, "SEQ_HMF_FREQ": M // 2, "SEQ_HMF_Q": M // 2,
           "SEQ_HF_GAIN": M // 2, "SEQ_HF_FREQ": M // 2, "SEQ_HF_BELL": 0,
           "SEQ_HP": 0, "SEQ_HP_FREQ": 0, "SEQ_LP": 0, "SEQ_LP_FREQ": M}
GRID = [0, M // 4, M // 2, 3 * M // 4, M]


def geq():
    steps = []

    def step(name, **kv):
        steps.append({"name": name, "set": [[0, k, v] for k, v in kv.items()]})

    for sec in ("LF", "LMF", "HMF", "HF"):
        for f in GRID:
            step(f"{sec} freq={f} gain=max", **{f"SEQ_{sec}_GAIN": M, f"SEQ_{sec}_FREQ": f})
        for g in (0, M // 4, 3 * M // 4, M):
            step(f"{sec} gain={g} freq=mid", **{f"SEQ_{sec}_GAIN": g})
        if sec in ("LMF", "HMF"):
            for q in GRID:
                step(f"{sec} q={q} gain=max", **{f"SEQ_{sec}_GAIN": M, f"SEQ_{sec}_Q": q})
            for f in (M // 4, 3 * M // 4):
                for q in (0, M):
                    step(f"{sec} freq={f} q={q} gain=max", **{f"SEQ_{sec}_GAIN": M, f"SEQ_{sec}_FREQ": f, f"SEQ_{sec}_Q": q})
        else:
            for f in GRID:
                step(f"{sec} bell freq={f} gain=max", **{f"SEQ_{sec}_GAIN": M, f"SEQ_{sec}_FREQ": f, f"SEQ_{sec}_BELL": 1})
                step(f"{sec} bell freq={f} gain=min", **{f"SEQ_{sec}_GAIN": 0, f"SEQ_{sec}_FREQ": f, f"SEQ_{sec}_BELL": 1})
            step(f"{sec} shelf gain=min", **{f"SEQ_{sec}_GAIN": 0})
    for f in (0, M // 5, 2 * M // 5, 3 * M // 5, 4 * M // 5, M):
        step(f"HP freq={f}", SEQ_HP=1, SEQ_HP_FREQ=f)
        step(f"LP freq={f}", SEQ_LP=1, SEQ_LP_FREQ=f)
    step("HP on freq=default(0)", SEQ_HP=1)
    step("LP on freq=default", SEQ_LP=1)
    step("HP and LP freq=mid", SEQ_HP=1, SEQ_HP_FREQ=M // 2, SEQ_LP=1, SEQ_LP_FREQ=M // 2)
    return {"chain": ["Filters/Solid G-EQ"], "pre": [[0, k, v] for k, v in NEUTRAL.items()], "steps": steps}


def legacy():
    """Same three bands as kind 22 x3 in series (3 slots) versus one kind 24 (and kind 23 + kind 22)."""
    raise SystemExit("legacy specs are built by legacy_topology.py")


if __name__ == "__main__":
    json.dump({"geq": geq}[sys.argv[1]](), open(sys.argv[2], "w"), indent=1)
