"""Native vectors for the Kontakt 8 Replika read tap (`141ab87d0`, section update `14199dcc0`).

`141ab87d0(K, x)` (x in xmm1, double) returns the interpolated ring sample at `K.pos - x`. It writes the
section freq block and state, the window `[I + 3 - n, I + 2]` of the ring (smoothed in place by the section,
n = min(15, trunc(phase + x_prev - x + offset) - idx)), and the position fields. K is a 0x100-byte object
(H+0x1a8 layout); its ring data pointer is reached through the handle at K+0xc0 (`*(*(K+0xc0))`).

For each seed a random ring and a random x walk are run through kemu. The vector stores the initial and final
object words (the fields the port reads and writes), the ring before and after, the x values and the returns.

    nice -n 19 python3 -I tools/re/replika_tap_read_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

TAP = 0x141AB87D0
OBJ_BYTES = 0x100
RATE = 1.0 / 48000.0
STEPS = 240
SEEDS = (1, 2, 3)


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def obj_fields(e, k):
    return {
        "pos": e.read(k + 0x00, "d"),
        "phase": e.read(k + 0x28, "d"),
        "idx": e.read(k + 0x30, "i"),
        "x_prev": e.read(k + 0x38, "d"),
        "s0": e.read(k + 0x50, "f"),
        "s1": e.read(k + 0x54, "f"),
        "c2": e.read(k + 0x58, "f"),
        "c3": e.read(k + 0x5C, "f"),
        "c4": e.read(k + 0x60, "f"),
        "bp": e.read(k + 0x64, "f"),
        "lp": e.read(k + 0x68, "f"),
        "d": e.read(k + 0x6C, "f"),
        "v": e.read(k + 0x70, "f"),
        "f": e.read(k + 0x74, "f"),
    }


def ring_words(e, ring, n):
    return list(struct.unpack("<%dI" % n, bytes(e.uc.mem_read(ring, 4 * n))))


def case(e, seed):
    rnd = random.Random(seed)
    mask = (32, 64, 128)[seed % 3] - 1
    size = mask + 1
    k = e.alloc(OBJ_BYTES)
    handle = e.alloc(8)
    ring = e.alloc(4 * size)
    e.write(handle, "Q", ring)
    e.write(k + 0xC0, "Q", handle)
    e.write(k + 0x18, "i", mask)
    e.write(k + 0x00, "d", 20.0 + rnd.random() * 8.0)
    e.write(k + 0x28, "d", rnd.random() * 2.0)
    e.write(k + 0x30, "i", rnd.randrange(size))
    e.write(k + 0x38, "d", 0.0)
    e.write(k + 0x48, "d", rnd.random() * 3.0)
    e.write(k + 0x50, "f", 0.0)
    e.write(k + 0x54, "f", 0.0)
    e.write(k + 0x7C, "f", 0.1 + rnd.random() * 0.8)  # damping k (preset by the object)
    e.write(k + 0x80, "f", RATE)                    # rate factor (preset)
    for i in range(size):
        e.write(ring + 4 * i, "f", f32(rnd.uniform(-0.9, 0.9)))
    ring0 = ring_words(e, ring, size)
    init = obj_fields(e, k)
    init_words = list(struct.unpack("<%dI" % (OBJ_BYTES // 4), bytes(e.uc.mem_read(k, OBJ_BYTES))))

    xs, rets = [], []
    x = rnd.uniform(0.0, 4.0)
    for _ in range(STEPS):
        step = rnd.uniform(0.0, 2.0)
        if rnd.random() < 0.1:
            step = -rnd.uniform(0.0, 3.0)  # backward jumps exercise the wrap branch
        x = x + step
        xs.append(x)
        e.call(TAP, k, d={1: x})
        rets.append(e.retf())

    end_fields = obj_fields(e, k)
    end_words = list(struct.unpack("<%dI" % (OBJ_BYTES // 4), bytes(e.uc.mem_read(k, OBJ_BYTES))))
    return {
        "seed": seed,
        "mask": mask,
        "init": init,
        "init_words": init_words,
        "ring0": ring0,
        "xs": xs,
        "rets": rets,
        "end": end_fields,
        "end_words": end_words,
        "ring1": ring_words(e, ring, size),
    }


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    cases = [case(e, s) for s in SEEDS]
    doc = {"name": "replika-tap-read", "units": "141ab87d0 (14199dcc0)", "cases": cases}
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    for c in cases:
        print("seed %d mask %d rets min %.4g max %.4g" % (c["seed"], c["mask"], min(c["rets"]), max(c["rets"])))


if __name__ == "__main__":
    main()
