"""Stage fixtures for the normal-path hop stages (translate-analysis pass 3).

The normal path (0x8ad640, zero-latency byte clear) runs two STFT stages in series:
  short: 0x8b61a0 -> hop loop 0x8c5f30 -> 0x8d9230 per hop  (frame 256, hop 128, FFT 512)
  long:  0x8b7410 -> hop loop 0x8c68a0 -> 0x8db030 per hop  (frame 2048, hop 1024, FFT 4096)
at 44.1/48 kHz. Each hop: window the newest `frame` ring samples (0x1190 by the table built
in 0x8a6960), zero-pad to 2*frame (0x10d0), forward real FFT (0x1010), multiply by the
complex per-bin multipliers H (0x1520), inverse real FFT (0x1280), scale by 1/N (0x1220),
zero `frame` floats at P+N then OLA dst += (hop/frame) * y (0x8e7000, 0x8e7150 -> 0x11f0),
and read the output at P - hop (0x8dad80 -> 0x10a0).

This script runs K blocks of seeded noise through the process block and writes
oracle/stage_fixture_<rate>.json with:
  kernels: the multiply 0x1520 and OLA 0x11f0 on random data (isolated calls in the same
           emulator, so the CPU-mode dispatch is the one the process path uses),
  short / long: every array of one hop (channel 0) at the traced call sites,
  stage_io: per block, the short-stage hop outputs (0x8dad80 dst) per channel.
The Rust tests in src/reference/hop.rs read it.

Evidence scope: isolated emulation, constructor state, identity band state (H = 1),
synthetic host. Run with: python3 -I stage_fixture.py [--rates 48000]
"""

import argparse
import json
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

from pe_emu import image_va  # noqa: E402
from process_fixture import f32  # noqa: E402
from process_lib import Session, alloc_frames, read_frames  # noqa: E402
import process_oracle as po  # noqa: E402
from stage_lib import Snap  # noqa: E402

SEED = 20261022
K = 12


def _len(a):
    return a["r9"] & 0xFFFFFFFF


SITES = {
    # 0x8e67a0 window multiply: (a = ring frame, b = window, dst = padded frame, len = frame)
    "win": (0x8E6A23, {"rcx": _len, "rdx": _len, "r8": lambda a: 2 * _len(a)}),
    # 0x1010 forward, short stage (inside the gain body 0x8e04c0; the stage reuses its spectrum)
    "fwd_short": (0x8E11BF, {"rcx": 512, "rdx": 514}),
    # 0x1010 forward, long stage (0x8db6a0)
    "fwd_long": (0x8DC59F, {"rcx": 4096, "rdx": 4098}),
    # 0x1520 multiply X *= H: short (0x8da0f3 path) and long (0x8db6a0)
    "mul_short": (0x8DAA10, {"rcx": 514, "rdx": 514}),
    "mul_long": (0x8DC810, {"rcx": 4098, "rdx": 4098}),
    # 0x1280 inverse inside 0x8e6eb0 and the 1/N scale 0x1220 after it
    "inv": (0x8E6F05, {}),
    "scale": (0x8E6FCB, {"rdx": lambda a: a["r8"]}),
    # 0x8e7150 -> 0x11f0 OLA add, (src, c = xmm1, dst, len)
    "ola": (0x8E71C8, {"rcx": lambda a: a["r9"], "r8": lambda a: a["r9"]}),
    # 0x8e7000 -> 0x10d0 zero ahead
    "ola_zero": (0x8E7095, {}),
    # 0x8dad80 -> 0x10a0 output read (src = ring at P - hop, dst, len)
    "out": (0x8DAF74, {"rcx": lambda a: a["r8"]}),
}


def kernels(emu, rng):
    def put(a):
        p = emu.arena(4 * len(a) + 64)
        emu.wr(p, np.asarray(a, np.float32).tobytes())
        return p

    def get(p, n):
        return np.frombuffer(emu.rd(p, 4 * n), np.float32).tolist()

    n = 257
    h = rng.standard_normal(2 * n).astype(np.float32)
    x = rng.standard_normal(2 * n).astype(np.float32)
    ph, px = put(h), put(x)
    emu.call(image_va(0x1520), ph, px, n, 0, max_insns=10_000_000)
    mul = {"h": h.tolist(), "x": x.tolist(), "y": get(px, 2 * n)}
    m = 512
    src = rng.standard_normal(m).astype(np.float32)
    dst = rng.standard_normal(m).astype(np.float32)
    ps, pd = put(src), put(dst)
    emu.call(image_va(0x11F0), ps, pd, pd, m, floats=(0.0, 0.5), max_insns=10_000_000)
    ola = {"src": src.tolist(), "dst": dst.tolist(), "c": 0.5, "out": get(pd, m)}
    return {"mul_0x1520": mul, "ola_0x11f0": ola}


