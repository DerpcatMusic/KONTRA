"""Verify the emulated soothe3 FFT primitives against numpy.

The detector's analysis chain (fcn.180619fc0) calls thunks in the image's
jump-table families. Each thunk's slot 0 (the runtime selector is 0 in this
harness) is the kernel that runs. Verified here, by running the image code
under emulation and comparing with numpy at orders 1..12:

  0x1010 -> 0x1640  real forward, unscaled (spec flag 8). N reals in,
                    N+2 floats out: [X0.re, 0, X1.re, X1.im, ..., X_{N/2-1},
                    X_{N/2}.re, 0]  (packed CCS, bins 0..N/2)
  0x1280 -> 0x7000  real inverse, unscaled. Input is the packed layout above,
                    output is N reals = N * x (no 1/N)
  0x1220 -> 0x5e80  in-place scale by a float (xmm0)
  0x10a0 -> 0x2ae0  copy: (src, dst, len) with src in rcx, dst in rdx, len in r8
  0x10d0 -> 0x2e40  zero-fill: (dst in rcx, len in rdx)
  0x1190 -> 0x49e0  elementwise multiply dst[i] = a[i] * b[i]
  0x6181b0          complex MAGNITUDE |Y_k| = sqrt(re^2 + im^2), not the power
                    (not a thunk; input interleaved complex, arg3 = bins)

Spec construction is the init thunk 0x12b0 -> 0x7c40 with six Win64 arguments:
(out**, order, flag, hint, scratch, twiddle scratch). The twiddle scratch is
needed from order 11 on. Flag 8 is the unscaled spec the detector uses.

Evidence scope: isolated emulation of the image's own kernels on random
input, with the reference-class CPU profile and the libm stubs. It checks the
primitives, not the plug-in's data or defaults.

Run with: python -I fft_verify.py [out.json]
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

from fake_host import install_tls, set_cpu_mode  # noqa: E402
from pe_emu import Emulator, image_va  # noqa: E402

INIT = image_va(0x12B0)
FWD = image_va(0x1010)
INV = image_va(0x1280)
SCALE = image_va(0x1220)
COPY = image_va(0x10A0)
ZERO = image_va(0x10D0)
MUL = image_va(0x1190)
POWER = image_va(0x6181B0)

ORDERS = range(1, 13)


def packed_from_numpy(x):
    """numpy reference for the forward thunk's packed output."""
    n = len(x)
    spec = np.fft.rfft(x.astype(np.float64))
    out = np.zeros(n + 2)
    out[0] = spec[0].real
    for k in range(1, n // 2):
        out[2 * k] = spec[k].real
        out[2 * k + 1] = spec[k].imag
    out[n] = spec[n // 2].real
    return out


def rel_err(got, ref):
    return float(np.max(np.abs(got - ref)) / max(1e-30, np.max(np.abs(ref))))


def make_spec(emu, order):
    outp = emu.arena(16)
    buf = emu.arena(1 << 16)
    buf_tw = emu.arena(1 << 22)
    emu.call(INIT, outp, order, 8, 1, buf, buf_tw, max_insns=400_000_000)
    return emu.r64(outp)


def check_fft(emu, order):
    n = 1 << order
    spec = make_spec(emu, order)
    tmp = emu.arena(4 * n + 64)
    x = np.random.default_rng(order).standard_normal(n).astype(np.float32)
    src = emu.arena(4 * n + 64)
    dst = emu.arena(4 * (n + 2) + 64)
    emu.wr(src, x.tobytes())
    emu.call(FWD, src, dst, spec, tmp, max_insns=400_000_000)
    fwd = np.frombuffer(emu.rd(dst, 4 * (n + 2)), dtype=np.float32).astype(np.float64)
    packed = packed_from_numpy(x)

    p = packed.astype(np.float32)
    psrc = emu.arena(4 * (n + 2) + 64)
    pdst = emu.arena(4 * n + 64)
    emu.wr(psrc, p.tobytes())
    emu.call(INV, psrc, pdst, spec, tmp, max_insns=400_000_000)
    inv = np.frombuffer(emu.rd(pdst, 4 * n), dtype=np.float32).astype(np.float64)
    return {
        "order": order,
        "n": n,
        "forward_relerr": rel_err(fwd, packed),
        "inverse_relerr_vs_n_x": rel_err(inv, n * x.astype(np.float64)),
    }


def check_primitives(emu):
    rng = np.random.default_rng(7)
    n = 37
    a = rng.standard_normal(n).astype(np.float32)
    b = rng.standard_normal(n).astype(np.float32)
    pa = emu.arena(4 * n + 64)
    pb = emu.arena(4 * n + 64)
    pd = emu.arena(4 * n + 64)
    emu.wr(pa, a.tobytes())
    emu.wr(pb, b.tobytes())
    emu.call(MUL, pa, pb, pd, n, max_insns=10_000_000)
    mul = np.frombuffer(emu.rd(pd, 4 * n), dtype=np.float32).astype(np.float64)

    emu.wr(pa, a.tobytes())
    emu.call(SCALE, 0, pa, n, floats=(0.25,), max_insns=10_000_000)
    scaled = np.frombuffer(emu.rd(pa, 4 * n), dtype=np.float32).astype(np.float64)

    emu.call(COPY, pb, pd, n, max_insns=10_000_000)
    copied = np.frombuffer(emu.rd(pd, 4 * n), dtype=np.float32).astype(np.float64)
    emu.call(ZERO, pd, n, max_insns=10_000_000)
    zeroed = np.frombuffer(emu.rd(pd, 4 * n), dtype=np.float32).astype(np.float64)

    # magnitude over complex bins: |Y_k| = sqrt(re^2 + im^2) (interleaved input, bins = n)
    cplx = (rng.standard_normal(2 * n)).astype(np.float32)
    pc = emu.arena(4 * 2 * n + 64)
    pp = emu.arena(4 * n + 64)
    emu.wr(pc, cplx.tobytes())
    emu.call(POWER, pp, pc, n, max_insns=10_000_000)
    power = np.frombuffer(emu.rd(pp, 4 * n), dtype=np.float32).astype(np.float64)
    c = cplx.astype(np.float64).reshape(-1, 2)[:n]
    return {
        "mul_relerr": rel_err(mul, (a * b).astype(np.float64)),
        "scale_relerr": rel_err(scaled, 0.25 * a.astype(np.float64)),
        "copy_relerr": rel_err(copied, b.astype(np.float64)),
        "zero_max_abs": float(np.max(np.abs(zeroed))),
        "magnitude_relerr": rel_err(power, np.hypot(c[:, 0], c[:, 1])),
    }


def main(argv):
    out = argv[0] if argv else None
    emu = Emulator()
    install_tls(emu)
    set_cpu_mode(emu)
    rows = [check_fft(emu, order) for order in ORDERS]
    prims = check_primitives(emu)
    worst = max(max(r["forward_relerr"], r["inverse_relerr_vs_n_x"]) for r in rows)
    result = {
        "source": "soothe3 emulated kernels vs numpy (isolated emulation, reference-class CPU profile)",
        "orders": rows,
        "primitives": prims,
        "worst_fft_relerr": worst,
        "emulator_aborted": emu.aborted,
    }
    text = json.dumps(result, indent=2)
    if out:
        Path(out).write_text(text + "\n")
    print(text)


if __name__ == "__main__":
    main(sys.argv[1:])
