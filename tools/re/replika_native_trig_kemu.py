"""Native 4-lane trig of the Kontakt 8 Replika mode-1 path, under kemu (piece 3a, trig).

`14199ae00` (sin): xmm0 = sin(v_i), xmm1 = |v_i|; large-argument branch for |v| >= 25966 (not sampled).
`14199b1d0` (cos): xmm0 = cos(v_i), xmm1 = sin(|v_i| - k pi/2), k = round(2|v_i| / pi); branch for |v| >= 18838.
Inputs are sampled below both thresholds, four lanes per call, so the Rust port is checked bit for bit.

    nice -n 19 python3 -I tools/re/replika_native_trig_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import math
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402
from unicorn.x86_const import UC_X86_REG_XMM0, UC_X86_REG_XMM1  # noqa: E402

SIN = 0x14199AE00
COS = 0x14199B1D0
CALLS = 512


def lanes(e, reg):
    return list(struct.unpack("<4f", e.uc.reg_read(reg).to_bytes(16, "little")))


def main():
    exe, out = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    buf = e.alloc(16)
    rng = random.Random(20261022)
    inputs = []
    for _ in range(CALLS):
        # Log-uniform magnitude over the write path's range and beyond (up to the 14199b1d0 threshold).
        row = []
        for _ in range(4):
            mag = math.exp(rng.uniform(math.log(1e-4), math.log(18000.0)))
            row.append(rk.f32(math.copysign(mag, rng.uniform(-1, 1))))
        inputs.append(row)
    sin_out, abs_out, cos_out, fold_out = [], [], [], []
    for row in inputs:
        e.write(buf, "f", *row)
        e.call(SIN, buf)
        sin_out.append(lanes(e, UC_X86_REG_XMM0))
        abs_out.append(lanes(e, UC_X86_REG_XMM1))
        e.write(buf, "f", *row)
        e.call(COS, buf)
        cos_out.append(lanes(e, UC_X86_REG_XMM0))
        fold_out.append(lanes(e, UC_X86_REG_XMM1))
    doc = {"units": "14199ae00 sin, 14199b1d0 cos", "in": inputs, "sin": sin_out, "abs": abs_out,
           "cos": cos_out, "fold": fold_out}
    with open(out, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    print("wrote %d calls to %s" % (CALLS, out))


if __name__ == "__main__":
    main()