def hop_arrays(recs, frame):
    """Channel-0 arrays of the first hop of a stage with this frame length.

    Records are matched by buffer address and taken in call order after the
    window multiply, because the kernel design (0x8f0c30) reuses the inverse and
    scale buffers just before the stage's own calls.
    """
    n = 2 * frame

    def after(i, pred):
        return next((j, r) for j, r in enumerate(recs) if j > i and pred(r))

    iw, win = after(-1, lambda r: r["site"] == "win" and _len(r["args"]) == frame)
    _, fwd = after(iw, lambda r: r["site"] in ("fwd_short", "fwd_long") and r["args"]["rcx"] == win["args"]["r8"])
    im, mul = after(iw, lambda r: r["site"] in ("mul_short", "mul_long") and r["args"]["rdx"] == fwd["args"]["rdx"])
    ii, inv = after(im, lambda r: r["site"] == "inv" and r["args"]["rcx"] == mul["args"]["rdx"])
    _, scale = after(ii, lambda r: r["site"] == "scale" and r["args"]["rdx"] == inv["args"]["rdx"])
    io, ola = after(ii, lambda r: r["site"] == "ola" and r["args"]["rcx"] == inv["args"]["rdx"])
    _, zero = after(ii, lambda r: r["site"] == "ola_zero" and r["args"]["rdx"] == frame)
    _, out = after(io, lambda r: r["site"] == "out" and abs(r["args"]["rcx"] - ola["args"]["r8"]) < 4 * n)
    return {
        "frame": frame,
        "fft_len": n,
        "ring_frame": win["before"]["rcx"],
        "window": win["before"]["rdx"],
        "windowed": win["after"]["r8"],
        "spectrum": fwd["after"]["rdx"],
        "multipliers": mul["before"]["rcx"],
        "product": mul["after"]["rdx"],
        "inverse": scale["before"]["rdx"],
        "scaled": scale["after"]["rdx"],
        "scale": scale["args"]["xmm0"],
        "ola_gain": ola["args"]["xmm1"],
        "ola_before": ola["before"]["r8"],
        "ola_after": ola["after"]["r8"],
        # positions in floats relative to the OLA add position P
        "zero_offset": (zero["args"]["rcx"] - ola["args"]["r8"]) // 4,
        "zero_len": zero["args"]["rdx"],
        "out_offset": (out["args"]["rcx"] - ola["args"]["r8"]) // 4,
        "out": out["before"]["rcx"],
    }


def fixture(rate):
    rnd = random.Random(SEED + int(rate))
    inp = [[f32(rnd.uniform(-0.5, 0.5)) for _ in range(K * po.N)] for _ in range(2)]
    sess = Session(rate)
    sess.prepare()
    emu = sess.emu
    snap = Snap(emu, SITES)
    out = [[], []]
    short_out = [[], []]
    long_block = None
    short_block = None
    snap.active = True
    for k in range(K):
        snap.records = []
        seg = [inp[c][k * po.N:(k + 1) * po.N] for c in range(2)]
        tin, _ = alloc_frames(emu, 2, po.N, seg)
        tout, bo = alloc_frames(emu, 2, po.N, [[0.0] * po.N] * 2)
        emu.call(po.PROCESS_THUNK, sess.wrapper, tout, tin, 0, po.N, max_insns=400_000_000)
        o = read_frames(emu, bo, po.N)
        for c in range(2):
            out[c] += o[c]
        # short-stage hop outputs: the 0x8dad80 reads of hop length (two channels per hop)
        hops = [r for r in snap.records if r["site"] == "out" and r["args"]["r8"] < po.N]
        for i, r in enumerate(hops):
            short_out[i % 2] += r["before"]["rcx"]
        if k == K - 2:
            short_block = snap.records
        if k == K - 1:
            long_block = [r for r in snap.records]
    # long stage runs on odd blocks only (hop 1024 = two blocks of 512)
    has_long = any(r["site"] == "fwd_long" for r in long_block)
    lb = long_block if has_long else short_block
    rng = np.random.default_rng(SEED)
    return {
        "source": "process block 0x890a00 under Unicorn, call-site snapshots (stage_lib.Snap)",
        "evidence_scope": "isolated emulation; identity band state (H = 1); synthetic host",
        "rate": rate,
        "seed": SEED + int(rate),
        "block_size": po.N,
        "blocks": K,
        "latency_getter": sess.latency(),
        "kernels": kernels(emu, rng),
        "short": hop_arrays(short_block, 256),
        "long": hop_arrays(lb, 2048),
        "input": inp,
        "short_out": short_out,
        "output": out,
    }


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--rates", default="48000")
    ap.add_argument("--out-dir", default=str(po.ORACLE_DIR))
    a = ap.parse_args(argv)
    for r in (float(x) for x in a.rates.split(",")):
        doc = fixture(r)
        path = Path(a.out_dir) / f"stage_fixture_{int(r)}.json"
        path.write_text(json.dumps(doc, separators=(",", ":")) + "\n")
        print(path, "short out_offset", doc["short"]["out_offset"], "long out_offset", doc["long"]["out_offset"],
              "zero", doc["short"]["zero_offset"], doc["long"]["zero_offset"])


if __name__ == "__main__":
    main(sys.argv[1:])
