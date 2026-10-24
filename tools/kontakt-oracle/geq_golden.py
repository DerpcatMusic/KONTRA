#!/usr/bin/env python3
"""Solid G-EQ golden table from a fxrig sweep: one row per step, the active section and its knobs.

    geq_golden.py SWEEP.tsv SPEC.json OUT.tsv

Columns: step, name, section, freq, q, gain (knobs 0..1), then the native dB at PROBES (LL pair, bypass-referenced).
Sections: LFSHELF LFBELL LMF HMF HFSHELF HFBELL HP LP. Steps that touch two sections (HP and LP) are skipped.
"""
import json
import sys

M = 1_000_000
PROBES = [30, 60, 100, 200, 500, 1000, 2000, 5000, 10000, 15000]


def section(merged, touched):
    """touched: the parameter names this step sets (the pre-state is neutral, so it does not pick the section)."""
    keys = {k.split("_", 2)[1] for k in touched if k.startswith("SEQ_")}
    if "SEQ_HP" in touched and "SEQ_LP" in touched:
        return None
    if merged.get("SEQ_HP") == 1 and "HP" in keys:
        return "HP"
    if merged.get("SEQ_LP") == 1 and "LP" in keys:
        return "LP"
    if "LF" in keys:
        return "LFBELL" if merged.get("SEQ_LF_BELL") == 1 else "LFSHELF"
    if "LMF" in keys:
        return "LMF"
    if "HMF" in keys:
        return "HMF"
    if "HF" in keys:
        return "HFBELL" if merged.get("SEQ_HF_BELL") == 1 else "HFSHELF"
    return None


def knobs(merged, sec):
    pre = {"HP": ("HP_FREQ", None, None), "LP": ("LP_FREQ", None, None),
           "LFSHELF": ("LF_FREQ", None, "LF_GAIN"), "LFBELL": ("LF_FREQ", None, "LF_GAIN"),
           "LMF": ("LMF_FREQ", "LMF_Q", "LMF_GAIN"), "HMF": ("HMF_FREQ", "HMF_Q", "HMF_GAIN"),
           "HFSHELF": ("HF_FREQ", None, "HF_GAIN"), "HFBELL": ("HF_FREQ", None, "HF_GAIN")}[sec]
    f, q, g = (merged.get(f"SEQ_{p}", 500_000) / M if p else 0.0 for p in pre)
    return f, q, g


def main():
    sweep, spec_path, out = sys.argv[1], sys.argv[2], sys.argv[3]
    spec = json.load(open(spec_path))
    base = {(a, b): c for a, b, c in spec["pre"]}
    rows = [l.split("\t") for l in open(sweep).read().splitlines() if l]
    lines = ["step\tname\tsection\tfreq\tq\tgain\t" + "\t".join(f"db@{p}" for p in PROBES)]
    for (n, step), row in zip(enumerate(spec["steps"], start=1), rows):
        merged = {b: c for (_, b), c in base.items()}
        merged.update({b: c for _, b, c in step["set"]})
        sec = section(merged, [b for _, b, _ in step["set"]])
        if sec is None:
            continue
        f, q, g = knobs(merged, sec)
        lines.append("\t".join([str(n), step["name"], sec, f"{f:.6f}", f"{q:.6f}", f"{g:.6f}"] + row[2:]))
    open(out, "w").write("\n".join(lines) + "\n")
    print(out, len(lines) - 1, "rows")


if __name__ == "__main__":
    main()
