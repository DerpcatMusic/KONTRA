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


def dense(secs):
    """11-point freq / gain / Q grids per section (plus Q x gain cross grid for LMF/HMF)."""
    steps = []

    def step(name, **kv):
        steps.append({"name": name, "set": [[0, k, v] for k, v in kv.items()]})

    g11 = [i * M // 10 for i in range(11)]
    for sec in secs:
        for f in g11:
            step(f"{sec} dense freq={f} gain=max", **{f"SEQ_{sec}_GAIN": M, f"SEQ_{sec}_FREQ": f})
        for g in g11:
            step(f"{sec} dense gain={g} freq=mid", **{f"SEQ_{sec}_GAIN": g})
        if sec in ("LMF", "HMF"):
            for q in g11:
                step(f"{sec} dense q={q} gain=max", **{f"SEQ_{sec}_GAIN": M, f"SEQ_{sec}_Q": q})
            for q in (0, M // 2, M):
                for g in (M // 10, 3 * M // 10, 7 * M // 10, 9 * M // 10):
                    step(f"{sec} cross q={q} gain={g}", **{f"SEQ_{sec}_GAIN": g, f"SEQ_{sec}_Q": q})
        else:
            for f in (0, M // 4, M // 2, 3 * M // 4, M):
                step(f"{sec} dense bell freq={f} gain=max", **{f"SEQ_{sec}_GAIN": M, f"SEQ_{sec}_FREQ": f, f"SEQ_{sec}_BELL": 1})
            for g in (0, M // 4, 3 * M // 4, M):
                step(f"{sec} dense bell gain={g} freq=mid", **{f"SEQ_{sec}_GAIN": g, f"SEQ_{sec}_BELL": 1})
    return {"chain": ["Filters/Solid G-EQ"], "pre": [[0, k, v] for k, v in NEUTRAL.items()], "steps": steps}


def legacy(kind):
    """Legacy Filters/EQ: one slot of kind 23/24 versus kind-22 slots in series, same bands.
    Bands 500 Hz / 1 kHz / 2 kHz, 1 octave, +12 dB (overlapping, so serial and parallel sums differ)."""
    import math
    fq = lambda hz: round(math.log(hz / 20, 1000) * M)
    bw, up, dn, flat = M // 4, round(30 / 36 * M), round(6 / 36 * M), M // 2
    bands = [fq(500), fq(1000), fq(2000)]
    nb = {22: 3, 23: 2, 24: 3}[kind]
    if kind == 22:      # nb slots of kind 22, band i in slot i
        chain = ["Filters/EQ"] * nb
        pre = [[i, "EFFECT_SUBTYPE", 22] for i in range(nb)]
        pre += [x for i in range(nb) for x in ([i, "FREQ1", bands[i]], [i, "BW1", bw], [i, "GAIN1", flat])]
        g = lambda i, v: [i, "GAIN1", v]
    else:
        chain = ["Filters/EQ"]
        pre = [[0, "EFFECT_SUBTYPE", kind]]
        pre += [x for i in range(nb) for x in ([0, f"FREQ{i + 1}", bands[i]], [0, f"BW{i + 1}", bw], [0, f"GAIN{i + 1}", flat])]
        g = lambda i, v: [0, f"GAIN{i + 1}", v]
    steps = [{"name": f"k{kind} all +12", "set": [g(i, up) for i in range(nb)]}]
    steps += [{"name": f"k{kind} band{i + 1} +12", "set": [g(i, up)]} for i in range(nb)]
    steps += [{"name": f"k{kind} band1 +12 band2 -12", "set": [g(0, up), g(1, dn)]}]
    steps += [{"name": f"k{kind} all 0", "set": []}]
    # a flat step first and between measured steps: multi-slot writes in one callback sometimes landed late
    out = [{"name": f"k{kind} settle", "set": []}]
    for st in steps:
        out += [st, {"name": f"k{kind} flat", "set": []}]
    steps = out
    return {"chain": chain, "pre": pre, "steps": steps}


FILTERS = {"SV_LP1": 50, "SV_HP1": 51, "SV_BP2": 53, "SV_BP4": 56, "SV_LP4": 55, "SV_HP4": 57, "SV_NOTCH4": 58,
           "SV_LP6": 59, "LP1POLE": 2, "HP1POLE": 3, "LP4POLE": 8, "LP6POLE": 12}


def filt():
    """Filters/Effect: cutoff grid at resonance 0 and resonance grid at cutoff 0.5 for each subtype."""
    steps = []
    for name, sub in FILTERS.items():
        for c in GRID:
            steps.append({"name": f"{name} cutoff={c} res=0", "set": [[0, "EFFECT_SUBTYPE", sub], [0, "CUTOFF", c], [0, "RESONANCE", 0]]})
        for r in (M // 2, 9 * M // 10):
            steps.append({"name": f"{name} cutoff=500000 res={r}", "set": [[0, "EFFECT_SUBTYPE", sub], [0, "CUTOFF", M // 2], [0, "RESONANCE", r]]})
    return {"chain": ["Filters/Effect"], "pre": [], "steps": steps}


if __name__ == "__main__":
    spec = {"geq": geq, "dlf": lambda: dense(["LF", "LMF"]), "dhf": lambda: dense(["HMF", "HF"]), "k22": lambda: legacy(22), "k23": lambda: legacy(23), "k24": lambda: legacy(24), "filt": filt}[sys.argv[1]]()
    json.dump(spec, open(sys.argv[2], "w"), indent=1)
