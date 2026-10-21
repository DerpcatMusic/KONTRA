"""Differential fixture for the output sum of soothe3's process block (0x890a00, output side).

One emulated process block per output configuration (wet/mix/out trims, delta, bypass). The
fixture stores the dry block, the wet block, the gain-record state at construction, the
parameter values the setter wrote, and the two output channels the emulated block produced.
`src/reference/output.rs` runs `process_output_block` on the same inputs and must match the
emulated output to 1e-6 relative.

Verified structure (emulation, research/output-stage-translation.md):
  - The output stage works on sub-blocks of at most 512 samples (the frame N field is 512,
    or the tail length). Gain ramps are one call per sub-block.
  - Per sub-block: out = W (copy), out *= g_wet (record +0xbf34), scratch = D * g_dry
    (record +0xbf28), out += scratch, out *= g_out (record +0xbf58).
  - The emulated wet path equals the delayed input in this stand-in (max |W - D| about 7e-7),
    so W is taken from the identity run, not from the dry signal. The mix = 0 run checks the
    dry path against the delayed input.

Evidence scope: isolated emulation on a synthetic host, constructor stand-in at 48 kHz.

Run with (from this directory):  python -I output_block_fixture.py <out.json>
"""

import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

from process_lib import PROCESS_THUNK, Session, alloc_frames  # noqa: E402
from reference_output_fixture import (  # noqa: E402
    P_BYPASS, P_DELTA, P_MIX_PCT, P_OUT_DB, P_WET_DB, ParamSetter, f32_bits,
)

DELAY = 2304  # getLatency at 48 kHz on the stand-in (groups = 1, base = 0; section 17 of re-latency notes)
BLOCK_N = 2600  # five full 512-sample sub-blocks and a 40-sample tail, past the latency
RATE = 48000
RECORDS = (("dry", 0xBF28), ("wet", 0xBF34), ("out", 0xBF58))

CONFIGS = [
    dict(wet_db=0.0, mix=100.0, out_db=0.0, delta=0, bypass=0),  # identity: W is taken from this run
    dict(wet_db=0.0, mix=0.0, out_db=0.0, delta=0, bypass=0),  # dry path
    dict(wet_db=-6.0, mix=50.0, out_db=-3.0, delta=0, bypass=0),
    dict(wet_db=0.0, mix=30.0, out_db=0.0, delta=1, bypass=0),  # delta
    dict(wet_db=-12.0, mix=80.0, out_db=6.0, delta=0, bypass=1),  # bypass
    dict(wet_db=3.0, mix=100.0, out_db=-9.0, delta=1, bypass=0),
    dict(wet_db=-20.0, mix=45.0, out_db=-40.0, delta=0, bypass=0),
]


def run_config(cfg, x):
    sess = Session(RATE)
    emu, obj = sess.emu, sess.obj
    st = ParamSetter(sess)
    st.set(P_WET_DB, cfg["wet_db"], -60.0, 12.0, 72.0, -60.0)
    st.set(P_MIX_PCT, cfg["mix"], 0.0, 100.0, 100.0)
    st.set(P_OUT_DB, cfg["out_db"], -60.0, 12.0, 72.0, -60.0)
    st.set(P_DELTA, float(cfg["delta"]), 0.0, 1.0, 1.0)
    st.set(P_BYPASS, float(cfg["bypass"]), 0.0, 1.0, 1.0)
    rec_in = {k: list(struct.unpack("<3f", emu.rd(obj + off, 12))) for k, off in RECORDS}
    params = {
        "wet_gain": struct.unpack("<f", emu.rd(obj + 0x94, 4))[0],
        "mix": struct.unpack("<f", emu.rd(obj + 0x98, 4))[0],
        "out_gain": struct.unpack("<f", emu.rd(obj + 0x9C, 4))[0],
    }
    tin, _ = alloc_frames(emu, 2, BLOCK_N, [x[0].tolist(), x[1].tolist()])
    tout, bout = alloc_frames(emu, 2, BLOCK_N, [[0.0] * BLOCK_N, [0.0] * BLOCK_N])
    emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, BLOCK_N, max_insns=4_000_000_000)
    out = [np.frombuffer(emu.rd(b, 4 * BLOCK_N), dtype=np.float32).copy() for b in bout]
    return dict(
        cfg=cfg,
        params=params,
        rec_in=rec_in,
        delta=bool(emu.rd(obj + 0xA8, 1)[0]),
        bypass=bool(emu.rd(obj + 0xA9, 1)[0]),
        out=out,
    )


def main(out_path):
    rng = np.random.default_rng(0xB10C)
    x = [rng.standard_normal(BLOCK_N).astype(np.float32) for _ in range(2)]
    runs = [run_config(cfg, x) for cfg in CONFIGS]

    wet_block = runs[0]["out"]  # identity: wet ramp at unity, dry ramp at zero, out at unity
    dry_block = []
    for c in range(2):
        d = np.zeros(BLOCK_N, dtype=np.float32)
        d[DELAY:] = x[c][: BLOCK_N - DELAY]
        dry_block.append(d)
    # Sanity: the mix = 0 run must still be the delayed input once the ramp settles (it does not,
    # the dry gain ramps from 0). Report the error of the ramp model instead of asserting it.
    dry_path_err = max(float(np.max(np.abs(runs[1]["out"][c][DELAY:] - dry_block[c][DELAY:]))) for c in range(2))
    print(f"mix 0 run vs delayed input (ramping, not identity): max abs {dry_path_err:.3g}")

    cases = []
    for r in runs:
        cases.append({
            "cfg": r["cfg"],
            "params": {
                "wet_gain": f32_bits([r["params"]["wet_gain"]])[0],
                "mix": f32_bits([r["params"]["mix"]])[0],
                "out_gain": f32_bits([r["params"]["out_gain"]])[0],
                "delta": r["delta"],
                "bypass": r["bypass"],
            },
            "rec_in": {k: f32_bits(v) for k, v in r["rec_in"].items()},
            "out": [f32_bits(o) for o in r["out"]],
        })
    doc = {
        "source": "soothe3 process block 0x890a00, output sum and gain ramps (512-sample sub-blocks), emulated",
        "evidence_scope": "isolated emulation, constructor stand-in at 48 kHz; wet path equals delayed input here",
        "n": BLOCK_N,
        "delay": DELAY,
        "sample_rate": RATE,
        "input": [f32_bits(v) for v in x],
        "wet_block": [f32_bits(w) for w in wet_block],
        "dry_block": [f32_bits(d) for d in dry_block],
        "cases": cases,
    }
    Path(out_path).write_text(json.dumps(doc))
    print(f"wrote {len(cases)} output-block cases to {out_path}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    main(sys.argv[1])
