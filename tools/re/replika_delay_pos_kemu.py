"""Native vectors for the Kontakt 8 Replika delay position leaf (`141abd0b0`, 214 B).

`141abd0b0(P, t, s)` (t in xmm1, s in xmm2, f32) sets the two delay positions of the object P:
mid = (hi - lo) t + lo, dl = (mode s) 0.5, a = max(mid + dl, 0) at +0xc8 and b = max(mid - dl, 0) at +0xd0.
On a change of a (b) it also sets +0x58 (+0x60) = base / a (b) with base = +0x48 and the flag +0x90 = 1.
The LOCK/UNLOCK pairs are plain stores here. For each seed, a random object and a sequence of (t, s) calls
are run, and the object fields the port writes are recorded after each call.

    nice -n 19 python3 -I tools/re/replika_delay_pos_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

LEAF = 0x141ABD0B0
OBJ_BYTES = 0x100
STEPS = 40
SEEDS = (1, 2, 3)


def state(e, p):
    return {
        "a": e.read(p + 0xC8, "d"),
        "b": e.read(p + 0xD0, "d"),
        "r58": e.read(p + 0x58, "d"),
        "r60": e.read(p + 0x60, "d"),
        "flag": e.read(p + 0x90, "B"),
    }


def case(e, seed):
    rnd = random.Random(seed)
    p = e.alloc(OBJ_BYTES)
    lo = rnd.randrange(0, 500)
    hi = lo + rnd.randrange(0, 1000)
    mode = rnd.randrange(1, 9)
    base = 1.0 + rnd.random() * 99.0
    e.write(p + 0x08, "i", lo)
    e.write(p + 0x0C, "i", hi)
    e.write(p + 0x28, "i", mode)
    e.write(p + 0x48, "d", base)
    e.write(p + 0x58, "d", rnd.random())
    e.write(p + 0x60, "d", rnd.random())
    e.write(p + 0xC8, "d", rnd.random() * 500.0)
    e.write(p + 0xD0, "d", rnd.random() * 500.0)
    e.write(p + 0x90, "B", 0)
    init = state(e, p)
    init.update({"lo": lo, "hi": hi, "mode": mode, "base": base})

    calls = []
    t, s = rnd.random(), rnd.uniform(-1.0, 1.0)
    for _ in range(STEPS):
        if rnd.random() < 0.7:
            t, s = rnd.random(), rnd.uniform(-1.0, 1.0)
        e.call(LEAF, p, f={1: t, 2: s})
        st = state(e, p)
        st.update({"t": t, "s": s})
        calls.append(st)
    return {"seed": seed, "init": init, "calls": calls}


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    cases = [case(e, s) for s in SEEDS]
    doc = {"name": "replika-delay-pos", "units": "141abd0b0", "cases": cases}
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    for c in cases:
        print("seed %d a range %.4g..%.4g" % (c["seed"], min(x["a"] for x in c["calls"]), max(x["a"] for x in c["calls"])))


if __name__ == "__main__":
    main()
