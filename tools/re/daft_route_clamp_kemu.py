"""Native routed-knob clamp of Daft (FilterDaft, stored 70/71) under kemu.

Runs the BFilterDaft slot-23 driver FUN_1408fa270 itself (its route body is unchanged) on a wrapper
object whose FilterDaft core sits at wrapper+0x210. The driver's prelude and postlude
(FUN_140ac28f0, FUN_140ac21f0: wrapper host-buffer and fade logic) are skipped by code hooks that
return at once; the route path does not read their results. Each route is a per-chunk float source
pointer (wrapper+0x49098 + 8*i) with an enable byte (wrapper+0x49090 + i); index 0 gain, 1 cutoff,
2 resonance, 3 response. The driver writes each enabled route with setParam (vtable +0x60, which
is FUN_140b05900) before the tick and the loop, once per 32-frame chunk.

    python3 -I tools/re/daft_route_clamp_kemu.py KONTAKT8_EXE OUT_JSON

Per case (same keys as daft_kemu_vectors.py, read by crates/sampler-core/src/dsp/filter/daft.rs):
'schedule' holds the RAW source values per chunk ([gain, cutoff, resonance]; a disabled knob is
constant), 'enabled' the route flags, 'stim', and 'outL'/'outR' the audio the driver produced.
'received' lists what setParam got for each enabled route, [index, value] per call. The script
checks it against the native clamp max(0, min(v, 1)) with NaN taken to 0 (DAT_14470bad4 = 1.0).
"""
import json
import math
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu, f32  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RDX, UC_X86_REG_RIP, UC_X86_REG_RSP, UC_X86_REG_XMM2  # noqa: E402

TABLE_INIT, CTOR, PREPARE, SETTER, TICK, PROCESS = 0x1404D14D0, 0x140AF4550, 0x140AFD210, 0x140B05900, 0x140B03620, 0x140AFFA30
DRIVER, PRELUDE, POSTLUDE = 0x1408FA270, 0x140AC28F0, 0x140AC21F0
CLAMP_ONE = 0x14470BAD4  # DAT_14470bad4, the driver's upper clamp
QUANTUM, BLOCK, BLOCKS = 32, 32, 8
N = BLOCKS * BLOCK
CORE = 0x210  # FilterDaft object inside the BFilterDaft wrapper
CORE_BYTES = 0x4A000  # as daft_kemu_vectors.py
WRAP_BYTES = CORE + CORE_BYTES + 0x1000
FRAMES, CHANNELS, COUNTDOWN, IN_PTRS = 0x120, 0x124, 0x1BC, 0x20
ROUTE_FLAG, ROUTE_SRC = 0x49090, 0x49098  # flag at +i, source pointer at +8i (i = setter index)
OUT_CHANNEL = 0x27004  # output channel k starts at (k * 0x400 + OUT_CHANNEL) floats into the out buffer
OUT_BYTES = 4 * (OUT_CHANNEL + 0x800 + N) + 0x1000


def native_clamp(v):
    """FUN_1408fa270 clamp: `v` raised to 0 when not >= 0 (so NaN gives 0), then capped at 1."""
    low = v if 0.0 <= v else 0.0
    return low if low <= 1.0 else 1.0


