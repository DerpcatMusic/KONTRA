"""sv_native_vectors.py OUT.json: native impulse outputs of the Kontakt 8 SV2 and SV4 process
functions, run under kemu (no program is executed), for the res .9 cases of cascade.rs.

    python3 tools/re/sv_native_vectors.py crates/sampler-core/src/dsp/filter/cascade_sv_res9_vectors.json

Process 0x140b02400 (SV2, one stage) and 0x140b02790 (SV4, two stages). The block the process
reads is filled with the setters' targets (g, R, gain, l) as the decompile gives them: SV2 g from
the cutoff table with `note = min(115 u + 20, 160)`, SV4 from the same table with the unclamped
note; `l` as `0x140b04ee0` (SV2) and `0x140b05030` (SV4). The table is `2^(m/60 - 20)` at the
integers, linearly interpolated (the binary's table, not an exact pow). Input: unit impulse.
"""
import json
import math
import os
import struct
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402

EXE = "/data/dev/workspace/re-binaries/Kontakt8.exe"
SV2, SV4, SV6 = 0x140B02400, 0x140B02790, 0x140B02CF0
N = 512


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def table_hz(note):
    m = note * 5.0 + 1381.881591796875
    i = math.floor(m)
    t = m - i
    w = lambda k: 2.0 ** (k / 60.0 - 20.0)
    return f32(w(i) + t * (w(i + 1) - w(i)))


def tan_g(f, t):
    x = ((t * t * 12.98788070678711) * f) * f + 3.2898683547973633
    y = ((x * f) * t) * f * t + 1.0
    return ((y * f) * math.pi) * t


def sv_r(v):
    q = 1.0 - v
    return 1.0 - q * q * q


def run(proc, count, mode, block_values, mix_slots):
    e = Emu(EXE)
    obj = e.alloc(0x400)
    e.write(obj + 0xC, "I", count)
    for addr, vals in block_values:
        e.write(obj + addr, "f", *vals)
    for addr, v in mix_slots(mode):
        e.write(obj + addr, "f", v)
    inp, out = e.alloc(4 * N), e.alloc(4 * N)
    imp = np.zeros(N, dtype=np.float32)
    imp[0] = 1.0
    e.uc.mem_write(inp, imp.tobytes())
    arr_in, arr_out = e.alloc(8), e.alloc(8)
    e.write(arr_in, "Q", inp)
    e.write(arr_out, "Q", out)
    e.call(proc, obj, arr_in, arr_out, N)
    return np.frombuffer(e.uc.mem_read(out, 4 * N), dtype=np.float32).astype(np.float64)


def sv2(mode, cutoff, res):
    note = min(cutoff * 115.0 + 20.0, 160.0)
    g = tan_g(table_hz(note), 1.0 / 48000.0)
    r = sv_r(res)
    n = min(max(note * 0.007462686393409967, 0.0), 1.0)
    limit = 600.0 - n * 576.0
    gain = 1.0 - (r ** 5) * 0.5
    block = [(0x260, (1.0, g, r, limit, gain)), (0x280, (0.0,) * 8)]
    mix = lambda m: [(0x44, 1.0 if m == 0 else 0.0), (0x48, 1.0 if m == 1 else 0.0), (0x4C, 1.0 if m == 2 else 0.0)]
    return run(SV2, 1, mode, block, mix)


def sv4(mode, cutoff, res):
    note = cutoff * 115.0 + 20.0
    g = tan_g(table_hz(note), 1.0 / 48000.0)
    r = sv_r(res)
    c = [0.77, 0.9334, 0.77, 0.88][mode]
    n = min(note, 134.0) * 0.007462686393409967
    limit = 1600.0 - (2346.0 - abs(n) * 782.0) * n
    block = [(0x2C0, (g, r, 1.0 - r * c, limit)), (0x2D0, (0.0,) * 4)]
    mix = lambda m: [(0x4C, 1.0 if m == 0 else 0.0), (0x50, 1.0 if m == 1 else 0.0),
                     (0x54, 1.0 if m == 2 else 0.0), (0x58, 1.0 if m == 3 else 0.0)]
    return run(SV4, 1, mode, block, mix)


def sv6(mode, cutoff, res):
    # 0x140b02cf0: three stages; block 0x380 = [g, R, gain, l]; coefficient slots shared with SV4.
    note = min(cutoff * 115.0 + 20.0, 160.0)
    g = tan_g(table_hz(note), 1.0 / 48000.0)
    r = sv_r(res * 0.9)
    s_ = {0: (1.2125001 - abs(r) * 0.2425) * r, 2: (1.2125001 - abs(r) * 0.2425) * r,
          1: ((2.0 - r) * r) * 0.991, 3: (1.482 - abs(r) * 0.494) * r}[mode]
    n = min(note, 128.0) * 0.0078125
    limit = 1600.0 - ((2.0 - n) * n) * 1576.0
    block = [(0x380, (g, r, 1.0 - s_, limit)), (0x390, (0.0,) * 4)]
    mix = lambda m: [(0x4C, 1.0 if m == 0 else 0.0), (0x50, 1.0 if m == 1 else 0.0),
                     (0x54, 1.0 if m == 2 else 0.0), (0x58, 1.0 if m == 3 else 0.0)]
    return run(SV6, 1, mode, block, mix)


# (class, type, function, mode, cutoff, res)
CASES = [
    ("SV2", "LP2", sv2, 0, 0.5, 0.9), ("SV2", "BP2", sv2, 1, 0.5, 0.9), ("SV2", "HP2", sv2, 2, 0.5, 0.9),
    ("SV2", "BP2", sv2, 1, 1.0, 0.0),
    ("SV4", "LP4", sv4, 0, 0.5, 0.9), ("SV4", "BP4", sv4, 1, 0.5, 0.9), ("SV4", "HP4", sv4, 2, 0.5, 0.9),
    ("SV4", "NOTCH4", sv4, 3, 0.5, 0.9),
    ("SV6", "LP6", sv6, 0, 0.5, 0.9), ("SV6", "BP6", sv6, 1, 0.5, 0.9), ("SV6", "HP6", sv6, 2, 0.5, 0.9),
    ("SV6", "NOTCH6", sv6, 3, 0.5, 0.9),
]


def main(argv):
    if len(argv) != 2:
        raise SystemExit(__doc__)
    cases = []
    for cls, name, fn, mode, cutoff, res in CASES:
        h = fn(mode, cutoff, res)
        cases.append({"class": cls, "setting": {"type": name, "cutoff": cutoff, "res": res},
                      "impulse": [float(f"{x:.9g}") for x in h]})
    with open(argv[1], "w") as fh:
        json.dump(cases, fh)


if __name__ == "__main__":
    main(sys.argv)
