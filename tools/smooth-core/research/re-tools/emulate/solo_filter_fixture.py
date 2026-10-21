"""Differential fixture for soothe3's band-solo filter (0x89a1a0, with 0x89bf10 and 0x88e150).

Runs the solo filter entry point 0x89a1a0 under Unicorn on the constructor's own solo state
(obj+0xb7a8, one 0x78-byte record per channel) and a synthetic band record. Each case records the
state before and after the call, the band fields, the input and output channels. The Rust side
(`src/reference/solo.rs`) replays the same cases from the same state and must match:
  - every output sample to 1e-6 relative (bit-exact expected),
  - every state field the call writes (targets, smoothed values, coefficients, TPT state).

Calling convention (0x89a1a0, Win64, from the disassembly):
  rcx = solo state (per channel records, stride 0x78), rdx = output frame (channel pointers at +8k,
  channels at +0x80, N at +0x84), r8 = input frame (same layout), r9 = band record (`obj+0x10c+0x20*idx`):
  +0x04 f32 frequency, +0x0c f32 half bandwidth (octaves), +0x1c u32 shape.

Record layout (f64 unless noted): +0x00 sample rate, +0x08 i32 mode, +0x10 target R, +0x18 target k,
+0x20 smoothed R, +0x28 smoothed k, +0x30 unused (30.0 in the constructor), +0x38 smoothing rate,
+0x40 g, +0x48 k, +0x50 a1, +0x58 a2, +0x60 a3, +0x68 ic1, +0x70 ic2.

Cases vary the shape (all eight), the band frequency (below the 1e-6 floor, in band, above Nyquist),
the half bandwidth (negative, zero, typical), the channel count (1 or 2, including the copy branch
with mismatched frame counts), the block length (1..700), and the smoothing rate (the constructor's
value, >= 1 to force a snap, and small to show the one-pole approach).

Evidence scope: isolated emulation with the constructor state and synthetic frames. Run from this
directory with:  python -I solo_filter_fixture.py <out.json>
"""

import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

from pe_emu import image_va  # noqa: E402
from process_lib import Session  # noqa: E402
from reference_output_fixture import SELECTOR, make_frame  # noqa: E402

SOLO_FN = image_va(0x89A1A0)
STATE_BASE = 0xB7A8  # obj + 0xb7a8: group-0 solo records (constructor writes them)
STRIDE = 0x78
BAND_BASE = 0x10C  # band record = obj + 0x10c + 0x20 * idx
BAND_STRIDE = 0x20
FIELDS = 15  # 0x78 bytes of f64


def f32_bits(v):
    return struct.unpack("<I", struct.pack("<f", float(v)))[0]


def f64_bits(v):
    return struct.unpack("<Q", struct.pack("<d", float(v)))[0]


def read_state(emu, addr):
    """Record as a list indexed by offset/8: [0] f64, [1] i32 mode (+8), [2..14] f64."""
    raw = emu.rd(addr, STRIDE)
    vals = [struct.unpack_from("<d", raw, 0)[0], struct.unpack_from("<i", raw, 8)[0]]
    vals += [struct.unpack_from("<d", raw, 8 * k)[0] for k in range(2, FIELDS)]
    return vals


def write_state(emu, addr, vals):
    raw = bytearray(STRIDE)
    struct.pack_into("<d", raw, 0, float(vals[0]))
    struct.pack_into("<i", raw, 8, int(vals[1]))
    for k in range(2, FIELDS):
        struct.pack_into("<d", raw, 8 * k, float(vals[k]))
    emu.wr(addr, bytes(raw))


def state_bits(vals):
    return [f64_bits(vals[0]), int(vals[1]) & 0xFFFFFFFF] + [f64_bits(v) for v in vals[2:]]


