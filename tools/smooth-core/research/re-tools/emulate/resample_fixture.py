"""Fixtures for the multi-group path (groups = trunc(rate / 44100) >= 2).

At groups D >= 2 the process block runs 0x897890 instead of going straight to 0x893de0:
  d = decimating FIR of the input (inline loop in 0x897890, taps `dec`, newest tap at x[D*j]),
  p = the normal-path chain at the decimated rate (0x898a10 -> 0x8acba0 -> 0x8b1120 workers),
  r = p - d delayed by 2304 decimated samples (0x8992f0 -> 0x13a0, dst -= src),
  up = interpolating FIR of r (inline loop in 0x897890, taps `itp`, r[j] lands at D*j),
  y = x delayed by 2304*D + T (0x891bca) + up (0x891c02 -> 0x1370).
The FIR is designed in the constructor by 0x89aed0 (length T = 48*D); 0x8965a0 stores
dec = reverse(h) and itp = D * h.

Writes oracle/resample_fixture_<rate>.json: the emulated h/dec/itp tables, K blocks of
seeded noise in and out, the decimated ch0 stream d (0x8a3886 copies), the ch0 residual r
(0x8992f0 result), and an impulse run: silence in, r[j] of block `imp_block` set to 1.0
right after 0x13a0, and the ch0 output.

Evidence scope: isolated emulation, constructor state, identity band state, synthetic host.
Run with: python3 -I resample_fixture.py [--rates 88200,96000,176400,192000]
"""

import argparse
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RDX  # noqa: E402

from pe_emu import IMAGE_BASE  # noqa: E402
from process_fixture import f32  # noqa: E402
from process_lib import Session, alloc_frames, read_frames  # noqa: E402
import process_oracle as po  # noqa: E402
from stage_lib import Snap  # noqa: E402

SEED = 20261023
K = 24
RESAMPLER = 0x258  # obj offset of the resampler struct (0x8965a0 is called on obj + 600)
SUB_CALL, SUB_RET = 0x8992F0, 0x8992F5


def tables(sess):
    emu = sess.emu
    q = lambda i: struct.unpack("<Q", emu.rd(sess.obj + RESAMPLER + 8 * i, 8))[0]  # noqa: E731
    d, t = q(2), q(7)
    itp = np.frombuffer(emu.rd(q(5), 4 * t), np.float32)
    dec = np.frombuffer(emu.rd(q(8), 4 * t), np.float32)
    return d, t, dec, itp


def noise_run(rate):
    sess = Session(rate)
    sess.prepare()
    n = lambda a: a["r8"] & 0xFFFFFFFF  # noqa: E731
    snap = Snap(sess.emu, {"dcopy": (0x8A3886, {"rdx": n}), "sub": (SUB_CALL, {"rdx": n})})
    snap.active = True
    rnd = random.Random(SEED + int(rate))
    x = [[f32(rnd.uniform(-0.5, 0.5)) for _ in range(K * po.N)] for _ in range(2)]
    y = [[], []]
    d0, r0 = [], []
    for k in range(K):
        snap.records = []
        tin, _ = alloc_frames(sess.emu, 2, po.N, [x[c][k * po.N:(k + 1) * po.N] for c in range(2)])
        tout, bo = alloc_frames(sess.emu, 2, po.N, [[0.0] * po.N] * 2)
        sess.emu.call(po.PROCESS_THUNK, sess.wrapper, tout, tin, 0, po.N, max_insns=800_000_000)
        o = read_frames(sess.emu, bo, po.N)
        for c in range(2):
            y[c] += o[c]
        d0 += next(r for r in snap.records if r["site"] == "dcopy")["after"]["rdx"]
        r0 += next(r for r in snap.records if r["site"] == "sub")["after"]["rdx"]
    return sess, x, y, d0, r0


def impulse_run(rate, block, j, blocks):
    sess = Session(rate)
    sess.prepare()
    emu = sess.emu
    st = {"k": -1, "n": 0, "rdx": 0}

    def enter(uc, a, sz, u):
        st["rdx"] = emu.reg(UC_X86_REG_RDX)

    def leave(uc, a, sz, u):
        if st["k"] == block and st["n"] == 0:
            emu.wr(st["rdx"] + 4 * j, struct.pack("<f", 1.0))
        st["n"] += 1

    emu.uc.hook_add(UC_HOOK_CODE, enter, begin=IMAGE_BASE + SUB_CALL, end=IMAGE_BASE + SUB_CALL)
    emu.uc.hook_add(UC_HOOK_CODE, leave, begin=IMAGE_BASE + SUB_RET, end=IMAGE_BASE + SUB_RET)
    y = []
    for k in range(blocks):
        st["k"], st["n"] = k, 0
        tin, _ = alloc_frames(emu, 2, po.N, [[0.0] * po.N] * 2)
        tout, bo = alloc_frames(emu, 2, po.N, [[0.0] * po.N] * 2)
        emu.call(po.PROCESS_THUNK, sess.wrapper, tout, tin, 0, po.N, max_insns=800_000_000)
        y += read_frames(emu, bo, po.N)[0]
    return y


def fixture(rate):
    sess, x, y, d0, r0 = noise_run(rate)
    d, t, dec, itp = tables(sess)
    imp_block, imp_j = 4, 5
    return {
        "source": "process block 0x890a00 under Unicorn (0x897890 resampler path)",
        "evidence_scope": "isolated emulation; identity band state; synthetic host",
        "rate": rate,
        "groups": d,
        "taps": t,
        "latency_getter": sess.latency(),
        "block_size": po.N,
        "h": dec[::-1].tolist(),
        "dec": dec.tolist(),
        "itp": itp.tolist(),
        "input": x,
        "output": y,
        "decimated_ch0": d0,
        "residual_ch0": r0,
        "imp_block": imp_block,
        "imp_j": imp_j,
        "imp_output_ch0": impulse_run(rate, imp_block, imp_j, 8),
    }


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--rates", default="88200,96000,176400,192000")
    ap.add_argument("--out-dir", default=str(po.ORACLE_DIR))
    a = ap.parse_args(argv)
    for r in (float(v) for v in a.rates.split(",")):
        doc = fixture(r)
        path = Path(a.out_dir) / f"resample_fixture_{int(r)}.json"
        path.write_text(json.dumps(doc, separators=(",", ":")) + "\n")
        print(path, "groups", doc["groups"], "taps", doc["taps"], "latency", doc["latency_getter"])


if __name__ == "__main__":
    main(sys.argv[1:])