def complete(e, *args, limit=4_000_000, **kwargs):
    e.call(*args, limit=limit, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


class Hooks:
    """Records setParam calls made by the driver, and returns from the prelude and postlude."""

    def __init__(self, e):
        self.received = None
        e.uc.hook_add(UC_HOOK_CODE, self.setter, begin=SETTER, end=SETTER)
        for fn in (PRELUDE, POSTLUDE):
            e.uc.hook_add(UC_HOOK_CODE, self.skip, begin=fn, end=fn)

    def setter(self, uc, addr, size, _):
        if self.received is not None:
            index = uc.reg_read(UC_X86_REG_RDX)
            raw = uc.reg_read(UC_X86_REG_XMM2).to_bytes(16, 'little')
            self.received.append([index, struct.unpack('<f', raw[:4])[0]])

    def skip(self, uc, addr, size, _):
        sp = uc.reg_read(UC_X86_REG_RSP)
        ret = struct.unpack('<Q', bytes(uc.mem_read(sp, 8)))[0]
        uc.reg_write(UC_X86_REG_RSP, sp + 8)
        uc.reg_write(UC_X86_REG_RIP, ret)


def sweep(lo, hi, period):
    return [lo + (hi - lo) * (0.5 + 0.5 * math.sin(2 * math.pi * b / period)) for b in range(BLOCKS)]


def cases():
    out = []

    def add(name, enabled, gain=0.5, cutoff=0.5, resonance=0.5, response=0.2):
        columns = [gain, cutoff, resonance]
        columns = [c if isinstance(c, list) else [c] * BLOCKS for c in columns]
        out.append({
            "name": name,
            "rate": 48000,
            "response": response,
            "enabled": list(enabled) + [False],
            "schedule": [[g, c, r] for g, c, r in zip(*columns)],
        })

    add("gain_route_above_one", (True, False, False), gain=[0.3, 1.7, 1.2, 0.8, 2.5, 0.4, 1.0, 1.3])
    add("gain_route_below_zero", (True, False, False), gain=[0.6, -0.5, -0.2, 0.9, -3.0, 0.1, 0.5, -0.3])
    add("gain_route_in_range", (True, False, False), gain=sweep(0.1, 0.9, 4))
    add("cutoff_route_out_of_range", (False, True, False), cutoff=[0.3, 1.4, -0.3, 0.9, 1.0, 0.2, 1.7, 0.5])
    add("resonance_route_out_of_range", (False, False, True), resonance=[0.2, 1.5, -0.4, 0.9, 0.1, 1.2, 0.6, -0.1])
    add("gain_cutoff_resonance_routes", (True, True, True),
        gain=[0.4, 1.6, -0.5, 0.9, 1.2, 0.0, -0.2, 1.0],
        cutoff=[0.3, 1.4, 0.9, -0.2, 0.5, 1.0, 0.6, 0.2],
        resonance=[0.2, 0.8, 1.5, 0.1, -0.3, 0.9, 0.5, 1.1])
    return out


def run_case(e, hooks, case, buffers):
    left, right, out = buffers
    schedule = case["schedule"]
    enabled = case["enabled"]
    wrap = e.alloc(WRAP_BYTES)
    core = wrap + CORE
    complete(e, CTOR, core)
    vtable = e.read(core, 'Q')
    assert e.read(vtable + 0x60, 'Q') == SETTER, "vtable +0x60 is not the setter dispatcher"
    first = schedule[0]
    for index, value in enumerate(first + [case["response"]]):
        complete(e, SETTER, core, index, f={2: value})
    complete(e, PREPARE, core, 0, QUANTUM, f={1: float(case["rate"])})
    e.write(wrap + FRAMES, 'i', N)
    e.write(wrap + CHANNELS, 'I', 2)
    e.write(wrap + COUNTDOWN, 'i', 0)
    e.write(wrap + IN_PTRS, 'Q', left, right)
    for i in range(3):
        if enabled[i]:
            source = e.alloc(4 * BLOCKS)
            e.write(source, 'f', *[f32(row[i]) for row in schedule])
            e.write(wrap + ROUTE_FLAG + i, 'B', 1)
            e.write(wrap + ROUTE_SRC + 8 * i, 'Q', source)
    hooks.received = []
    complete(e, DRIVER, wrap, out, 0, limit=40_000_000)
    received, hooks.received = hooks.received, None
    expected = [[i, native_clamp(f32(schedule[c][i]))] for c in range(BLOCKS) for i in range(3) if enabled[i]]
    if received != expected:
        raise RuntimeError(f"{case['name']}: setParam saw {received}, native clamp {expected}")
    case["received"] = received
    case["outL"] = list(e.read(out + 4 * OUT_CHANNEL, 'f', N))
    case["outR"] = list(e.read(out + 4 * (0x400 + OUT_CHANNEL), 'f', N))
    return case


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe)
    complete(e, TABLE_INIT)  # exponential table, filled at load in the original
    assert e.read(CLAMP_ONE, 'f') == 1.0, "DAT_14470bad4 is not the unit clamp"
    hooks = Hooks(e)
    left, right = e.alloc(N * 4), e.alloc(N * 4)
    out = e.alloc(OUT_BYTES)
    results = []
    for case in cases():
        rng = random.Random(70)
        stim = [[rng.uniform(-0.3, 0.3) for _ in range(N)], [rng.uniform(-0.3, 0.3) for _ in range(N)]]
        e.write(left, "f", *stim[0])
        e.write(right, "f", *stim[1])
        case["stim"] = stim
        case = run_case(e, hooks, case, (left, right, out))
        results.append(case)
        worst = max(abs(v) for v in case["outL"] + case["outR"])
        print(case["name"], "received", len(case["received"]), "peak", worst, flush=True)
    with open(dst, "w") as f:
        json.dump(results, f, separators=(",", ":"))


if __name__ == "__main__":
    main()
