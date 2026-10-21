"""Differential fixtures for the sidechain path of soothe3 (research/stereo-ms-sidechain-output.md).

Writes src/reference/fixtures/sidechain.json, read by src/reference/sidechain.rs tests. Sections:

  meter   -- host id 13 (`sidechain_input_level`), published by the process block at 0x893928 once
             per 512-sample sub-block (ceil(N/512) values). Emulated with a side pair, with no side
             (null pointer), and with a silent side. Values are the f32 bit patterns the block passes.
  listen  -- sidechain listen (+0xa7, param 15) with a side pair. The output is the side signal
             delayed by the block latency (2304 at 48 kHz on the stand-in) and multiplied by the
             out-trim ramp, stepped on the absolute sample index. Dry/wet gains and mix do not apply.
  focus   -- the stereo focus law (0x892b34..0x892bf7), run as an isolated snippet with the real
             IPP scale helper (0x180001220) on a two-channel group. Band 1 focus is written through
             the real setter (param 25 -> +0x120).
  solo    -- setter case 0x5b (param 91, band solo): plain value -> +0x100 int index, through the
             real setter switch (0x894370) and its libm rounding.
  layout  -- 0x56cad0: layout index -> channel count, for indices 0..22 (the 23 layouts).

Evidence scope: isolated emulation on a synthetic host with the constructor stand-in at 48 kHz.
What is NOT here: the group list is empty in the stand-in (the layout builder 0x88acb0's static
init does not run under the harness), so the process block's per-group loop, and with it the
detector analysis, never runs. Those values come from the snippet and the emulator state only.

Run with (from this directory):  python -I sidechain_fixture.py ../../../../src/reference/fixtures/sidechain.json
"""

import json
import math
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

import process_lib  # noqa: E402
from process_lib import PROCESS_THUNK, Session, alloc_frames  # noqa: E402
from reference_output_fixture import ParamSetter  # noqa: E402
from pe_emu import Emulator, image_va  # noqa: E402

process_lib.HOST_BOOL_DEFAULTS[13] = 1  # the block publishes the meter only if the host reports id 13

RATE = 48000
DELAY = 2304  # getLatency at 48 kHz on the stand-in (groups = 1, base = 0)
SUB = 512

P_SIDECHAIN, P_LISTEN, P_OUT_DB, P_MIX_PCT, P_BYPASS = 0x0E, 0x0F, 0x5F, 0x5E, 0x61
P_SOLO, P_FOCUS_B1 = 0x5B, 0x19  # 'Band 1 focus' is 0x13 + 6
RECORD_OUT = 0xBF58
STAGE_OUT_DB = (-60.0, 12.0, 72.0, -60.0)  # lo, hi, scale, offset of the out trim (as the output fixture)


def f32(x):
    return struct.unpack("<f", struct.pack("<f", float(x)))[0]


def bits(values):
    return [struct.unpack("<I", struct.pack("<f", float(v)))[0] for v in values]


def process_run(sess, x, side, listen, out_db, mix):
    """One emulated block. Returns (out channel bits, meter list, out record state before, after)."""
    emu, obj = sess.emu, sess.obj
    n = len(x[0])
    meter = []
    host = emu.r64(obj)

    def slot(e, ev):
        if ev.rdx == 13:
            meter.append(struct.unpack("<f", e.rd(ev.r8, 4))[0])
            return 0
        return sess._host_get(e, ev)

    emu.w64(host + 0x20, emu.stub("Host::meter", slot))
    st = ParamSetter(sess)
    st.set(P_LISTEN, float(listen), 0, 1, 1)
    st.set(P_OUT_DB, out_db, *STAGE_OUT_DB)
    st.set(P_MIX_PCT, mix, 0.0, 100.0, 100.0)
    rec_in = struct.unpack("<3f", emu.rd(obj + RECORD_OUT, 12))
    tin, _ = alloc_frames(emu, 2, n, [x[0].tolist(), x[1].tolist()])
    tout, bout = alloc_frames(emu, 2, n, [[0.0] * n, [0.0] * n])
    tside = alloc_frames(emu, 2, n, [side[0].tolist(), side[1].tolist()])[0] if side is not None else 0
    emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, tside, n, max_insns=4_000_000_000)
    out = [np.frombuffer(emu.rd(b, 4 * n), dtype=np.float32).copy() for b in bout]
    rec_out = struct.unpack("<3f", emu.rd(obj + RECORD_OUT, 12))
    return out, meter, rec_in, rec_out


def meter_section(rng):
    cases = []
    sess_rate = RATE
    for n in (512, 1000, 2600, 4096):
        for _ in range(2):
            side = [(rng.standard_normal(n) * 0.25).astype(np.float32) for _ in range(2)]
            main = [(rng.standard_normal(n) * 0.3).astype(np.float32) for _ in range(2)]
            sess = Session(sess_rate)
            out, meter, _, _ = process_run(sess, main, side, 0, 0.0, 100.0)
            cases.append({"n": n, "side": [bits(s) for s in side], "meter": bits(meter), "side_present": True})
    # Null side (no side pointer) and silent side (pointer to zeros): both must publish zeros.
    for n, present in ((2600, False), (2600, True), (512, True)):
        main = [(rng.standard_normal(n) * 0.3).astype(np.float32) for _ in range(2)]
        side = [np.zeros(n, dtype=np.float32) for _ in range(2)] if present else None
        sess = Session(sess_rate)
        _, meter, _, _ = process_run(sess, main, side, 0, 0.0, 100.0)
        cases.append({
            "n": n,
            "side": [bits(s) for s in side] if present else None,
            "meter": bits(meter),
            "side_present": present,
        })
    return cases


