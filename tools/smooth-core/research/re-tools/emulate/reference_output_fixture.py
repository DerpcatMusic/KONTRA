"""Differential fixtures for the soothe3 output stage (translation of 0x890a00's output side).

Two fixtures, both written as raw f32 bit patterns so the Rust side compares bit-for-bit:

  ramp    -- the gain helper 0x899ab0 run in isolation on random records and frames.
             Its arithmetic is `g = (t - g) * a + g` per sample, then each channel sample is
             multiplied by g; the near-zero branch zeroes the channels through 0x1800010d0.
  targets -- the five gain-record targets that the process block writes at 0x891e0e..0x891ed8
             (`+0xbf28` dry, `+0xbf34` wet, `+0xbf40` main, `+0xbf4c` solo stream, `+0xbf58` out).
             Driven through the real setter 0x894370 (param switch cases 0x5d..0x61, 0xe, 0xf,
             0x10) on a constructed DSP object, then one emulated process block. The targets are
             written before the ramps run, so their values after one block are the block's targets.

Evidence scope: isolated emulation on a synthetic host with the constructor's stand-in state.
Checks the translated arithmetic, not the plug-in's data or its defaults.

Run with:  python -I reference_output_fixture.py ramp <out.json>
           python -I reference_output_fixture.py targets <out.json>
"""

import json
import math
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
from unicorn.x86_const import UC_X86_REG_XMM0, UC_X86_REG_XMM1  # noqa: E402

from fake_host import install_tls, set_cpu_mode  # noqa: E402
from pe_emu import Emulator, image_va  # noqa: E402
from process_lib import PROCESS_THUNK, SETPARAM_THUNK, Session, alloc_frames  # noqa: E402
from unicorn.x86_const import UC_X86_REG_XMM2  # noqa: E402

RAMP = image_va(0x899AB0)
SETPARAM = image_va(0x894370)  # setter body; the thunk 0x88f4e0 derefs the wrapper first
SELECTOR = image_va(0x21ED508)

RECORD_BYTES = 12  # t, g, a
GATE_T = 1.0e-3  # [0x18209d03c]
TIME_CONSTANT = -1.0 / 0.03  # [0x18209da5c], the 30 ms ramp


def f32_bits(values):
    return [struct.unpack("<I", struct.pack("<f", float(v)))[0] for v in values]


def f32(x):
    return struct.unpack("<f", struct.pack("<f", float(x)))[0]


def coeff_for_rate(fs):
    """a = 1 - exp(-1/(0.03 fs)), as the constructor computes it (0x88fa90, fVar23 = 1 - expf(c / fs))."""
    return f32(1.0 - math.exp(TIME_CONSTANT / float(fs)))


def make_frame(emu, bufs, n, ch):
    """Frame struct as the ramp reads it: channel pointers at +8j, channels at +0x80, N at +0x84."""
    frame = emu.arena(0x100)
    for j in range(ch):
        emu.w64(frame + 8 * j, bufs[j])
    emu.w32(frame + 0x80, ch)
    emu.w32(frame + 0x84, n)
    return frame


def ramp_case(emu, rng, fs, n, ch, rec, samples):
    t, g, a = rec
    rec_addr = emu.arena(RECORD_BYTES)
    emu.wr(rec_addr, struct.pack("<3f", t, g, a))
    bufs = []
    for j in range(ch):
        b = emu.arena(4 * max(n, 1) + 64)
        emu.wr(b, np.asarray(samples[j][:n], dtype=np.float32).tobytes())
        bufs.append(b)
    frame = make_frame(emu, bufs, n, ch)
    emu.call(RAMP, rec_addr, frame, max_insns=10_000_000)
    t2, g2, a2 = struct.unpack("<3f", emu.rd(rec_addr, RECORD_BYTES))
    out = [list(np.frombuffer(emu.rd(b, 4 * n), dtype=np.float32)) for b in bufs]
    return {
        "fs": fs,
        "n": n,
        "channels": ch,
        "rec_in": f32_bits([t, g, a]),
        "rec_out": f32_bits([t2, g2, a2]),
        "in": [f32_bits(samples[j][:n]) for j in range(ch)],
        "out": [f32_bits(o) for o in out],
    }


