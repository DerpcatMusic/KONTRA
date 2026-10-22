"""Native vectors for the Kontakt 8 Replika mode-1 deposit (`141accfb0`, 337 B).

`141accfb0(K, _, x)` (x in xmm2, f32) shifts the object's three-sample history (`+0x84` x_n, `+0x88` x_(n-1),
`+0x8c` x_(n-2)) and then overwrites `count` ring taps (`+0x1c`) at `(write - i) & mask` (`write` = `+0x8`,
`mask` = `+0x18`) with the quadratic `(t A + B) t + x_n`, t = -(i + off) (`off` = `+0x10`), where A and B come
from the history, a = -`+0x90` and b = `+0x98`. The ring is the f32 array behind the handle at `K+0xc0`.

For each seed, a random ring, random a, b, offset, count and write index; then a random walk of x values. The
vector stores the initial object words, the ring before and after each call, x, and the object at the end.

    nice -n 19 python3 -I tools/re/replika_deposit_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

DEPOSIT = 0x141ACCFB0
OBJ_BYTES = 0x100
STEPS = 24
SEEDS = (1, 2, 3)


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


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
    e.write(k + 0x08, "i", rnd.randrange(size))  # write index
    e.write(k + 0x10, "d", rnd.random())         # sub-sample offset
    e.write(k + 0x18, "i", mask)
    e.write(k + 0x1C, "i", rnd.randrange(0, 16))  # count of taps
    e.write(k + 0x84, "f", 0.0)
    e.write(k + 0x88, "f", 0.0)
    e.write(k + 0x8C, "f", 0.0)
    e.write(k + 0x90, "d", rnd.uniform(0.3, 2.0))  # a: the node position is -a
    e.write(k + 0x98, "d", rnd.uniform(0.1, 1.0))  # b: the node spacing
    for i in range(size):
        e.write(ring + 4 * i, "f", f32(rnd.uniform(-0.9, 0.9)))
    init_words = list(struct.unpack("<%dI" % (OBJ_BYTES // 4), bytes(e.uc.mem_read(k, OBJ_BYTES))))
    ring0 = ring_words(e, ring, size)

    xs, ring_steps = [], []
    for _ in range(STEPS):
        x = f32(rnd.uniform(-1.0, 1.0))
        xs.append(x)
        e.call(DEPOSIT, k, 0, f={2: x})
        ring_steps.append(ring_words(e, ring, size))
    end_words = list(struct.unpack("<%dI" % (OBJ_BYTES // 4), bytes(e.uc.mem_read(k, OBJ_BYTES))))
    return {
        "seed": seed,
        "mask": mask,
        "init_words": init_words,
        "ring0": ring0,
        "xs": xs,
        "ring_steps": ring_steps,
        "end_words": end_words,
    }


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    cases = [case(e, s) for s in SEEDS]
    doc = {"name": "replika-deposit", "units": "141accfb0", "cases": cases}
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    for c in cases:
        flat = [struct.unpack("<f", struct.pack("<I", w))[0] for s in c["ring_steps"] for w in s]
        print("seed %d mask %d ring range %.4g..%.4g" % (c["seed"], c["mask"], min(flat), max(flat)))


if __name__ == "__main__":
    main()
