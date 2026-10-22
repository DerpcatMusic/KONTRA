"""Native vectors for the Kontakt 8 Replika kernel-object reset (`141aba040` with flag 0, and `141abaab0` inside it).

`141aba040(K, flag)` (166 B) zeroes the driver and kernel fields of the tap object K (`K+0x00`, `+0x08`, `+0x10`,
`+0x1c..0x23`, `+0x28`, `+0x30`, `+0x38`, `+0x40`, `+0x48`, `+0x50`, `+0x54`), then calls `141abaab0` (zeroes `+0x84` and
`+0x8c`, sets the rates `+0x90`, `+0x98`, `+0xa0` to 1.0). With flag != 0 it also calls `1407e6d80` on the ring handle
`K+0xc0` (not vectored: the mode writer passes 0 on the selector path). Only the flag-0 path is checked; the fields
it does not touch are checked unchanged.

    nice -n 19 python3 -I tools/re/replika_reset_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

RESET = 0x141ABA040
OBJ_BYTES = 0x100
SEEDS = tuple(range(1, 7))

FIELDS = {
    "pos": (0x00, "d"), "wr": (0x08, "i"), "frac": (0x10, "d"), "delta": (0x1C, "i"), "pwr": (0x20, "i"),
    "phase": (0x28, "d"), "idx": (0x30, "i"), "xprev": (0x38, "d"), "pfrac": (0x40, "d"), "offset": (0x48, "d"),
    "s0": (0x50, "f"), "s1": (0x54, "f"), "damp": (0x7C, "f"), "rate": (0x80, "f"),
    "rate0": (0x90, "d"), "rate1": (0x98, "d"), "rate2": (0xA0, "d"),
    "sec_c2": (0x58, "f"), "sec_c3": (0x5C, "f"), "sec_c4": (0x60, "f"), "bp": (0x64, "f"), "lp": (0x68, "f"),
}


def snapshot(e, k):
    return {name: e.read(k + off, fmt) for name, (off, fmt) in FIELDS.items()}


def case(e, seed):
    rnd = random.Random(seed)
    k = e.alloc(OBJ_BYTES)
    e.uc.mem_write(k, bytes(OBJ_BYTES))
    for name, (off, fmt) in FIELDS.items():
        if fmt == "d":
            e.write(k + off, "d", rnd.uniform(-50.0, 50.0))
        elif fmt == "f":
            e.write(k + off, "f", rnd.uniform(-1.0, 1.0))
        else:
            e.write(k + off, "i", rnd.randrange(-100, 100))
    e.write(k + 0x84, "d", rnd.uniform(-1.0, 1.0))  # the deposit's history, zeroed by 141abaab0
    e.write(k + 0x8C, "i", rnd.randrange(1, 9))
    init = snapshot(e, k)
    e.call(RESET, k, 0)
    return {"seed": seed, "init": init, "end": snapshot(e, k), "hist84_end": e.read(k + 0x84, "d"),
            "hist8c_end": e.read(k + 0x8C, "i")}


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    doc = {"name": "replika-kernel-reset", "units": "141aba040 (flag 0), 141abaab0",
           "cases": [case(e, s) for s in SEEDS]}
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    print("%d cases, hist +0x84 after reset: %s" % (len(doc["cases"]), [c["hist84_end"] for c in doc["cases"]]))


if __name__ == "__main__":
    main()
