"""Native vectors for the Kontakt Surround Panner speaker law (BEffectSurroundPanner), under kemu.

    python3 -I tools/re/surround_panner_kemu_vectors.py KONTAKT8_EXE OUT_JSON

Runs the unchanged original functions on an object whose fields are written at their native offsets:
  +0x280 int speaker count, +0x284 + 12*i source (x, y), +0x1c0 + 12*i speaker (x, y), byte +0x1c8 + 12*i
  speaker disabled flag, +0x348 scale, +0x34c exponent, +0x350/+0x354 output offset, +0x358 radius offset,
  +0x35c angle offset, +0x394 gain mode.

Functions:
  tables   FUN_140a9cb40  writes the sin table (0x14a611750, quarter-wave step 2pi/4096) and the asin table
           (0x14a612760, step 2/1024 from -1). 1025 entries each.
  lookup   FUN_140ac3230  periodic sin lookup (scale 1.0).
  pos      FUN_140a985b0  source position in the speaker plane (writes +x / +y outputs).
  dist     FUN_140a94180  distance source -> speaker (flag 1 scales by 1.17851126).
  weight   FUN_140a93cb0  per-speaker angular weight.
  gain     FUN_140a94580  speaker gain law, modes 0 (normalised power law), 1 (cosine), 2 (exp2).
The case list covers two layouts (a 5.1-like ring and an irregular one with a disabled speaker).
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

TABLES, LOOKUP, POS, DIST, WEIGHT, GAIN = 0x140A9CB40, 0x140AC3230, 0x140A985B0, 0x140A94180, 0x140A93CB0, 0x140A94580
SIN_TAB, ASIN_TAB = 0x14A611750, 0x14A612760
TAB_LEN = 1025

OBJ_SIZE = 0x800


def complete(e, fn, *args, **kwargs):
    e.call(fn, *args, limit=2_000_000, **kwargs)


def setup(e, obj, layout, scale, p, mode):
    e.write(obj + 0x280, "i", len(layout["speakers"]))
    e.write(obj + 0x284, "f", *layout["source"], 0.0)
    for i, (x, y, disabled) in enumerate(layout["speakers"]):
        base = 0x1C0 + 12 * i
        e.write(obj + base, "f", x, y, 0.0)
        e.write(obj + 0x1C8 + 12 * i, "B", 1 if disabled else 0)
    e.write(obj + 0x348, "f", scale)
    e.write(obj + 0x34C, "f", p)
    e.write(obj + 0x350, "f", layout["off"][0], layout["off"][1])
    e.write(obj + 0x358, "f", layout["radius_off"], layout["angle_off"])
    e.write(obj + 0x394, "i", mode)


def main():
    exe, dst = sys.argv[1:3]
    rng = random.Random(11)
    e = Emu(exe)
    complete(e, TABLES)
    sin_t = list(e.read(SIN_TAB, "f", TAB_LEN))
    asin_t = list(e.read(ASIN_TAB, "f", TAB_LEN))

    obj = e.alloc(OBJ_SIZE)
    layouts = [
        {
            "name": "ring5",
            "source": (0.8, 0.3),
            "speakers": [(0.1, 0.9, False), (0.9, 0.9, False), (0.5, 0.5, False), (0.0, 0.1, False), (1.0, 0.1, False)],
            "off": (0.0, 0.0),
            "radius_off": 0.0,
            "angle_off": 0.0,
        },
        {
            "name": "irregular",
            "source": (0.22, 0.61),
            "speakers": [(0.3, 0.2, False), (0.7, 0.8, True), (0.95, 0.4, False), (0.05, 0.6, False)],
            "off": (0.05, -0.03),
            "radius_off": 0.2,
            "angle_off": 0.4,
        },
    ]
    lookups = []
    for _ in range(64):
        x = rng.uniform(-9, 9)
        complete(e, LOOKUP, f={0: x})
        lookups.append([x, e.retf()])

    cases = []
    for lay in layouts:
        for scale, p in [(1.0, 1.0), (1.7, 2.5), (0.6, 0.7)]:
            for mode in (0, 1, 2):
                setup(e, obj, lay, scale, p, mode)
                n = len(lay["speakers"])
                row = {"layout": lay["name"], "scale": scale, "p": p, "mode": mode, "gain": [], "dist": [], "weight": []}
                complete(e, POS, obj, 0, obj + 0x3F0, obj + 0x3F4)
                row["pos"] = list(e.read(obj + 0x3F0, "f", 2))
                for spk in range(n):
                    complete(e, DIST, obj, 0, spk, 1)
                    row["dist"].append(e.retf())
                    complete(e, WEIGHT, obj, 0, spk)
                    row["weight"].append(e.retf())
                    complete(e, GAIN, obj, 0, spk)
                    row["gain"].append(e.retf())
                cases.append(row)
                print(lay["name"], scale, p, mode, "ok", flush=True)

    out = {
        "sin_table": sin_t,
        "asin_table": asin_t,
        "lookup": lookups,
        "cases": cases,
        "layouts": [
            {"name": l["name"], "source": l["source"], "speakers": l["speakers"], "off": l["off"],
             "radius_off": l["radius_off"], "angle_off": l["angle_off"]}
            for l in layouts
        ],
    }
    with open(dst, "w") as f:
        json.dump(out, f, separators=(",", ":"))


if __name__ == "__main__":
    main()
