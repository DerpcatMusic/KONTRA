"""Function-level native vectors for the Kontakt 8 Replika mode-1 chain under kemu.

Each unit is one decompiled function called on its own state, so a Rust port can be checked without the whole
core. The state is the real one: the head-0 post-filter object (`H+0x56c0` L, `H+0x5724` R) after a run of
index 28 = 0.3 (mode 1, kernel 1) on noise, read from the core object by the harness (not a Kontakt render).

    nice -n 19 python3 -I tools/re/replika_mode1_units_kemu.py KONTAKT8_EXE OUT_DIR

Units:
- postfilter (`141aa3e00`, `0141aa0000.c` 2217-2248): `float f(obj, uint x_bits)`; x is passed in xmm1 (the
  decompile types it as uint because it takes fabs by mask). Writes the envelope `+0x20`, the section at
  `+0x28` (`140d947a0`, see `dsp/tpt_svf.rs` `Section::tick`). Returns `+0x54 x +0x3c x g`.
"""
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

POSTFILTER = 0x141AA3E00
HEAD0 = 0xAC0
POST_L, POST_R = HEAD0 + 0x56C0, HEAD0 + 0x5724
OBJ_BYTES = 0x64  # `+0x00` .. `+0x63`: env (+0x20), coef (+0x24), section (+0x28..+0x48), bp (+0x3c), gain (+0x54)
FIELDS = {"env": 0x20, "coef": 0x24, "s0": 0x28, "s1": 0x2C, "c2": 0x30, "c3": 0x34, "c4": 0x38,
          "bp": 0x3C, "lp": 0x40, "gain": 0x54}
STEPS = 512


def noise_stream(seed):
    """Uniform noise whose level steps up every 128 samples (the envelope and T4 gain both move)."""
    rng = random.Random(seed)
    return [0.0 if n % 97 == 0 else rng.uniform(-0.25 * (1 + n // 128), 0.25 * (1 + n // 128))
            for n in range(STEPS)]


def run_postfilter(e, obj, xs):
    ys = []
    for x in xs:
        e.call(POSTFILTER, obj, 0, f={1: x})
        ys.append(e.retf())
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
    for name, addr in (("l", POST_L), ("r", POST_R)):
        state = bytes(e.uc.mem_read(core.obj + addr, OBJ_BYTES))
        obj = e.alloc(OBJ_BYTES)
        e.uc.mem_write(obj, state)
        init = {k: struct_f32(state, off) for k, off in FIELDS.items()}
        xs = [rk.f32(x) for x in noise_stream(1408 + (name == "r"))]
        ys = run_postfilter(e, obj, xs)
        doc = {"name": "mode1-postfilter-" + name, "units": "141aa3e00", "init": init, "x": xs, "y": ys}
        path = os.path.join(out_dir, "replika-native-mode1-postfilter-%s.json" % name)
        with open(path, "w") as f:
            json.dump(doc, f, separators=(",", ":"))
        print("%s: env %.6g coef %.6g gain %.6g; y max %.6g" % (
            name, init["env"], init["coef"], init["gain"], max(abs(y) for y in ys)), flush=True)


def struct_f32(state, off):
    return struct.unpack_from("<f", state, off)[0]


if __name__ == "__main__":
    main()