def listen_section(rng):
    cases = []
    for out_db in (0.0, -6.0, 3.0, -40.0):
        n = 2600
        x = [(rng.standard_normal(n) * 0.3).astype(np.float32) for _ in range(2)]
        side = [(rng.standard_normal(n) * 0.25).astype(np.float32) for _ in range(2)]
        mix = float(rng.uniform(0, 100))
        sess = Session(RATE)
        out, meter, rec_in, rec_out = process_run(sess, x, side, 1, out_db, mix)
        cases.append({
            "n": n,
            "delay": DELAY,
            "out_db": out_db,
            "mix": mix,
            "side": [bits(s) for s in side],
            "out_rec_in": bits(rec_in),
            "out_rec_out": bits(rec_out),
            "out": [bits(o) for o in out],
        })
    return cases


def listen_no_side_section(rng):
    n = 2600
    x = [(rng.standard_normal(n) * 0.3).astype(np.float32) for _ in range(2)]
    sess = Session(RATE)
    out, _, _, _ = process_run(sess, x, None, 1, 0.0, 100.0)
    return {"n": n, "out": [bits(o) for o in out]}


# Focus snippet (0x892b34..0x892bf7). The snippet reads the group at rdi, the frame length at
# [rbp-0x3c], the channel table at [rsp+0x40] and the band balance at [r9 + r15 + 0x120].
SELECTOR = image_va(0x21ED508)


def focus_run(balance, seed, n):
    from unicorn.x86_const import (UC_X86_REG_R9, UC_X86_REG_R15, UC_X86_REG_RBP, UC_X86_REG_RDI,
                                   UC_X86_REG_RSP)

    sess = Session(RATE)
    emu, obj = sess.emu, sess.obj
    emu.w32(SELECTOR, 0)
    ParamSetter(sess).set(P_FOCUS_B1, balance, 0.0, 1.0, 1.0)
    stored = struct.unpack("<f", emu.rd(obj + 0x120, 4))[0]
    rng = np.random.default_rng(seed)
    left = rng.standard_normal(n).astype(np.float32)
    right = rng.standard_normal(n).astype(np.float32)
    bl = emu.arena(4 * n + 64)
    br = emu.arena(4 * n + 64)
    emu.wr(bl, left.tobytes())
    emu.wr(br, right.tobytes())
    rec = emu.arena(0x18)
    emu.w64(rec, 8)  # kind of the stereo pair (0x88acb0 case 2)
    emu.w64(rec + 8, 0)  # first channel of the group
    emu.w32(rec + 0x10, 2)  # channels in the group
    stack = emu.arena(0x3000)
    rsp = stack + 0x1000
    rbp = rsp + 0x800
    emu.w64(rsp + 0x40, bl)
    emu.w64(rsp + 0x48, br)
    emu.w32(rbp - 0x3C, n)
    emu.uc.reg_write(UC_X86_REG_RSP, rsp)
    emu.uc.reg_write(UC_X86_REG_RBP, rbp)
    emu.uc.reg_write(UC_X86_REG_R9, 0)  # band 1
    emu.uc.reg_write(UC_X86_REG_R15, obj)
    emu.uc.reg_write(UC_X86_REG_RDI, rec)
    emu.uc.emu_start(image_va(0x892B34), image_va(0x892BF7), 0, 10000)
    lo = np.frombuffer(emu.rd(bl, 4 * n), dtype=np.float32).copy()
    ro = np.frombuffer(emu.rd(br, 4 * n), dtype=np.float32).copy()
    return {
        "balance_in": bits([balance])[0],
        "balance_stored": bits([stored])[0],
        "n": n,
        "left": bits(left),
        "right": bits(right),
        "left_out": bits(lo),
        "right_out": bits(ro),
    }


def focus_section(rng):
    balances = [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0, -0.2, 1.3]
    balances += [float(v) for v in rng.uniform(0.0, 1.0, 8)]
    return [focus_run(b, 1000 + i, 37) for i, b in enumerate(balances)]


def solo_section(rng):
    values = [0.0, 0.4, 0.5, 1.0, 1.4, 1.5, 2.0, 2.5, 3.0, 8.0, 8.4, 8.6, 9.0, 12.0, -1.0, -0.5]
    values += [float(v) for v in rng.uniform(-2.0, 10.0, 24)]
    sess = Session(RATE)
    st = ParamSetter(sess)
    cases = []
    for v in values:
        st.set(P_SOLO, v, 0.0, 8.0, 8.0, 0.0)
        cases.append({"value": bits([v])[0], "index": struct.unpack("<i", sess.emu.rd(sess.obj + 0x100, 4))[0]})
    return cases


def layout_section():
    le = Emulator()
    return [{"index": i, "channels": le.call(image_va(0x56CAD0), i) & 0xFFFFFFFF} for i in range(23)]


def main(out_path):
    rng = np.random.default_rng(0x51DE)
    doc = {
        "source": "soothe3 process block 0x890a00 (side, listen, meter), focus snippet 0x892b34..0x892bf7, "
                  "setter 0x894370 (solo case 0x5b), layout 0x56cad0; emulated",
        "evidence_scope": "isolated emulation, constructor stand-in at 48 kHz; group list empty (see module doc)",
        "sample_rate": RATE,
        "delay": DELAY,
        "meter": meter_section(rng),
        "listen": listen_section(rng),
        "listen_no_side": listen_no_side_section(rng),
        "focus": focus_section(rng),
        "solo": solo_section(rng),
        "layout": layout_section(),
    }
    Path(out_path).write_text(json.dumps(doc))
    print(f"wrote {out_path}: meter {len(doc['meter'])}, listen {len(doc['listen'])}, "
          f"focus {len(doc['focus'])}, solo {len(doc['solo'])}, layout {len(doc['layout'])}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    main(sys.argv[1])
