"""Native vectors for the Kontakt 8 Replika mode-1 design chain (`141abbc90`, lane `141ac3fd0`).

`141abbc90(H, mode)` sets the write lanes (`H+0x2dc0`, `H+0x4240`, 0x1480 bytes) and the post filters
(`H+0x56c0`, `H+0x5724`, stride 0x64) from the mode in edx and the sample rate stored in each object
(`lane+0x1450`, `post+0x5c`). The head object is zeroed, the rate is written to those fields, and the call runs
under kemu for every rate and mode 0..3. Stored per case: the lane windows `+0x50..+0xc0` (28 words) and
`+0x1450..+0x1480` (12 words), and the post window `+0x20..+0x64` (17 words), as u32 words.

    nice -n 19 python3 -I tools/re/replika_mode1_design_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

DESIGN = 0x141ABBC90
HEAD_BYTES = 0x5800
LANES = (0x2DC0, 0x4240)
POSTS = (0x56C0, 0x5724)
RATES = (44100, 48000, 96000)
MODES = (0, 1, 2, 3)


def words(e, addr, n):
    return list(struct.unpack("<%dI" % n, bytes(e.uc.mem_read(addr, 4 * n))))


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    cases = []
    for rate in RATES:
        for mode in MODES:
            head = e.alloc(HEAD_BYTES)
            for lane in LANES:
                e.write(head + lane + 0x1450, "f", float(rate))
            for post in POSTS:
                e.write(head + post + 0x5C, "f", float(rate))
            e.call(DESIGN, head, mode)
            cases.append({
                "mode": mode,
                "fs": float(rate),
                "lanes": [
                    {"w": words(e, head + lane + 0x50, 28), "p": words(e, head + lane + 0x1450, 12)}
                    for lane in LANES
                ],
                "posts": [{"w": words(e, head + post + 0x20, 17)} for post in POSTS],
            })
            print("rate %d mode %d done" % (rate, mode), flush=True)
    doc = {"name": "mode1-design", "units": "141abbc90", "cases": cases}
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))


if __name__ == "__main__":
    main()
