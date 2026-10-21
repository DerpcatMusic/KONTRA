"""Fixture for the zero-latency partitioned convolver 0x8dec70 (translate-analysis pass 3).

Runs the emulated plugin in zero-latency mode (byte obj+0x237 = 1) for one block so the
workers 0x8b1630 / 0x8b3be0 build their convolver objects, then takes the first short
(IR 256) and long (IR 2048) convolver, clears its rings, writes a random impulse response
(32 direct head taps at +0x40, random packed spectra in the partition list +0x108) and
calls 0x8dec70 once per sample with random input. Writes oracle/zl_fixture_48000.json:
per convolver the size, level FFT sizes (+0xf0), FDL slot count (+0x120), head taps,
partition spectra, input and output.

Evidence scope: isolated emulation, constructor state, synthetic host (process_lib).
"""

import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import unicorn.x86_const as UC  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

import process_oracle as po  # noqa: E402
from pe_emu import IMAGE_BASE  # noqa: E402
from process_fixture import f32  # noqa: E402
from process_lib import Session, alloc_frames  # noqa: E402

CONV = 0x8DEC70
SEED = 20261021
OUT = Path(__file__).resolve().parents[2] / "oracle" / "zl_fixture_48000.json"


def vec(emu, at):
    b, e = emu.r64(at), emu.r64(at + 8)
    return [emu.r64(b + 8 * i) for i in range((e - b) // 8)]


def run(emu, conv, rng, samples):
    q = lambda o: emu.r64(conv + o)  # noqa: E731
    size = q(0)
    levels = [emu.r32(lv + 0x18) for lv in vec(emu, conv + 0xF0)]
    fdl = vec(emu, conv + 0x120)
    # clear state: input/output rings, head ring, FDL slots, positions
    emu.wr(q(0xC0), bytes(4 * size))
    emu.wr(q(0xD8), bytes(4 * size))
    emu.wr(conv + 0x188, bytes(128))
    for p in fdl:
        emu.wr(emu.r64(p), bytes(8 * emu.r64(p + 0x10)))
    for o in (0x138, 0x140, 0x188 + 0x80):
        emu.w64(conv + o, 0)
    head = [f32(rng.uniform(-0.5, 0.5)) for _ in range(32)]
    emu.wr(conv + 0x40, struct.pack("<32f", *head))
    parts = []
    for p in vec(emu, conv + 0x108):
        bins = emu.r64(p + 0x10)
        spec = [f32(rng.uniform(-0.5, 0.5)) for _ in range(2 * bins)]
        spec[1] = spec[-1] = 0.0
        emu.wr(emu.r64(p), struct.pack(f"<{2 * bins}f", *spec))
        parts.append(spec)
    x = [f32(rng.uniform(-1, 1)) for _ in range(samples)]
    y = []
    for v in x:
        emu.call(IMAGE_BASE + CONV, conv, floats=(0.0, v), max_insns=50_000_000)
        raw = emu.uc.reg_read(UC.UC_X86_REG_XMM0).to_bytes(16, "little")
        y.append(struct.unpack("<f", raw[:4])[0])
    return {"size": size, "levels": levels, "fdl_slots": len(fdl), "head": head, "parts": parts, "input": x, "output": y}


def main():
    s = Session(48000.0)
    s.prepare()
    emu = s.emu
    emu.wr(s.obj + po.ZL_OFFSET, b"\x01")
    convs = []

    def enter(uc, addr, size, user):
        c = emu.reg(UC.UC_X86_REG_RCX)
        if c not in convs:
            convs.append(c)

    h = emu.uc.hook_add(UC_HOOK_CODE, enter, begin=IMAGE_BASE + CONV, end=IMAGE_BASE + CONV)
    tin, _ = alloc_frames(emu, 2, 512, [[0.0] * 512] * 2)
    tout, _ = alloc_frames(emu, 2, 512, [[0.0] * 512] * 2)
    emu.call(po.PROCESS_THUNK, s.wrapper, tout, tin, 0, 512, max_insns=2_000_000_000)
    emu.uc.hook_del(h)
    short = next(c for c in convs if emu.r64(c) == 256)
    long = next(c for c in convs if emu.r64(c) == 2048)
    rng = random.Random(SEED)
    data = {"short": run(emu, short, rng, 1024), "long": run(emu, long, rng, 6144)}
    OUT.write_text(json.dumps(data))
    for k, v in data.items():
        print(k, v["size"], v["levels"], v["fdl_slots"], len(v["parts"]), [len(p) // 2 for p in v["parts"]])


if __name__ == "__main__":
    main()
