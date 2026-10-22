"""Native vectors for two head helpers of the Kontakt 8 Replika mode-1 chain, run under kemu on a head object H.

`141ac4510(H)` (105 B): H+0x98 (f64) = the time field of the head mode, f32 at H+0xa0 + 4 mode for modes 0..3.
`141abd190(H)` (115 B): the base change request. The source H+0x50 (f64) is anchor-dependent: mode 1 is
H+0x0 * 0.25 * H+0x38, mode 2 is H+0x0, mode 3 is H+0x40 * H+0x0; then it is clamped at 1.0. The target H+0x57d0
goes to 0.0, and when the level H+0x57c8 is nonzero the settle flag H+0x57e0 is set and the trigger H+0x57e1 is
cleared (the 16-bit store of 1 at H+0x57e0).

For each seed a random head state is run through each function and the fields it writes are stored with the
inputs. The reset `141abaab0` is not vectored: it writes only the kernel history fields the port does not model.

    nice -n 19 python3 -I tools/re/replika_head_units_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

SAMPLE_TIME = 0x141AC4510
REBASE = 0x141ABD190
H_BYTES = 0x5900
SEEDS = (1, 2, 3, 4, 5, 6)


def sample_time_case(e, seed):
    rnd = random.Random(seed)
    h = e.alloc(H_BYTES)
    mode = rnd.randrange(0, 5)
    times = [rnd.uniform(-2.0, 2.0) for _ in range(4)]
    e.write(h + 0xC0, "i", mode)
    for k, t in enumerate(times):
        e.write(h + 0xA0 + 4 * k, "f", t)
    e.write(h + 0x98, "d", rnd.uniform(-1.0, 1.0))
    before = e.read(h + 0x98, "d")
    e.call(SAMPLE_TIME, h)
    return {"mode": mode, "times": times, "before": before, "s": e.read(h + 0x98, "d")}


def rebase_case(e, seed):
    rnd = random.Random(seed)
    h = e.alloc(H_BYTES)
    mode = rnd.randrange(0, 5)
    anchor = [rnd.uniform(0.0, 3.0), rnd.uniform(0.0, 3.0), rnd.uniform(0.0, 3.0)]
    base_src = rnd.uniform(0.0, 3.0)
    level = rnd.choice((0.0, rnd.uniform(0.1, 1.0)))
    target = rnd.uniform(0.0, 1.0)
    pend, trig = rnd.randrange(2), rnd.randrange(2)
    e.write(h + 0x00, "d", anchor[0])
    e.write(h + 0x38, "d", anchor[1])
    e.write(h + 0x40, "d", anchor[2])
    e.write(h + 0x50, "d", base_src)
    e.write(h + 0xC0, "i", mode)
    e.write(h + 0x57C8, "d", level)
    e.write(h + 0x57D0, "d", target)
    e.write(h + 0x57E0, "B", pend)
    e.write(h + 0x57E1, "B", trig)
    e.call(REBASE, h)
    return {
        "mode": mode,
        "anchor": anchor,
        "base_src_in": base_src,
        "level": level,
        "target_in": target,
        "pend_in": pend,
        "trig_in": trig,
        "base_src": e.read(h + 0x50, "d"),
        "target": e.read(h + 0x57D0, "d"),
        "pend": e.read(h + 0x57E0, "B"),
        "trig": e.read(h + 0x57E1, "B"),
    }


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    doc = {
        "name": "replika-head-units",
        "units": {"141ac4510": "sample_time", "141abd190": "rebase_request"},
        "sample_time": [sample_time_case(e, s) for s in SEEDS],
        "rebase": [rebase_case(e, s) for s in SEEDS],
    }
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    for c in doc["sample_time"]:
        print("sample_time mode %d: s %.6g -> %.6g" % (c["mode"], c["before"], c["s"]))
    for c in doc["rebase"]:
        print("rebase mode %d: src %.6g -> %.6g, target %.3g -> %.3g, pend %d trig %d" % (
            c["mode"], c["base_src_in"], c["base_src"], c["target_in"], c["target"], c["pend"], c["trig"]))


if __name__ == "__main__":
    main()
