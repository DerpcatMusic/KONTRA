#!/usr/bin/env python3
"""Turn probe_fx.py sweep rows into a binding table: ENGINE_PAR -> stored field -> law.

    bindings.py PREFIX.sweep.tsv OUT.tsv [--probe NAME]

Law is fitted on the measured points (normalized n = value / 1e6 -> stored):
    pow a p        stored == a * n**p
    bool           0/1 switch
    identity       stored == n
    linear a b     stored == a + b*n
    exp a r        stored == a * r**n           (frequency-like)
    table          anything else; all points listed (enums, switches, clamps)
Every row keeps the worst fit error so a reader can judge the claim.
"""
import math
import sys
from collections import defaultdict

CONT = [0, 100000, 200000, 300000, 400000, 500000, 600000, 700000, 800000, 900000, 1000000]


def fit(points: dict[int, float]) -> tuple[str, float]:
    if set(points.values()) <= {0.0, 1.0} and 0 in points:
        first = min((v for v in sorted(points) if points[v] == 1.0), default=None)
        return (f"bool: stored=1 when value>={first}" if first is not None else "constant"), 0.0
    if not all(v in points for v in CONT):
        return "table", 0.0
    ns = [v / 1e6 for v in CONT]
    ys = [points[v] for v in CONT]
    err = max(abs(y - n) for n, y in zip(ns, ys))
    if err < 1e-5:
        return "identity", err
    b = ys[-1] - ys[0]
    a = ys[0]
    err = max(abs(a + b * n - y) for n, y in zip(ns, ys))
    if err < 1e-4 * max(1.0, max(abs(y) for y in ys)):
        return f"linear a={a:.6g} b={b:.6g}", err
    if ys[0] > 0 and all(y > 0 for y in ys):
        r = ys[-1] / ys[0]
        err = max(abs(ys[0] * r**n - y) / y for n, y in zip(ns, ys))
        if err < 1e-4:
            return f"exp a={ys[0]:.6g} r={r:.6g}", err
    if ys[0] == 0 and ys[-1] > 0:
        p = math.log(ys[5] / ys[-1]) / math.log(0.5) if ys[5] > 0 else 0
        err = max(abs(ys[-1] * n**p - y) / ys[-1] for n, y in zip(ns, ys))
        if err < 1e-4:
            return f"pow a={ys[-1]:.6g} p={p:.4g}", err
    return "table", 0.0


def main() -> int:
    src, dst = sys.argv[1], sys.argv[2]
    groups = defaultdict(dict)
    meta = {}
    for line in open(src).read().splitlines()[1:]:
        c = line.split("\t")
        if c[7] == "class" or c[3].endswith("EFFECT_SUBTYPE"):
            continue
        key = (c[0], c[3], c[1], c[2], c[5], c[6], c[7])
        try:
            groups[key][int(c[4])] = float(c[9])
        except ValueError:
            continue
        meta[key] = c[8]
    out = ["effect\tpar\tclass\tversion\tfield\tslot\tkind\tlaw\tmax_error\tpoints"]
    for key, pts in sorted(groups.items()):
        if len(pts) == 1 or all(y == next(iter(pts.values())) for y in pts.values()):
            law, err = "constant" if len(pts) > 1 else "table", 0.0
        else:
            law, err = fit(pts)
        shown = " ".join(f"{v}:{pts[v]:.7g}" for v in sorted(pts))
        out.append("\t".join([*key[:4], key[4], key[5], key[6], law, f"{err:.2g}", shown]))
    open(dst, "w").write("\n".join(out) + "\n")
    print(len(out) - 1, "bindings")
    return 0


if __name__ == "__main__":
    sys.exit(main())
