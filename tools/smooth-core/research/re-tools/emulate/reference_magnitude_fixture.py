"""Differential fixture for the reference magnitude kernel 0x6181b0.

Drives the emulated kernel with random interleaved complex input and writes the
input and output as raw f32 bit patterns, so the Rust translation in
`tools/smooth-core/src/reference/magnitude.rs` can be compared bit-for-bit.

The kernel computes re*re + im*im per bin (x64 SSE, one product and one sum per
term, no FMA) and then tail-calls `(*table[sel])(dst, dst, bins)` through the
runtime selector at 0x1821ed508. The fixture records the selector and the target
RVA, so the Rust side can name the tail step it mirrors.

Evidence scope: isolated emulation with the reference-class CPU profile, on random
input. It checks the kernel's own arithmetic, not the plug-in's data.

Run with:  python -I reference_magnitude_fixture.py <out.json>
Writes ../../../src/reference/fixtures/magnitude.json when given that path.
"""

import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

from fake_host import install_tls, set_cpu_mode  # noqa: E402
from pe_emu import IMAGE_BASE, Emulator, image_va  # noqa: E402

MAGNITUDE = image_va(0x6181B0)
SELECTOR = image_va(0x21ED508)  # VA 0x1821ed508
TABLE = image_va(0x21ECAE8)  # VA 0x1821ecae8
# Slot 0 of the table resolves to a `jmp [rip+x]` at 0x1c8740 whose pointer lives at
# 0x21ece78 (IPP sqrt, 0x233f40). Pointing that slot at a `ret` stub leaves the
# kernel's power stage as the whole result, so the power arithmetic can be compared bit-exact.
TAIL_SLOT = image_va(0x21ECE78)

# Bin counts cover every loop shape in the kernel: the 8-wide unrolled loop, its
# 4-wide tail, the scalar tail, and the exact multiples in between.
BINS = list(range(1, 34)) + [40, 63, 64, 65, 127, 129, 255, 1000]


def f32_bits(arr):
    return [struct.unpack("<I", struct.pack("<f", float(v)))[0] for v in arr]


def run_case(emu, rng, bins, ret_stub, ref_tail):
    cplx = rng.standard_normal(2 * bins).astype(np.float32)
    # Exact zeros and one large magnitude keep the edge cases in every case set.
    if bins % 5 == 0:
        cplx[:2] = 0.0
    if bins % 7 == 0:
        cplx[-1] = np.float32(3.0e18)
    src = emu.arena(4 * 2 * bins + 64)
    dst = emu.arena(4 * bins + 64)
    emu.wr(src, cplx.tobytes())
    emu.call(MAGNITUDE, dst, src, bins, max_insns=10_000_000)
    out = np.frombuffer(emu.rd(dst, 4 * bins), dtype=np.float32)
    emu.w64(TAIL_SLOT, ret_stub)
    emu.call(MAGNITUDE, dst, src, bins, max_insns=10_000_000)
    power = np.frombuffer(emu.rd(dst, 4 * bins), dtype=np.float32)
    emu.w64(TAIL_SLOT, ref_tail)
    return {
        "bins": bins,
        "in": f32_bits(cplx),
        "power": f32_bits(power),
        "out": f32_bits(out),
    }


def main(argv):
    out = Path(argv[0]) if argv else None
    emu = Emulator()
    install_tls(emu)
    set_cpu_mode(emu)
    # The file holds -1 here; the runtime init (fcn.1802fb880) writes 0 or 0x14. The
    # harness pins 0, the slot the other emulated kernels use (fft_verify.py).
    emu.w32(SELECTOR, 0)
    sel = emu.r32(SELECTOR)
    target = emu.r64(TABLE + 8 * sel)
    rng = np.random.default_rng(0x6181B0)
    ref_tail = emu.r64(TAIL_SLOT)
    ret_stub = emu.stub("magnitude-power-only", lambda emu, ev: 0)
    cases = [run_case(emu, rng, n, ret_stub, ref_tail) for n in BINS]
    result = {
        "source": "soothe3 0x6181b0 emulated (isolated, reference-class CPU profile, random input)",
        "kernel_rva": "0x6181b0",
        "dispatch": {
            "selector_rva": "0x21ed508",
            "selector": sel,
            "target_rva": hex(target - IMAGE_BASE),
            "sqrt_tail_rva": hex(ref_tail - IMAGE_BASE),
        },
        "cases": cases,
    }
    text = json.dumps(result, separators=(",", ":"))
    if out:
        out.write_text(text + "\n")
    print(f"{len(cases)} cases, selector={sel}, bins {BINS[0]}..{BINS[-1]}")


if __name__ == "__main__":
    main(sys.argv[1:])
