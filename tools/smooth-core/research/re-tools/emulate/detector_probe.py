"""Diagnostics for the reduction detector 0x619fc0 on the stand-in state (section 16).

Checks, each printed as a small table:
  1. kernel inputs: the first values reaching the inverse (0x180007000), forward (0x180001640)
     and magnitude (0x1806181b0) kernels, for silence and a sine. If the sample data never
     reaches a kernel, the output cannot depend on the input.
  2. output versus input amplitude at groups 1 and 2 (the group count is S+0x4a0).
  3. output versus a flat weight level W for silence and a sine. The output is a function of
     the host weight tables in this state.

Evidence scope: isolated emulation with a synthetic host and stand-in state. Nothing here is a
plug-in default. Run with: python -I detector_probe.py
"""

import math
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RCX, UC_X86_REG_RDX  # noqa: E402

import detector_run as dr  # noqa: E402
from fake_host import run_ctor  # noqa: E402

RATE = 48000.0
BINS = 1025
N = 2048
KERNELS = (
    (0x180007000, "inv", UC_X86_REG_RCX),  # real inverse, src in rcx
    (0x180001640, "fwd", UC_X86_REG_RCX),  # real forward, src in rcx
    (0x1806181B0, "mag", UC_X86_REG_RDX),  # complex magnitude, src in rdx
)


def run(samples, groups=1, weight=1.0):
    """Run the detector once; return (host output list, first kernel inputs)."""
    emu, obj, _ = run_ctor(RATE)
    S = dr.build_state(emu, obj, RATE, BINS)
    emu.w32(emu.r64(S + 0x4A0), groups)
    step = RATE / (2 * (BINS - 1))
    freqs = [k * step for k in range(BINS)]
    weights = [weight] * (BINS * groups)
    dr.dispatch(emu, S, 0, freqs)
    dr.dispatch(emu, S, 1, freqs)
    dr.dispatch(emu, S, 2, weights)
    dr.dispatch(emu, S, 3, weights)
    frame = dr.build_frame(emu, samples)
    host_ptr = emu.arena(8)
    host_block = emu.arena(0x2000)
    emu.w64(host_ptr, host_block)
    rect_lo = struct.unpack("<Q", struct.pack("<ff", 0.0, 0.0))[0]
    rect_hi = struct.unpack("<Q", struct.pack("<ff", dr.PLOT_W, dr.PLOT_H))[0]

    seen = []

    def make_hook(label, reg):
        def hook(uc, addr, size, user):
            base = emu.reg(reg)
            seen.append((label, [round(emu.rf32(base + 4 * i), 4) for i in range(4)]))

        return hook

    for pc, label, reg in KERNELS:
        emu.uc.hook_add(UC_HOOK_CODE, make_hook(label, reg), begin=pc, end=pc)
    emu.call(dr.DETECTOR, frame, obj, host_ptr, rect_lo, rect_hi, max_insns=400_000_000)
    out = [emu.rf32(host_block + 0x68 + 4 * i) for i in range(2022)]
    return out, seen[:3]


def sine(amp):
    return [amp * math.sin(2 * math.pi * 1000.0 * i / RATE) for i in range(N)]


def summary(out):
    return f"min={min(out):.4f} max={max(out):.4f}"


def main():
    print("1. kernel inputs (first calls), groups=1")
    for amp in (0.0, 0.9):
        _, seen = run(sine(amp))
        print(f"   amp={amp}: {seen}")
    print("2. output versus amplitude")
    for groups in (1, 2):
        for amp in (0.0, 0.9):
            out, _ = run(sine(amp), groups=groups)
            print(f"   groups={groups} amp={amp}: {summary(out)}")
    print("3. output versus flat weight W (groups=1)")
    for w in (1e-4, 1.0, 1e2):
        a, _ = run([0.0] * N, weight=w)
        b, _ = run(sine(0.9), weight=w)
        print(f"   W={w:g}: silence {summary(a)} | sine {summary(b)} | differ={a != b}")


if __name__ == "__main__":
    main()
