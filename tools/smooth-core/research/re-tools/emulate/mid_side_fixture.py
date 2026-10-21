"""Differential fixture for the stereo-pair helper 0x8a3b20 (mid/side encode and decode).

Runs the helper under Unicorn on random stereo pairs for both flag values and both group modes,
and writes the source and destination channels as f32 bit patterns. The Rust side
(`src/reference/output.rs`, `mid_side_pair`) must reproduce the destination bit-for-bit.

Calling convention (from the disassembly and `stereo_pair_probe.py`):
  rcx = destination channel-pointer table (N at +0x84), rdx = source table, r8 = group list
  (stride 0x18: +0 rec pointer, +8 channel index, mode at rec+0x10), r9 = scratch arena,
  5th argument = flag: 1 for encode (gain 0.5, site 0x1808928a3), 0 for decode (gain 1.0,
  site 0x18089a5bc).

Evidence scope: isolated emulation with random data, N = 32. Run from this directory:
  python -I mid_side_fixture.py <out.json>
"""

import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from pe_emu import Emulator, image_va  # noqa: E402
from process_lib import PROCESS_THUNK  # noqa: E402,F401  (keeps the import path the same as the other fixtures)
from fake_host import install_tls, set_cpu_mode  # noqa: E402
from reference_output_fixture import SELECTOR  # noqa: E402

HELPER = image_va(0x8A3B20)
IPP_INIT = image_va(0x1DAE2A0)
N = 32


def bits(vals):
    return [struct.unpack("<I", struct.pack("<f", v))[0] for v in vals]


def run(emu, rnd, flag, mode):
    src_vals = [[rnd.uniform(-1.0, 1.0) for _ in range(N)] for _ in range(2)]
    dst = [emu.arena(4 * N + 64) for _ in range(2)]
    src = [emu.arena(4 * N + 64) for _ in range(2)]
    for c in range(2):
        emu.wr(src[c], struct.pack(f"<{N}f", *src_vals[c]))
        emu.wr(dst[c], bytes(4 * N + 64))
    t_dst = emu.arena(0x100)
    t_src = emu.arena(0x100)
    for c in range(2):
        emu.w64(t_dst + 8 * c, dst[c])
        emu.w64(t_src + 8 * c, src[c])
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
    emu.call(HELPER, t_dst, t_src, grp, arena, flag, max_insns=2_000_000)
    out = [list(struct.unpack(f"<{N}f", emu.rd(d, 4 * N))) for d in dst]
    return dict(
        flag=flag,
        mode=mode,
        n=N,
        src=[bits(v) for v in src_vals],
        out=[bits(v) for v in out],
    )


def main(out_path):
    emu = Emulator()
    install_tls(emu)
    set_cpu_mode(emu)
    emu.w32(image_va(0x21ED508), 0)  # IPP dispatch index, pinned as the other fixtures do
    emu.call(IPP_INIT, max_insns=2_000_000)
    rnd = random.Random(0x8A3B)
    cases = []
    for _ in range(24):
        cases.append(run(emu, rnd, 1, 0))  # encode: M = g(L+R), S = g(L-R), g = 0.5
    for _ in range(24):
        cases.append(run(emu, rnd, 0, 0))  # decode: g = 1.0
    for _ in range(4):
        cases.append(run(emu, rnd, 0, 1))  # mode 1: copy one channel
    doc = {
        "source": "soothe3 0x8a3b20 (stereo pair / mid-side), emulated; encode site 0x1808928a3 (flag 1), "
                  "decode site 0x18089a5bc (flag 0)",
        "cases": cases,
    }
    Path(out_path).write_text(json.dumps(doc))
    print(f"wrote {len(cases)} mid/side cases to {out_path}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    main(sys.argv[1])
