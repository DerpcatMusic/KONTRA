"""Native vectors for the Kontakt 8 Replika head mode writer `141aca4a0` (460 B) under kemu, k = 0, 1, 2, 4 and flag 0, 1.

`141aca4a0(H, k, flag)` writes `H+0xc0 = k`, then by k:
- k = 0: the glide target from the byte `H+0x68` (the glide retarget, `+0x57ec` and the glide fields).
- k = 1: `141abbc90(H, H+0x8c)` (the design chain; its tail writes the kernel flags `K+0xc8` = 1 for lane modes 1..3,
  else 2, and resets both taps with `141abaab0`), then with flag: `141aba040(K, 1)` on both taps (the ring clear
  `1407e6d80`), then the glide target from `H+0x69`.
- k = 2: `H+0x270 = 2`, `141abaab0` on the L tap, `H+0x340 = 2`, `141abaab0` on the R tap; with flag,
  `141aba040(K, 1)` on both taps.
- k = 3: calls `141acb100` (not ported, not vectored here).
- every path: `141abd190(H)` (base change request), then `141ac4510(H)` (sample time, tail call).

The taps are `K = H+0x1a8` (L) and `H+0x278` (R); `K+0xc0` is the ring handle (`{begin, end}`). The identity is
closed by emulation: `H+0x268` (= `K_L+0xc0`) equals `H+0x150` and `H+0x338` (= `K_R+0xc0`) equals `H+0x188` after the
head constructor (the stores at 141a7e8de, 141a7eb84, 141a7ebc0), so the mode-1 taps clear the rings the mode-0
reader uses. The vectors put small synthetic rings behind those handles (`H+0x150/0x188` and `H+0x268/0x338`) so the
whole ring can be recorded. Flags `H+0x270` and `H+0x340` are the taps' `+0xc8` fields. k = 3 is excluded.

For each seed a random head state is run through the writer and the fields it reads or writes are stored before and
after. The vector stores the inputs as well, so the port can be run from the same state.

    nice -n 19 python3 -I tools/re/replika_mode_writer_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

MODE_WRITER = 0x141ACA4A0
H_BYTES = 0x5900
RING = 64  # synthetic ring length in samples (the real rings are 2 MB)
SEEDS = (1, 2, 3, 4)
KS = (0, 1, 2, 4)
FLAGS = (0, 1)
RATES = (44100.0, 48000.0, 96000.0)
LANES = (0x2DC0, 0x4240)
POSTS = (0x56C0, 0x5724)
TAPS = {"L": 0x1A8, "R": 0x278}

# Head fields: (offset, format). `B` is a byte, `i` an int32, `f` a float32, `d` a double.
HEAD_FIELDS = {
    "mode": (0xC0, "i"), "lane": (0x8C, "i"), "kernel_l": (0x270, "i"), "kernel_r": (0x340, "i"),
    "flag68": (0x68, "B"), "flag69": (0x69, "B"), "flag6a": (0x6A, "B"),
    "base_src": (0x50, "d"), "level": (0x57C8, "d"), "fade_target": (0x57D0, "d"),
    "pend": (0x57E0, "B"), "trig": (0x57E1, "B"), "s": (0x98, "d"),
    "a0": (0x00, "d"), "a38": (0x38, "d"), "a40": (0x40, "d"),
    "t0": (0xA0, "f"), "t1": (0xA4, "f"), "t2": (0xA8, "f"), "t3": (0xAC, "f"),
    "g_cur": (0x57E8, "f"), "g_target": (0x57EC, "f"), "g_step": (0x57F0, "f"), "g_div": (0x57F8, "f"),
    "g_count": (0x57FC, "i"), "g_count_init": (0x5800, "i"), "g_pend": (0x5804, "B"), "g_fresh": (0x5805, "B"),
}
# Tap fields (offset from K, format). The kernel flag `K+0xc8` is the head's `kernel_l` / `kernel_r`.
TAP_FIELDS = {
    "pos": (0x00, "d"), "wr": (0x08, "i"), "frac": (0x10, "d"), "delta": (0x1C, "i"), "pwr": (0x20, "i"),
    "phase": (0x28, "d"), "idx": (0x30, "i"), "xprev": (0x38, "d"), "pfrac": (0x40, "d"), "offset": (0x48, "d"),
    "s0": (0x50, "f"), "s1": (0x54, "f"), "rate0": (0x90, "d"), "rate1": (0x98, "d"), "rate2": (0xA0, "d"),
    "hist84": (0x84, "d"), "hist8c": (0x8C, "i"),
}
RANDOM = {"d": lambda r: r.uniform(-50.0, 50.0), "f": lambda r: r.uniform(-1.0, 1.0),
          "i": lambda r: r.randrange(-100, 100), "B": lambda r: r.randrange(2)}


def snapshot(e, h, taps, rings):
    out = {name: e.read(h + off, fmt) for name, (off, fmt) in HEAD_FIELDS.items()}
    for side, k in taps.items():
        for name, (off, fmt) in TAP_FIELDS.items():
            out["%s_%s" % (side, name)] = e.read(k + off, fmt)
    out["ring_l"] = list(e.read(rings[0], "f", RING))
    out["ring_r"] = list(e.read(rings[1], "f", RING))
    return out


def case(e, seed, k, flag):
    rnd = random.Random(seed * 1000 + k * 10 + flag)
    h = e.alloc(H_BYTES)
    e.uc.mem_write(h, bytes(H_BYTES))
    e.write(h + 0x8C, "i", rnd.randrange(0, 5))  # lane mode, the kernel flag source of 141abbc90
    e.write(h + 0x270, "i", rnd.randrange(0, 3))  # kernel flags (the taps' +0xc8)
    e.write(h + 0x340, "i", rnd.randrange(0, 3))
    for off in (0x68, 0x69, 0x6A):
        e.write(h + off, "B", rnd.randrange(2))
    for off in (0x00, 0x38, 0x40, 0x50, 0x98):
        e.write(h + off, "d", rnd.uniform(0.0, 3.0))
    for off in (0xA0, 0xA4, 0xA8, 0xAC):
        e.write(h + off, "f", rnd.uniform(-2.0, 2.0))
    e.write(h + 0x57C8, "d", rnd.choice((0.0, rnd.uniform(0.1, 1.0))))
    e.write(h + 0x57D0, "d", rnd.uniform(0.0, 1.0))
    e.write(h + 0x57E0, "B", rnd.randrange(2))
    e.write(h + 0x57E1, "B", rnd.randrange(2))
    rate = rnd.choice(RATES)
    for lane in LANES:
        e.write(h + lane + 0x1450, "f", rate)  # the design chain reads the rate from the lane windows
    for post in POSTS:
        e.write(h + post + 0x5C, "f", rate)
    cur = rnd.choice((0.0, 1.0, rnd.uniform(0.0, 1.0)))  # 0 or 1 exercises the no-change branch
    e.write(h + 0x57E8, "f", cur)
    e.write(h + 0x57EC, "f", rnd.uniform(0.0, 1.0))
    e.write(h + 0x57F0, "f", rnd.uniform(-0.5, 0.5))
    e.write(h + 0x57F8, "f", rnd.uniform(1.0, 4000.0))
    e.write(h + 0x5800, "i", rnd.randrange(1, 4000))
    e.write(h + 0x57FC, "i", rnd.randrange(-5, 50))
    e.write(h + 0x5804, "B", rnd.randrange(2))
    e.write(h + 0x5805, "B", rnd.randrange(2))

    # Synthetic rings behind the mode-0 handles (H+0x150 / H+0x188) and the tap handles (H+0x268 / H+0x338).
    rings, handles = [], []
    for _ in range(2):
        data = e.alloc(RING * 4)
        vals = [rnd.uniform(-1.0, 1.0) for _ in range(RING)]
        e.write(data, "f", *vals)
        handle = e.alloc(0x18)
        e.write(handle, "Q", data)
        e.write(handle + 8, "Q", data + RING * 4)
        rings.append(data)
        handles.append(handle)
    e.write(h + 0x150, "Q", handles[0])
    e.write(h + 0x188, "Q", handles[1])
    e.write(h + 0x268, "Q", handles[0])
    e.write(h + 0x338, "Q", handles[1])
    taps = {side: h + off for side, off in TAPS.items()}
    for k_ in taps.values():
        for name, (off, fmt) in TAP_FIELDS.items():
            e.write(k_ + off, fmt, RANDOM[fmt](rnd))

    init = snapshot(e, h, taps, rings)
    e.call(MODE_WRITER, h, k, flag)
    end = snapshot(e, h, taps, rings)
    assert end["mode"] == k, end["mode"]
    return {"seed": seed, "k": k, "flag": flag, "lane": init["lane"], "init": init, "end": end}


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    cases = [case(e, s, k, f) for s in SEEDS for k in KS for f in FLAGS]
    doc = {
        "name": "replika-mode-writer",
        "units": {"141aca4a0": "mode writer (k = 0, 1, 2, 4)", "141abbc90": "design (k = 1, tail flags)",
                  "141abaab0": "tap rates reset", "141aba040": "tap reset (flag 1: ring clear 1407e6d80)",
                  "141abd190": "rebase request", "141ac4510": "sample time"},
        "ring_len": RING,
        "cases": cases,
    }
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    for c in cases:
        i, en = c["init"], c["end"]
        print("k %d flag %d seed %d: mode %d, kernel %d/%d, glide %.3g -> %.3g, step %.3g, count %d -> %d" % (
            c["k"], c["flag"], c["seed"], en["mode"], en["kernel_l"], en["kernel_r"], i["g_target"],
            en["g_target"], en["g_step"], i["g_count"], en["g_count"]))


if __name__ == "__main__":
    main()
