"""Fixture for the whole zero-latency path at 48 kHz (translate-analysis pass 3).

Runs the emulated plugin in zero-latency mode (byte obj+0x237 = 1) for K blocks of
random stereo input. At every call of the minimum-phase designer 0x8df6b0 (short worker
site 0x8d85d7, 257 bins; long worker site 0x8d9047, 2049 bins) the log-gain input array
is overwritten with a smooth random curve, standing in for the gain body's output.
Records the injected curves in call order, the short-stage output (the input stream of
the long worker's convolvers, call site 0x8b5919) and the plugin output. Also calls
0x8df6b0 directly once per size with a random curve and the plugin's window table and
records the IR it writes and the window.
Writes oracle/zl_path_fixture_48000.json.

Evidence scope: isolated emulation, constructor state, synthetic host (process_lib).
"""

import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
import unicorn.x86_const as UC  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

import process_oracle as po  # noqa: E402
from pe_emu import IMAGE_BASE  # noqa: E402
from process_fixture import f32  # noqa: E402
from process_lib import Session, alloc_frames, read_frames  # noqa: E402

K = 6
SEED = 20261021
SITES = {0x8D85D7: "short", 0x8D9047: "long"}
DESIGN = 0x8DF6B0
LONG_IN = 0x8B5919  # long worker, first convolver list
OUT = Path(__file__).resolve().parents[2] / "oracle" / "zl_path_fixture_48000.json"


def curve(rng, n):
    k = np.arange(n)
    v = sum(rng.uniform(-0.4, 0.4) * np.cos(np.pi * k * j / n + rng.uniform(0, 6)) for j in range(1, 5)) - 0.3
    return [f32(x) for x in v]


def xmm(emu, i):
    raw = emu.uc.reg_read(getattr(UC, f"UC_X86_REG_XMM{i}")).to_bytes(16, "little")
    return struct.unpack("<f", raw[:4])[0]


def main():
    s = Session(48000.0)
    s.prepare()
    emu = s.emu
    emu.wr(s.obj + po.ZL_OFFSET, b"\x01")
    rng = random.Random(SEED)
    designs = {"short": [], "long": []}
    args = {}

    def inject(site):
        def cb(uc, addr, size, user):
            regs = [emu.reg(r) for r in (UC.UC_X86_REG_RCX, UC.UC_X86_REG_RDX, UC.UC_X86_REG_R8, UC.UC_X86_REG_R9)]
            rsp = emu.reg(UC.UC_X86_REG_RSP)
            args.setdefault(site, (regs[2], emu.r64(regs[3]), emu.r32(regs[3] + 0x84)))
            a2 = regs[1]
            n = emu.r32(a2 + 8)
            v = curve(rng, n)
            emu.wr(emu.r64(a2), struct.pack(f"<{n}f", *v))
            designs[SITES[site]].append(v)
        return cb

    for site in SITES:
        emu.uc.hook_add(UC_HOOK_CODE, inject(site), begin=IMAGE_BASE + site, end=IMAGE_BASE + site)
    long_in = {}

    def grab(uc, addr, size, user):
        long_in.setdefault(emu.reg(UC.UC_X86_REG_RCX), []).append(xmm(emu, 1))

    emu.uc.hook_add(UC_HOOK_CODE, grab, begin=IMAGE_BASE + LONG_IN, end=IMAGE_BASE + LONG_IN)
    x = [[f32(rng.uniform(-0.5, 0.5)) for _ in range(K * 512)] for _ in range(2)]
    y = [[], []]
    for k in range(K):
        tin, _ = alloc_frames(emu, 2, 512, [x[c][k * 512:(k + 1) * 512] for c in range(2)])
        tout, bo = alloc_frames(emu, 2, 512, [[0.0] * 512] * 2)
        emu.call(po.PROCESS_THUNK, s.wrapper, tout, tin, 0, 512, max_insns=4_000_000_000)
        o = read_frames(emu, bo, 512)
        for c in range(2):
            y[c] += o[c]
    # long-worker convolver objects are allocated per channel in channel order
    short_out = [long_in[c] for c in sorted(long_in)]
    direct = {}
    for site, name in SITES.items():
        # fresh argument structs: the captured ones were stack locals of 0x8d8090
        fft, window, ir_len = args[site]
        size = emu.r32(fft + 0x18)
        n = size // 2 + 1
        v = curve(rng, n)
        a1, a2, a4, s5 = (emu.alloc(0x100) for _ in range(4))
        out, inp, scratch = emu.alloc(4 * size), emu.alloc(4 * n), emu.alloc(4 * 8 * size)
        emu.w64(a1, out)
        emu.w32(a1 + 8, size)
        emu.w64(a2, inp)
        emu.w32(a2 + 8, n)
        emu.wr(inp, struct.pack(f"<{n}f", *v))
        emu.w64(a4, window)  # the plugin's half-Hann (0x8a6960, start 0.5), count 1
        emu.w32(a4 + 0x80, 1)
        emu.w32(a4 + 0x84, ir_len)
        emu.w64(s5, scratch)
        emu.w64(s5 + 0x18, 0)
        emu.call(IMAGE_BASE + DESIGN, a1, a2, fft, a4, s5, max_insns=200_000_000)
        ir = list(struct.unpack(f"<{size}f", emu.rd(out, 4 * size)))
        win = list(struct.unpack(f"<{ir_len}f", emu.rd(window, 4 * ir_len)))
        direct[name] = {"fft": size, "ir_len": ir_len, "log_gain": v, "ir": ir, "window": win}
    data = {"input": x, "output": y, "short_out": short_out, "designs": designs, "direct": direct}
    OUT.write_text(json.dumps(data))
    print({k: len(v) for k, v in designs.items()}, {k: (v["fft"], v["ir_len"]) for k, v in direct.items()},
          "max |y-x|", float(np.abs(np.array(y) - np.array(x)).max()))


if __name__ == "__main__":
    main()
