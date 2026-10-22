"""Function-level native vectors for the Kontakt 8 Replika mode-1 write filter under kemu (piece 3a).

Unit: `141aa3c60` (`0141aa0000.c` 2165-2214), `void f(lane, float x)`; x in xmm1. The lane object is the real
write-side object of head 0 (`H+0x2dc0` L, `H+0x4240` R, 0x1480 bytes, MT state at `+0xc0`) after a run of
index 28 = 0.3 (mode 1, kernel 1) on noise, read from the core object. Each sample: the lane object is the
native state, the output is `+0xa4` (the second section's `lp`, verified as XMM0 after the call).

    nice -n 19 python3 -I tools/re/replika_mode1_write_units_kemu.py KONTAKT8_EXE OUT_DIR

The vector stores the whole lane object as u32 words (1312) so the Rust test can read every field it needs
by its offset and rebuild the MT ring (`+0xc0` index, `+0xc4..+0x1440` ring of 1248, `+0x1444` mask).
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402
from replika_mode1_units_kemu import noise_stream  # noqa: E402

WRITE = 0x141AA3C60
HEAD0 = 0xAC0
LANE_L, LANE_R = HEAD0 + 0x2DC0, HEAD0 + 0x4240
LANE_BYTES = 0x1480
STEPS = 512


def run_write(e, obj, xs):
    ys = []
    for x in xs:
        e.call(WRITE, obj, 0, f={1: x})
        y_ret = e.retf()
        y_field = e.read(obj + 0xA4, "f")
        assert y_ret == y_field, (y_ret, y_field)  # XMM0 is the +0xa4 field
        ys.append(y_field)
    return ys


def main():
    exe, out_dir = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    core = rk.Core(e)
    for i, v in sorted(rk.DEFAULTS.items()):
        core.set(i, v)
    core.prepare(float(rk.RATE))
    core.set(28, 0.3)
    stim = rk.noise()
    for q in range(128):
        span = slice(q * rk.QUANTUM, (q + 1) * rk.QUANTUM)
        core.quantum(stim[0][span], stim[1][span])
    for name, addr in (("l", LANE_L), ("r", LANE_R)):
        state = bytes(e.uc.mem_read(core.obj + addr, LANE_BYTES))
        obj = e.alloc(LANE_BYTES)
        e.uc.mem_write(obj, state)
        xs = [rk.f32(x) for x in noise_stream(2701 + (name == "r"))][:STEPS]
        ys = run_write(e, obj, xs)
        words = list(struct.unpack("<%dI" % (LANE_BYTES // 4), state))
        doc = {"name": "mode1-write-" + name, "units": "141aa3c60", "words": words, "x": xs, "y": ys}
        path = os.path.join(out_dir, "replika-native-mode1-write-%s.json" % name)
        with open(path, "w") as f:
            json.dump(doc, f, separators=(",", ":"))
        print("%s: y max %.6g min %.6g" % (name, max(ys), min(ys)), flush=True)


if __name__ == "__main__":
    main()
