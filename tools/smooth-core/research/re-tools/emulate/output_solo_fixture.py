"""Differential fixture for the output stage with the band-solo path (process block 0x890a00).

Runs the emulated process block over short sequences of blocks (N = 2600) with the solo band set
through the real setter (depth, frequency, Q, shape, used, solo index), then records, per block:
  - the parameters and the five gain records (dry, wet, main, solo, out) before and after,
  - the solo filter state before and after (two channels, 0x78 bytes each),
  - the input block x, the wet block W and the output channels as the emulator produced them.
  - the dry input D: the content of the dry ramp's frame (x delayed by the latency, to about 7e-7).
    The solo filter input is the sum wet + dry before the main ramp (exact match in traces).

Sub-block structure and the solo gate (verified in the disassembly and in traces, see
research/output-stage-translation.md section "Solo path"):
  out = W; out *= g_wet; scratch = D; scratch *= g_dry; out += scratch;
  if !bypass && (solo_active || solo.g > 1e-3):
      S = solo_filter(sum) (per sub-block, sum = out after the dry add); S *= g_solo; out *= g_main; out += S
  out *= g_out.

Evidence scope: isolated emulation on the constructor stand-in at 48 kHz. The wet path in this
stand-in equals the delayed input (no detector host state), so the wet and dry roles cannot be
separated by this fixture. The records and the solo path are checked; the wet role is not.

Run from this directory:  python -I output_solo_fixture.py <out.json>
"""

import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RCX, UC_X86_REG_RDX  # noqa: E402

from pe_emu import image_va  # noqa: E402
from process_lib import PROCESS_THUNK, Session, alloc_frames  # noqa: E402
from reference_output_fixture import SELECTOR, ParamSetter, f32_bits  # noqa: E402
from solo_filter_fixture import read_state, state_bits  # noqa: E402

RAMP = image_va(0x899AB0)
RATE = 48000
N = 2600
DELAY = 2304  # getLatency at 48 kHz (constructor stand-in)
RECORDS = {"dry": 0xBF28, "wet": 0xBF34, "main": 0xBF40, "solo": 0xBF4C, "out": 0xBF58}
STATE_BASE = 0xB7A8
BAND_BASE = 0x10C

# Each case is a list of block settings. Blocks in a case share the object, so the solo state and
# the gain records carry over (the second block is where the solo gain fades out).
CASES = [
    dict(name="solo_identity", blocks=[
        dict(wet_db=0.0, mix=100.0, out_db=0.0, delta=0, bypass=0, solo=1),
        dict(wet_db=0.0, mix=100.0, out_db=0.0, delta=0, bypass=0, solo=0),
    ]),
    dict(name="solo_trims", blocks=[
        dict(wet_db=-6.0, mix=50.0, out_db=-3.0, delta=0, bypass=0, solo=1),
        dict(wet_db=-6.0, mix=50.0, out_db=-3.0, delta=0, bypass=0, solo=0),
    ]),
    dict(name="solo_delta", blocks=[
        dict(wet_db=0.0, mix=30.0, out_db=0.0, delta=1, bypass=0, solo=1),
    ]),
    dict(name="solo_bypass", blocks=[
        dict(wet_db=0.0, mix=100.0, out_db=0.0, delta=0, bypass=1, solo=1),
    ]),
    dict(name="solo_tilt", blocks=[
        dict(wet_db=0.0, mix=100.0, out_db=0.0, delta=0, bypass=0, solo=1, shape=5),
    ]),
]


