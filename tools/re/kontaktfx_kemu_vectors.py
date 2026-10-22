"""KONTRAFX filter vectors (Dual SKF saved 130, BlueMonark saved 140..143) under kemu.

Runs the unchanged Kontakt 8 cores: BFilter factory steps, setters, prepare, the 32-frame
tick and process. Model: tools/re/daft_kemu_vectors.py.

    python3 -I tools/re/kontaktfx_kemu_vectors.py KONTAKT8_EXE OUT_DIR

OUT_DIR/kontaktfx_vectors.json is read first. Its Simple LP/HP and BlueMonark cases are carried over
unchanged (no regenerated Monark cases yet: see the validation report). Dual SKF mono cases (48 kHz,
the schema read by `matches_the_native_classes`) are rewritten from kemu. OUT_DIR/
kontaktfx_stereo_vectors.json gets stereo Dual SKF noise at 44.1, 48 and 96 kHz, steady and with
knobs moving at 32-frame block boundaries.

Native layout (decompile, Kontakt 8):
- Dual SKF: BFilter object 0x51A0 bytes. Factory FUN_1407c95c0 runs the BFilter base (FUN_140a856c0),
  then the wrapper ctor FUN_140ae5f40 at +0x210. The core is at +0x420. Channel count is the
  core's uint at +0xc. Process FUN_140b000c0 reads planar channel pointers (in table, out table).
- BlueMonark: BFilter object 0x5490 bytes. Factory FUN_1407c92f0 runs the wrapper ctor FUN_1407f0ed0
  on the object. The core is at +0x210. setType FUN_140af0020(obj, code) sets the mode:
  36 LP1 = mode 2, 37 LP2 = mode 1, 38 LP4 = mode 0, 39 BP = mode 3.

Knob order in the core: Dual SKF HP cutoff, HP resonance, LP cutoff, LP resonance, drive (stored
2v - 1). Monark cutoff, resonance, input gain (3v + 1), modulation (2v - 1), feedback.
"""

import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

TABLE_INIT = 0x1404D14D0
BFILTER_BASE = 0x140A856C0
DUAL = {
    "name": "DualSKF", "obj": 0x51A0, "ctor": 0x140AE5F40, "ctor_arg": 0x210, "core": 0x420,
    "setter": 0x140B059C0, "prepare": 0x140AFD350, "tick": 0x140B03630, "process": 0x140B000C0,
}
MONARK = {
    "name": "BlueMonark", "obj": 0x5490, "ctor": 0x1407F0ED0, "ctor_arg": 0, "core": 0x210,
    "set_type": 0x140AF0020, "setter": 0x140B05740, "prepare": 0x140AFCC40, "tick": 0x140B032F0,
    "process": 0x140AFD9C0,
}
BLOCK = 32
MONARK_MODE_CODE = {2: 0x24, 1: 0x25, 0: 0x26, 3: 0x27}  # FUN_140af0020 codes (LP1, LP2, LP4, BP)