def run_case(sess, rng, case_cfg):
    emu, obj = sess.emu, sess.obj
    ch = case_cfg["channels"]
    n = case_cfg["n"]
    band_idx = case_cfg["band_idx"]

    # Band record: frequency at +4, half bandwidth at +0xc, shape at +0x1c (what the binary reads).
    band = obj + BAND_BASE + BAND_STRIDE * band_idx
    emu.wr(band, bytes(0x20))
    emu.wr(band + 4, struct.pack("<f", case_cfg["freq"]))
    emu.wr(band + 0xC, struct.pack("<f", case_cfg["half_bw"]))
    emu.wr(band + 0x1C, struct.pack("<I", case_cfg["shape"]))

    # Smoothing rate override (the constructor's value unless the case sets one).
    states = [STATE_BASE + STRIDE * k for k in range(2)]
    before = []
    for k in range(2):
        vals = read_state(emu, obj + states[k])
        if case_cfg.get("rate") is not None:
            vals[7] = case_cfg["rate"]  # +0x38
        write_state(emu, obj + states[k], vals)
        before.append(read_state(emu, obj + states[k]))

    in_ch = case_cfg["in_channels"]
    xin = [rng.standard_normal(n).astype(np.float32) * case_cfg["amp"] for _ in range(max(in_ch, 1))]
    in_bufs = []
    for j in range(in_ch):
        b = emu.arena(4 * max(n, 1) + 64)
        emu.wr(b, xin[j].tobytes())
        in_bufs.append(b)
    out_bufs = []
    for j in range(ch):
        b = emu.arena(4 * max(n, 1) + 64)
        emu.wr(b, bytes(4 * max(n, 1) + 64))
        out_bufs.append(b)
    in_frame = make_frame(emu, in_bufs, n, in_ch)
    out_frame = make_frame(emu, out_bufs, n, ch)

    emu.call(SOLO_FN, obj + STATE_BASE, out_frame, in_frame, band, max_insns=2_000_000_000)

    out = [np.frombuffer(emu.rd(b, 4 * n), dtype=np.float32).copy() for b in out_bufs]
    after = [read_state(emu, obj + states[k]) for k in range(2)]
    return {
        "shape": case_cfg["shape"],
        "freq": f32_bits(case_cfg["freq"]),
        "half_bw": f32_bits(case_cfg["half_bw"]),
        "band_idx": band_idx,
        "channels": ch,
        "in_channels": in_ch,
        "n": n,
        "in": [[f32_bits(v) for v in xin[j]] for j in range(in_ch)],
        "out": [[f32_bits(v) for v in o] for o in out],
        "state_before": [state_bits(s) for s in before],
        "state_after": [state_bits(s) for s in after],
    }


def make_cases(rng):
    cases = []
    freqs = [1000.0, 40.0, 3000.0, 1e-7, 20000.0, 30000.0, 1234.5]
    hbs = [1.0, 0.25, 0.0, -0.5, 2.5]
    for shape in range(8):
        for freq in freqs[:4]:
            for hb in hbs[:3]:
                cases.append(dict(shape=shape, freq=freq, half_bw=hb, channels=2, in_channels=2,
                                  n=int(rng.integers(1, 300)), band_idx=int(rng.integers(0, 8)),
                                  amp=1.0, rate=None))
    # Copy branch: shape 5 with mismatched channel counts.
    cases.append(dict(shape=5, freq=1000.0, half_bw=1.0, channels=2, in_channels=1, n=40, band_idx=0, amp=1.0, rate=None))
    cases.append(dict(shape=5, freq=1000.0, half_bw=1.0, channels=1, in_channels=2, n=40, band_idx=0, amp=1.0, rate=None))
    cases.append(dict(shape=5, freq=1000.0, half_bw=1.0, channels=2, in_channels=2, n=40, band_idx=0, amp=1.0, rate=None))
    cases.append(dict(shape=5, freq=1000.0, half_bw=1.0, channels=1, in_channels=2, n=0, band_idx=0, amp=1.0, rate=None))
    # Mismatch with no copy (channels differ, input not mono): nothing is written.
    cases.append(dict(shape=2, freq=1000.0, half_bw=1.0, channels=1, in_channels=2, n=16, band_idx=0, amp=1.0, rate=None))
    # Smoothing: snap at rate >= 1, smooth at small rates, and the constructor's rate on a moving band.
    for rate in (1.0, 1.5, 0.5, 0.01, 1e-4):
        for shape in (0, 2, 4, 1):
            cases.append(dict(shape=shape, freq=5000.0, half_bw=0.8, channels=2, in_channels=2,
                              n=int(rng.integers(50, 600)), band_idx=2, amp=0.5, rate=rate))
    # Random sequence: shapes, frequencies, widths, lengths; state carries across cases.
    for _ in range(30):
        cases.append(dict(
            shape=int(rng.integers(0, 8)),
            freq=float(10 ** rng.uniform(1.0, 4.6)),
            half_bw=float(rng.uniform(-0.5, 3.0)),
            channels=int(rng.integers(1, 3)),
            in_channels=2,
            n=int(rng.integers(1, 700)),
            band_idx=int(rng.integers(0, 8)),
            amp=float(rng.uniform(0.01, 2.0)),
            rate=None,
        ))
        cases[-1]["in_channels"] = cases[-1]["channels"]
    return cases


def main(out_path):
    rng = np.random.default_rng(0x501D)
    results = []
    for rate in (48000, 44100):
        sess = Session(rate)
        emu = sess.emu
        emu.w32(SELECTOR, 0)  # IPP wrappers (copy) dispatch through this index; pin it as the other fixtures do
        for cfg in make_cases(rng):
            results.append(dict(rate=rate, **run_case(sess, rng, cfg)))
    doc = {
        "source": "soothe3 0x89a1a0 (band-solo filter), with 0x89bf10 (coefficient store) and 0x88e150 (SVF coefficients), emulated",
        "evidence_scope": "isolated emulation; constructor solo state at obj+0xb7a8; synthetic band records and frames",
        "cases": results,
    }
    Path(out_path).write_text(json.dumps(doc))
    print(f"wrote {len(results)} solo-filter cases to {out_path}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    main(sys.argv[1])