def run_case(case, rng):
    sess = Session(RATE)
    emu, obj = sess.emu, sess.obj
    emu.w32(SELECTOR, 0)
    st = ParamSetter(sess)
    # Solo band 1 (index 0): used, frequency, Q, depth, shape. The depth stays at its default here
    # because the wet path does not change in this stand-in.
    st.set(19, 1.0, 0.0, 1.0, 1.0)
    st.set(21, 1000.0, 20.0, 20000.0, 20000.0)
    st.set(23, 1.0, 0.1, 10.0, 10.0)
    results = []
    for blk in case["blocks"]:
        st.set(0x5D, blk["wet_db"], -60.0, 12.0, 72.0, -60.0)
        st.set(0x5E, blk["mix"], 0.0, 100.0, 100.0)
        st.set(0x5F, blk["out_db"], -60.0, 12.0, 72.0, -60.0)
        st.set(0x60, float(blk["delta"]), 0.0, 1.0, 1.0)
        st.set(0x61, float(blk["bypass"]), 0.0, 1.0, 1.0)
        st.set(0x5B, float(blk["solo"]), 0.0, 8.0, 8.0)  # solo index: 0 off, 1 band 1
        # The solo filter reads its shape from band+0x1c, which the setter fills from index 27
        # (`selector`), not from index 26 (`shape`, band+0x18). The constructor leaves it at 2.
        st.set(27, float(blk.get("shape", 2)), 0.0, 7.0, 7.0)

        params = dict(
            wet_gain=f32_bits([struct.unpack("<f", emu.rd(obj + 0x94, 4))[0]])[0],
            mix=f32_bits([struct.unpack("<f", emu.rd(obj + 0x98, 4))[0]])[0],
            out_gain=f32_bits([struct.unpack("<f", emu.rd(obj + 0x9C, 4))[0]])[0],
            delta=int(emu.rd(obj + 0xA8, 1)[0]),
            bypass=int(emu.rd(obj + 0xA9, 1)[0]),
            flag_236=int(emu.rd(obj + 0x236, 1)[0]),
        )
        rec_in = {k: list(struct.unpack("<3f", emu.rd(obj + off, 12))) for k, off in RECORDS.items()}
        state_in = [read_state(emu, obj + STATE_BASE + 0x78 * k) for k in range(2)]
        x = [rng.standard_normal(N).astype(np.float32) for _ in range(2)]
        tin, _ = alloc_frames(emu, 2, N, [x[0].tolist(), x[1].tolist()])
        tout, bout = alloc_frames(emu, 2, N, [[0.0] * N, [0.0] * N])

        captured = {"wet": [], "dry": []}

        def make_hook(_label):
            def on_ramp(uc, addr, size, user):
                rec = emu.reg(UC_X86_REG_RCX)
                frame = emu.reg(UC_X86_REG_RDX)
                if rec - obj not in (RECORDS["wet"], RECORDS["dry"]):
                    return
                name = "wet" if rec - obj == RECORDS["wet"] else "dry"
                n = struct.unpack("<i", emu.rd(frame + 0x84, 4))[0]
                ch = struct.unpack("<i", emu.rd(frame + 0x80, 4))[0]
                ptrs = [struct.unpack("<Q", emu.rd(frame + 8 * c, 8))[0] for c in range(ch)]
                if name == "wet":
                    captured["wet"].append([np.frombuffer(emu.rd(p, 4 * n), dtype=np.float32).copy() for p in ptrs])
                else:
                    captured["dry"].append([np.frombuffer(emu.rd(p, 4 * n), dtype=np.float32).copy() for p in ptrs])

            return on_ramp

        hook = make_hook("ramp")
        h = emu.uc.hook_add(UC_HOOK_CODE, hook, begin=RAMP, end=RAMP)
        emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, N, max_insns=4_000_000_000)
        emu.uc.hook_del(h)

        out = [np.frombuffer(emu.rd(b, 4 * N), dtype=np.float32).copy() for b in bout]
        rec_out = {k: list(struct.unpack("<3f", emu.rd(obj + off, 12))) for k, off in RECORDS.items()}
        state_out = [read_state(emu, obj + STATE_BASE + 0x78 * k) for k in range(2)]

        wet_block = [np.concatenate([c[ch] for c in captured["wet"]]) for ch in range(2)]
        dry_block = [np.concatenate([c[ch] for c in captured["dry"]]) for ch in range(2)]
        # The dry input is the delay line: x delayed by the latency, to about 7e-7 in this stand-in.
        # The first DELAY samples of a block hold the previous block's tail, so only [DELAY:] is x.
        for ch in range(2):
            err = float(np.max(np.abs(dry_block[ch][DELAY:] - x[ch][: N - DELAY])))
            assert err < 1e-5, f"dry input differs from delayed x by {err}"

        params["solo_active"] = int(emu.rd(obj + 0xAA, 1)[0])  # the flag this block computed
        results.append(dict(
            settings=blk,
            params=params,
            band=dict(
                freq=f32_bits([struct.unpack("<f", emu.rd(obj + BAND_BASE + 4, 4))[0]])[0],
                half_bw=f32_bits([struct.unpack("<f", emu.rd(obj + BAND_BASE + 0xC, 4))[0]])[0],
                shape=struct.unpack("<I", emu.rd(obj + BAND_BASE + 0x1C, 4))[0],
                idx=0,
            ),
            rec_in={k: f32_bits(v) for k, v in rec_in.items()},
            rec_out={k: f32_bits(v) for k, v in rec_out.items()},
            state_in=[state_bits(s) for s in state_in],
            state_out=[state_bits(s) for s in state_out],
            x=[f32_bits(v) for v in x],
            wet=[f32_bits(v) for v in wet_block],
            dry=[f32_bits(v) for v in dry_block],
            out=[f32_bits(v) for v in out],
        ))
    return results


def main(out_path):
    rng = np.random.default_rng(0x5010)
    cases = []
    for case in CASES:
        blocks = run_case(case, rng)
        cases.append(dict(name=case["name"], blocks=blocks))
    doc = {
        "source": "soothe3 process block 0x890a00, output sum with the band-solo path, emulated",
        "evidence_scope": "isolated emulation, constructor stand-in at 48 kHz, one solo band (index 0), "
                          "wet path equals the delayed input in this stand-in",
        "n": N,
        "delay": DELAY,
        "sample_rate": RATE,
        "cases": cases,
    }
    Path(out_path).write_text(json.dumps(doc))
    total = sum(len(c["blocks"]) for c in cases)
    print(f"wrote {len(cases)} cases, {total} blocks to {out_path}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    main(sys.argv[1])