def ramp_cases(emu, rng):
    cases = []
    # Gate branches: settled at unity, settled at zero, ramp from either side, edge at the gate.
    recs = [
        (1.0, 1.0, 0.0), (1.0, 1.0 - 5e-4, 0.0),  # settled at unity: no multiply, g kept
        (0.0, 0.0, 0.0), (0.0, 5e-4, 0.0),  # both under the gate: zero path
        (0.0, 0.2, 0.0), (1.0, 0.0, 0.0), (0.5, 0.25, 0.0),  # ramp (coefficient filled below)
        (0.0, GATE_T, 0.0), (GATE_T, 0.0, 0.0),  # exact gate boundary
    ]
    for fs in (44100, 48000, 96000):
        a = coeff_for_rate(fs)
        for n in (1, 3, 4, 7, 64):
            for ch in (1, 2, 3, 5):
                for rec in recs:
                    t, g, _ = rec
                    r = (t, g, a)
                    samples = [rng.standard_normal(n).astype(np.float32) for _ in range(ch)]
                    cases.append(ramp_case(emu, rng, fs, n, ch, r, samples))
    # Random records: sweep targets 0..1 and starts over the full range.
    for _ in range(32):
        fs = int(rng.choice([44100, 48000, 96000, 192000]))
        n = int(rng.integers(1, 65))
        ch = int(rng.integers(1, 6))
        rec = (float(rng.uniform(0, 1)), float(rng.uniform(0, 1)), coeff_for_rate(fs))
        samples = [rng.standard_normal(n).astype(np.float32) for _ in range(ch)]
        cases.append(ramp_case(emu, rng, fs, n, ch, rec, samples))
    # n = 0 leaves the record untouched.
    cases.append(ramp_case(emu, rng, 48000, 0, 2, (0.0, 0.5, coeff_for_rate(48000)), [[], []]))
    return cases


def write_ramp(out_path):
    rng = np.random.default_rng(0x5E0F)
    emu = Emulator()
    install_tls(emu)
    set_cpu_mode(emu)
    # 0x1800010d0 (zero-fill) dispatches through this selector; pin it as the magnitude harness does.
    emu.w32(SELECTOR, 0)
    cases = ramp_cases(emu, rng)
    doc = {
        "source": "soothe3 0x899ab0 (gain ramp), emulated; time constant -1/0.03 at [0x18209da5c]",
        "gate": f32_bits([GATE_T])[0],
        "cases": cases,
    }
    Path(out_path).write_text(json.dumps(doc))
    print(f"wrote {len(cases)} ramp cases to {out_path}")


# Setter parameter ids used by the targets fixture (switch cases of 0x894370).
P_WET_DB, P_MIX_PCT, P_OUT_DB, P_DELTA, P_BYPASS, P_SOLO_BAND = 0x5D, 0x5E, 0x5F, 0x60, 0x61, 0x5B
RECORD_T = {"dry": 0xBF28, "wet": 0xBF34, "main": 0xBF40, "solo": 0xBF4C, "out": 0xBF58}


def _normalise(emu, ev):
    """Setter callback [desc+0x50](desc, clamp(x)) -> double in xmm0: offset + x * scale.

    The setter clamps the result to [desc+0x28, desc+0x30] itself. Scale and offset live in the
    descriptor (+0x58, +0x60), so one callback serves every parameter range.
    """
    raw = emu.uc.reg_read(UC_X86_REG_XMM1)
    raw = raw if isinstance(raw, bytes) else int(raw).to_bytes(16, "little")
    x = struct.unpack("<d", raw[:8])[0]
    offset = struct.unpack("<d", emu.rd(ev.rcx + 0x60, 8))[0]
    scale = struct.unpack("<d", emu.rd(ev.rcx + 0x58, 8))[0]
    emu.uc.reg_write(UC_X86_REG_XMM0, int.from_bytes(struct.pack("<d", offset + x * scale) + b"\0" * 8, "little"))
    return None


class ParamSetter:
    """Calls the setter through the wrapper thunk, as the host does (wrapper +0x08)."""

    def __init__(self, sess):
        self.sess = sess
        self.emu = sess.emu
        self.cb = self.emu.stub("output_normalise", _normalise)

    def descriptor(self, pid, lo, hi, scale, offset):
        d = self.emu.arena(0x80)
        self.emu.w64(d, pid)
        self.emu.wf64(d + 0x28, lo)
        self.emu.wf64(d + 0x30, hi)
        self.emu.wf64(d + 0x58, scale)
        self.emu.wf64(d + 0x60, offset)
        self.emu.w64(d + 0x50, self.cb)
        return d

    def set(self, pid, value, lo, hi, scale, offset=0.0):
        """Sets plain `value` through the normalised path: x = (value - offset) / scale."""
        x = (value - offset) / scale
        self.emu.uc.reg_write(UC_X86_REG_XMM2, int.from_bytes(struct.pack("<d", x) + b"\0" * 8, "little"))
        self.emu.call(SETPARAM_THUNK, self.sess.wrapper, self.descriptor(pid, lo, hi, scale, offset), max_insns=1_000_000)


