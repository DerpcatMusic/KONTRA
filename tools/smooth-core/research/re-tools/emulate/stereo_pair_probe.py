# Emulate the stereo-pair helper 0x8a3b20 (soothe3) on synthetic channel tables.
#
# Establishes what the helper writes and with which gain. Run from the repo root:
#   python3 -I tools/smooth-core/research/re-tools/emulate/stereo_pair_probe.py
#
# Findings it checks (see research/stereo-ms-sidechain-output.md):
#   arg2 = source pair (L, R), arg1 = destination pair. For a stereo group (mode 0):
#     dst[0] = g * (src[0] + src[1]),  dst[1] = g * (src[0] - src[1]),
#   with g = 1.0 when the 5th argument is 0 and g = 0.5 when it is 1.
#   Mode 1 copies one channel: dst[ch] = src[ch].
# Notes for the next reader: the IPP wrappers take (src, dst, n) for copy (0x10a0),
# not (dst, src, n), and 0x1070 takes its scalar in xmm1.
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from pe_emu import Emulator  # noqa: E402

N = 32
HELPER = 0x1808A3B20   # VA of rva 0x8a3b20
IPP_INIT = 0x181DAE2A0  # sets the dispatch index the IPP wrappers read


def floats(seed):
    rnd = random.Random(seed)
    return [rnd.uniform(-1, 1) for _ in range(N)]


def run(emu, flag, mode, same=False):
    left1, right1 = floats(1), floats(2)
    left2, right2 = floats(3), floats(4)

    def buf(vals):
        p = emu.alloc(4 * N)
        emu.wr(p, struct.pack(f"<{N}f", *vals))
        return p

    def read(p):
        return list(struct.unpack(f"<{N}f", emu.rd(p, 4 * N)))

    dst = [buf(left1), buf(right1)]
    src = dst if same else [buf(left2), buf(right2)]
    t_dst = emu.arena(0x100)
    t_src = emu.arena(0x100)
    for k in range(2):
        emu.w64(t_dst + 8 * k, dst[k])
        emu.w64(t_src + 8 * k, src[k])
    emu.w32(t_dst + 0x84, N)
    emu.w32(t_src + 0x84, N)
    rec = emu.arena(0x18)
    emu.w64(rec + 8, 0)
    emu.w32(rec + 0x10, mode)
    grp = emu.arena(0x10)
    emu.w64(grp + 0, rec)
    emu.w64(grp + 8, rec + 0x18)
    arena = emu.arena(0x20)
    base = emu.alloc(0x10000)
    emu.w64(arena, base)
    emu.w32(arena + 0x18, 0)
    emu.call(HELPER, t_dst, t_src, grp, arena, flag)
    return dict(l1=left1, r1=right1, l2=left2, r2=right2,
                out_l=read(dst[0]), out_r=read(dst[1]))


def err(a, b):
    return max(abs(x - y) for x, y in zip(a, b))


def main():
    emu = Emulator()
    emu.call(IPP_INIT)
    for flag in (0, 1):
        g = 0.5 if flag else 1.0
        d = run(emu, flag, 0)
        s = [g * (a + b) for a, b in zip(d["l2"], d["r2"])]
        t = [g * (a - b) for a, b in zip(d["l2"], d["r2"])]
        print(f"flag={flag} g={g}: out_l=g(L+R) err {err(d['out_l'], s):.2e}, "
              f"out_r=g(L-R) err {err(d['out_r'], t):.2e}")
        assert err(d["out_l"], s) < 1e-5 and err(d["out_r"], t) < 1e-5
    d = run(emu, 0, 1)
    print(f"mode=1 copy: out_l=in_l err {err(d['out_l'], d['l2']):.2e}")
    assert err(d["out_l"], d["l2"]) < 1e-5


if __name__ == "__main__":
    main()
