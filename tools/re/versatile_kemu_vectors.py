"""Native 3x2 Versatile (saved 19) vectors under the kemu emulator (original code, no host).

Runs the unchanged Versatile code: rate setter FUN_1408de980, cutoff setter FUN_14095db50 (stage 0
note, gains and limits; it calls the stage 0 resonance setter FUN_140966230 and the normaliser
FUN_14096f840), stage 1 and 2 resonance setters FUN_1409663f0 and FUN_1409665c0, and the scalar
process FUN_140aef6e0 (the SSE branch of FUN_140aec840 gives the same output for one channel).

    python3 -I tools/re/versatile_kemu_vectors.py KONTAKT8_EXE OUT_JSON

Harness inputs (not native): the morph weights, the wet amounts, the output gain factor (FILTER_GAIN
= 0.5, factor 1) and the shift are written into the object as the LP-type values the stage laws give
(type 0, wet 1, shift 0). Their setters were not located, so these sweeps vary only cutoff and the
three resonances. The object has no constructor in this harness (see cascade_kemu_vectors.py).

Cases ('name', 'cut', 'res' = [stage 0, stage 1, stage 2], 'input', 'out' = [64 frames]):
  cut   cutoff sweep at equal resonances.
  res   independent stage resonances at cutoff 0.5 and 0.8.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

TABLE_INIT = 0x1404D14D0
SET_RATE = 0x1408DE980  # FUN_1408de980(this, rate): rate at +0x14 (via FUN_140a9a300), 2pi/rate
CUTOFF = 0x14095DB50  # FUN_14095db50(this, cutoff, resonance of stage 0)
RES_STAGE1 = 0x1409663F0
RES_STAGE2 = 0x1409665C0
PROCESS = 0x140AEF6E0  # FUN_140aef6e0(this, _, first channel, channel end, offset, frames, replace)
OBJ_BYTES = 0x2000
FRAMES = 64
STUB = bytes([0x31, 0xC0, 0xC3])
HARNESS_LP = [(0x3E0, 1.0), (0x3E4, 0.0), (0x3E8, 0.0),  # stage 0 low, band, high weights
              (0x3EC, 1.0), (0x3F0, 0.0), (0x3F4, 0.0),  # stage 1
              (0x3F8, 1.0), (0x3FC, 0.0), (0x400, 0.0),  # stage 2
              (0x404, 1.0), (0x408, 1.0), (0x40C, 1.0),  # wet per stage
              (0x420, 1.0)]  # output gain factor (FILTER_GAIN 0.5)


def complete(e, fn, *args, **kwargs):
    e.call(fn, *args, limit=20_000_000, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


def stimulus(kind):
    if kind == "imp":
        return [1.0] + [0.0] * (FRAMES - 1)
    return [4.0 if (i // 20) % 2 == 0 else -4.0 for i in range(FRAMES)]


def render(e, cut, res, kind):
    stub = e.alloc(16)
    e.uc.mem_write(stub, STUB)
    vt = e.alloc(0x200)
    for off in (0x28, 0x168, 0x170):
        e.write(vt + off, "Q", stub)
    obj = e.alloc(OBJ_BYTES)
    e.write(obj, "Q", vt)
    e.write(obj + 0x1E4, "f", -123.0)
    complete(e, SET_RATE, obj, f={1: 48000.0})
    inb, outb = e.alloc(FRAMES * 4), e.alloc(FRAMES * 4)
    e.write(inb, "f", *stimulus(kind))
    e.write(obj + 0x20, "Q", inb)
    e.write(obj + 0xA0, "Q", outb)
    e.write(obj + 0x124, "i", 1)
    for off, value in HARNESS_LP:
        e.write(obj + off, "f", value)
    complete(e, CUTOFF, obj, f={1: cut, 2: res[0]})
    complete(e, RES_STAGE1, obj, 0, f={1: res[1]})
    complete(e, RES_STAGE2, obj, 0, f={1: res[2]})
    # The output gain smoothing starts at its target (the native reset).
    e.write(obj + 0x388, "f", e.read(obj + 0x420, "f") * e.read(obj + 0x41C, "f"))
    complete(e, PROCESS, obj, 0, 0, 1, 0, FRAMES, 1)
    return list(e.read(outb, "f", FRAMES))


def cases():
    out = []
    for cut in (0.1, 0.3, 0.7, 0.9):
        for kind in ("imp", "loud"):
            out.append(("cut", cut, [0.5, 0.5, 0.5], kind))
    for cut in (0.5, 0.8):
        for res in ([0.2, 0.2, 0.2], [0.9, 0.9, 0.9], [0.6, 0.4, 0.0], [0.0, 0.7, 0.9]):
            for kind in ("imp", "loud"):
                out.append(("res", cut, res, kind))
    return out


def check_lp(e):
    """The existing VERS-lp keys (cutoff .5, resonance 0 / .5 / .3, all stages) must come out of this."""
    ref = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..",
                                      "crates/sampler-core/src/dsp/filter/cascade_stage_vectors.json")))
    for res in (0.0, 0.5, 0.3):
        for kind in ("imp", "loud"):
            want = ref[f"VERS-lp 0.50 {res:.2f} {kind}"]
            got = render(e, 0.5, [res, res, res], kind)
            err = max(abs(a - b) for a, b in zip(got, want)) / max(1e-3, max(abs(v) for v in want))
            if not err < 2e-5:
                raise SystemExit(f"VERS-lp {res} {kind}: err {err}")
    print("VERS-lp keys reproduced (2e-5)", flush=True)


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe, heap_mb=512)
    complete(e, TABLE_INIT)
    check_lp(e)
    rows = []
    for name, cut, res, kind in cases():
        rows.append({"name": name, "cut": cut, "res": res, "input": kind, "out": render(e, cut, res, kind)})
    with open(dst, "w") as f:
        json.dump({"native": "Kontakt8 FUN_14095db50 / FUN_1409663f0 / FUN_1409665c0 / FUN_140aef6e0 under kemu",
                   "cases": [{**r, "out": [float("%.9g" % v) for v in r["out"]]} for r in rows]},
                  f, separators=(",", ":"))
    print("cases", len(rows), flush=True)


if __name__ == "__main__":
    main()