def complete(e, *args, **kwargs):
    e.call(*args, limit=4_000_000, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


def build(e, spec, rate, channels, mode=None):
    """A prepared-to-run core: factory steps, channel count, then the mode (Monark)."""
    obj = e.alloc(spec["obj"])
    if spec is DUAL:
        complete(e, BFILTER_BASE, obj)
    complete(e, spec["ctor"], obj + spec["ctor_arg"])
    core = obj + spec["core"]
    e.write(core + 0xC, "I", channels)
    if spec is MONARK:
        complete(e, spec["set_type"], obj, MONARK_MODE_CODE[mode])
    return core


def render(e, spec, *, rate, channels, mode, variant, schedule, left, right, tables):
    """One filter run: knobs per 32-frame block (written only when they change), tick, process."""
    in_table, out_table, in_l, in_r, out_l, out_r = tables
    core = build(e, spec, rate, channels, mode)
    first = schedule[0]
    for index, value in enumerate(first):
        complete(e, spec["setter"], core, index, f={2: value})
    complete(e, spec["prepare"], core, 0, BLOCK, f={1: float(rate)})
    prev = list(first)
    out_left, out_right = [], []
    for block in range(len(left) // BLOCK):
        knobs = schedule[block]
        for index, value in enumerate(knobs):
            if value != prev[index]:
                complete(e, spec["setter"], core, index, f={2: value})
        prev = list(knobs)
        span = slice(block * BLOCK, (block + 1) * BLOCK)
        e.write(in_l, "f", *left[span])
        e.write(in_r, "f", *right[span])
        complete(e, spec["tick"], core)
        complete(e, spec["process"], core, in_table, out_table, BLOCK)
        out_left.extend(e.read(out_l, "f", BLOCK))
        out_right.extend(e.read(out_r, "f", BLOCK))
    return out_left, out_right


def make_tables(e, frames):
    in_table, out_table = e.alloc(16), e.alloc(16)
    in_l, in_r = e.alloc(frames * 4), e.alloc(frames * 4)
    out_l, out_r = e.alloc(frames * 4), e.alloc(frames * 4)
    e.write(in_table, "Q", in_l, in_r)
    e.write(out_table, "Q", out_l, out_r)
    return in_table, out_table, in_l, in_r, out_l, out_r


def mono_cases():
    """The 48 kHz mono cases of the existing Rust test (impulse and +-4 square of period 40)."""
    impulse = [1.0 if n == 0 else 0.0 for n in range(64)]
    square = [4.0 if n % 40 < 20 else -4.0 for n in range(64)]
    a, b = [0.6, 0.3, 0.0, 0.5, 0.0], [0.8, 0.8, 0.5, 1.0, 1.0]
    rows = [("DualSKF", [0.5, 0.3, 0.5, 0.5, 0.5], 0, 0), ("DualSKF", [0.8, 0.8, 0.2, 0.7, 1.0], 0, 0),
            ("DualSKF", [0.3, 0.0, 1.0, 0.0, 0.0], 0, 0)]
    rows += [("BlueMonark", k, mode, 0) for mode in (2, 1, 0, 3) for k in (a, b)]
    rows += [("BlueMonark", b, 2, 1), ("BlueMonark", b, 0, 1), ("BlueMonark", b, 2, 2), ("BlueMonark", b, 0, 2)]
    return [{"class": cls, "knobs": k, "mode": mode, "variant": var, "impulse_in": impulse,
             "square_in": square} for cls, k, mode, var in rows]


def run_mono(e, tables, case, spec):
    schedule = [case["knobs"]] * 2
    res = {}
    for name in ("impulse", "square"):
        frames = case[name + "_in"]
        out_l, _ = render(e, spec, rate=48000, channels=1, mode=case["mode"], variant=case["variant"],
                          schedule=schedule, left=frames, right=frames, tables=tables)
        res[name] = out_l
    return res


def rel_error(got, want):
    peak = max(abs(v) for v in want) or 1.0
    return max(abs(a - b) for a, b in zip(got, want)) / peak


def stereo_cases():
    """Stereo: independent L/R noise, steady and moving knobs (one knob set per 32-frame block).

    Dual SKF at 44.1, 48 and 96 kHz. BlueMonark mode 0 (LP4) variant 0 at the same rates: the only
    Monark mode and variant that reproduce the stored mono cases under kemu. Other modes and variants
    are open (see the unit doc) and get no stereo vectors.
    """
    rng = random.Random(0x5EED)
    frames = 8 * BLOCK
    left = [rng.uniform(-0.5, 0.5) for _ in range(frames)]
    right = [rng.uniform(-0.5, 0.5) for _ in range(frames)]
    dual_steady = [[0.5, 0.3, 0.5, 0.5, 0.5]] * 8
    dual_moving = [[0.3 + 0.05 * b, 0.4, 0.6 - 0.04 * b, 0.2 + 0.03 * b, 0.5 - 0.02 * b] for b in range(8)]
    mon_steady = [[0.6, 0.3, 0.0, 0.5, 0.0]] * 8
    mon_moving = [[0.3 + 0.05 * b, 0.6 - 0.05 * b, 0.1 + 0.1 * b, 0.4 + 0.05 * b, 0.1 * b] for b in range(8)]
    out = []
    for rate in (44100, 48000, 96000):
        for label, schedule in (("steady", dual_steady), ("moving", dual_moving)):
            out.append({"class": "DualSKF", "mode": 0, "variant": 0, "rate": rate, "label": label,
                        "schedule": schedule, "left": left, "right": right})
        for label, schedule in (("steady", mon_steady), ("moving", mon_moving)):
            out.append({"class": "BlueMonark", "mode": 0, "variant": 0, "rate": rate, "label": label,
                        "schedule": schedule, "left": left, "right": right})
    return out


def main():
    exe, out_dir = sys.argv[1:3]
    e = Emu(exe)
    complete(e, TABLE_INIT)
    mono_path = os.path.join(out_dir, "kontaktfx_vectors.json")
    with open(mono_path) as f:
        existing = json.load(f)["cases"]
    tables = make_tables(e, 8 * BLOCK)
    worst = {}
    out = []
    for case in mono_cases():
        spec = DUAL if case["class"] == "DualSKF" else MONARK
        res = run_mono(e, tables, case, spec)
        if case["class"] == "DualSKF":
            row = {"class": case["class"], "knobs": case["knobs"], "mode": case["mode"],
                   "variant": case["variant"], "impulse": res["impulse"], "square": res["square"]}
            out.append(row)
            continue
        # Validation only: the stored BlueMonark case is kept, kemu is compared against it.
        match = [c for c in existing if c["class"] == "BlueMonark" and c["knobs"] == case["knobs"]
                 and c["mode"] == case["mode"] and c["variant"] == case["variant"]][0]
        for name in ("impulse", "square"):
            err = rel_error(res[name], match[name])
            key = (case["mode"], case["variant"])
            worst[key] = max(worst.get(key, 0.0), err)
    print("BlueMonark kemu vs stored, worst error of peak per (mode, variant):", flush=True)
    for key, err in sorted(worst.items()):
        print("  mode %d variant %d: %.3g%s" % (key[0], key[1], err, "" if err <= 1e-6 else "  NOT REPRODUCED"))
    with open(mono_path, "w") as f:
        json.dump({"rate": 48000, "cases": [c for c in existing if c["class"] == "SimpleLPHP"] + out
                   + [c for c in existing if c["class"] == "BlueMonark"]}, f, separators=(",", ":"))
    stereo_out = []
    for case in stereo_cases():
        spec = DUAL if case["class"] == "DualSKF" else MONARK
        tables = make_tables(e, len(case["left"]))
        out_l, out_r = render(e, spec, rate=case["rate"], channels=2, mode=case["mode"], variant=case["variant"],
                              schedule=case["schedule"], left=case["left"], right=case["right"], tables=tables)
        stereo_out.append({"class": case["class"], "mode": case["mode"], "variant": case["variant"],
                           "rate": case["rate"], "label": case["label"], "knobs": case["schedule"],
                           "left": case["left"], "right": case["right"], "outL": out_l, "outR": out_r})
        print(case["class"], case["rate"], case["label"], "peak %.4g" % max(abs(v) for v in out_l + out_r), flush=True)
    with open(os.path.join(out_dir, "kontaktfx_stereo_vectors.json"), "w") as f:
        json.dump(stereo_out, f, separators=(",", ":"))


if __name__ == "__main__":
    main()