def target_configs(rng):
    """Non-identity output settings: trims, mix, delta, bypass, solo on/off, solo band."""
    cfgs = [
        dict(wet_db=0.0, mix=100.0, out_db=0.0, delta=0, bypass=0, solo_band=0, flag236=0),  # identity
        dict(wet_db=-6.0, mix=50.0, out_db=-3.0, delta=0, bypass=0, solo_band=0, flag236=0),
        dict(wet_db=0.0, mix=30.0, out_db=0.0, delta=1, bypass=0, solo_band=0, flag236=0),
        dict(wet_db=-12.0, mix=80.0, out_db=6.0, delta=0, bypass=1, solo_band=0, flag236=0),
        dict(wet_db=-12.0, mix=80.0, out_db=6.0, delta=1, bypass=1, solo_band=0, flag236=0),
        dict(wet_db=0.0, mix=60.0, out_db=0.0, delta=0, bypass=0, solo_band=3, flag236=0),
        dict(wet_db=0.0, mix=60.0, out_db=0.0, delta=0, bypass=0, solo_band=3, flag236=1),
        dict(wet_db=3.0, mix=45.0, out_db=-9.0, delta=1, bypass=0, solo_band=8, flag236=1),
    ]
    for _ in range(24):
        cfgs.append(dict(
            wet_db=float(rng.uniform(-40, 12)), mix=float(rng.uniform(0, 100)), out_db=float(rng.uniform(-40, 12)),
            delta=int(rng.integers(0, 2)), bypass=int(rng.integers(0, 2)),
            solo_band=int(rng.choice([0, 0, 1, 2, 5, 8])), flag236=int(rng.integers(0, 2)),
        ))
    return cfgs


def write_targets(out_path):
    rng = np.random.default_rng(0x7A26)
    sess = Session(48000)
    emu = sess.emu
    setter = ParamSetter(sess)
    obj = sess.obj
    n = 128
    cases = []
    for cfg in target_configs(rng):
        setter.set(P_WET_DB, cfg["wet_db"], -60.0, 12.0, 72.0, -60.0)
        setter.set(P_MIX_PCT, cfg["mix"], 0.0, 100.0, 100.0)
        setter.set(P_OUT_DB, cfg["out_db"], -60.0, 12.0, 72.0, -60.0)
        setter.set(P_DELTA, float(cfg["delta"]), 0.0, 1.0, 1.0)
        setter.set(P_BYPASS, float(cfg["bypass"]), 0.0, 1.0, 1.0)
        setter.set(P_SOLO_BAND, float(cfg["solo_band"]), 0.0, 8.0, 8.0)
        # Band record byte at obj+0x10c+0x20*idx is the solo flag the block copies into +0xaa.
        for idx in range(8):
            emu.wr(obj + 0x10C + 0x20 * idx, b"\0")
        if cfg["solo_band"] >= 1:
            emu.wr(obj + 0x10C + 0x20 * (cfg["solo_band"] - 1), b"\1")
        emu.wr(obj + 0x236, bytes([cfg["flag236"]]))
        tin, _ = alloc_frames(emu, 2, n, [rng.standard_normal(n).tolist(), rng.standard_normal(n).tolist()])
        tout, _ = alloc_frames(emu, 2, n, [[0.0] * n, [0.0] * n])
        emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, n, max_insns=400_000_000)
        p = {
            "wet_gain": struct.unpack("<f", emu.rd(obj + 0x94, 4))[0],
            "mix": struct.unpack("<f", emu.rd(obj + 0x98, 4))[0],
            "out_gain": struct.unpack("<f", emu.rd(obj + 0x9C, 4))[0],
            "delta": bool(emu.rd(obj + 0xA8, 1)[0]),
            "bypass": bool(emu.rd(obj + 0xA9, 1)[0]),
            "solo_active": bool(emu.rd(obj + 0xAA, 1)[0]),
            "solo_flag_236": bool(emu.rd(obj + 0x236, 1)[0]),
        }
        targets = {k: struct.unpack("<f", emu.rd(obj + off, 4))[0] for k, off in RECORD_T.items()}
        cases.append({
            "cfg": cfg,
            "params": {k: (f32_bits([v])[0] if isinstance(v, float) else v) for k, v in p.items()},
            "targets": {k: f32_bits([v])[0] for k, v in targets.items()},
        })
    doc = {
        "source": "soothe3 process block 0x890a00, output targets 0x891e0e..0x891ed8, via setter 0x894370",
        "evidence_scope": "isolated emulation, constructor stand-in at 48 kHz; solo band byte and +0x236 written directly",
        "cases": cases,
    }
    Path(out_path).write_text(json.dumps(doc, indent=1))
    print(f"wrote {len(cases)} target cases to {out_path}")


def main(argv):
    if len(argv) != 2 or argv[0] not in ("ramp", "targets"):
        raise SystemExit(__doc__)
    if argv[0] == "ramp":
        write_ramp(argv[1])
    else:
        write_targets(argv[1])


if __name__ == "__main__":
    main(sys.argv[1:])
