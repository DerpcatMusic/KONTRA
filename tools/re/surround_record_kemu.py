"""Record -> native Surround Panner, end to end, under kemu.

    python3 -I tools/re/surround_record_kemu.py KONTAKT8_EXE OUT_JSON

The real sequence for a version 0x70 record (no field is poked by hand):
  1. factory body of the DSP object (surround_panner_driver_kemu.build),
  2. reader 0x140d02f60 (vtable slot 8 of BParFXSurroundPanner) on a record object P, fed by a
     stream wrapper whose inner stream copies the crafted record bytes,
  3. apply: the load path case 6 (0x1409619b0 at 0140960000.c:1493) calls the setter
     0x1409685e0 with the fields of P (SIZE, DIVERGENCE, LFE dB, offsets, gain mode, layout, ...),
  4. per block the driver 0x140905fa0 (mode 0), with source positions written at +0x284.

Dumps P and the DSP object fields after the apply, and the stereo output blocks. Scenarios vary one
record field at a time (one-at-a-time sweep), plus a per-block SIZE step that re-applies.
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import surround_panner_driver_kemu as drv  # noqa: E402
from kemu import Emu, STUBS  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RDX, UC_X86_REG_R8  # noqa: E402

READER, SETTER = 0x140D02F60, 0x1409685E0
REC_READ = STUBS + 0xFE000  # inner stream read stub (kemu RET sentinel is STUBS + 0xFF000)
P_SIZE = 0x1D80
N, BLOCKS = drv.N, drv.BLOCKS
SPEAKERS = drv.SPEAKERS
FIELDS = {  # DSP object fields written by the apply (native names from the setter)
    "scale_348": 0x348, "exp_34c": 0x34C, "off_x_350": 0x350, "off_y_354": 0x354,
    "radius_358": 0x358, "angle_35c": 0x35C, "gain_mode_394": 0x394, "layout_398": 0x398,
    "spk_count_280": 0x280, "src_count_344": 0x344, "lfe_obj_7b4": 0x7B4, "c04": 0xC04,
    "c8c": 0xC8C, "f2498": 0x2498, "f263c": 0x263C, "f2638": 0x2638, "f2640": 0x2640,
    "f380": 0x380, "f384": 0x384, "f388": 0x388, "f38c": 0x38C,
}

INT_FIELDS = {0x280, 0x344, 0x394, 0x398, 0x263C, 0x2638, 0x2640}

BASE = dict(
    speakers=[(x, y, 0) for x, y in SPEAKERS],
    sources=[(0.8, 0.3, 0), (0.3, 0.6, 0)],
    p194=0.0, lfe=0.0, size=0.5, div=0.5, p1bc=0.0, p1c0=0.0, mode=0, layout=0,
    flag_a=0, flag_b=0, flag_c=0, dist=0.0, az=0.0, ox=0.0, oy=0.0, p1cc=0,
)


def f32bits(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def encode(r):
    b = bytearray(struct.pack("<i", len(r["speakers"])))
    for x, y, d in r["speakers"]:
        b += struct.pack("<ffB", x, y, d)
    b += struct.pack("<i", len(r["sources"]))
    for x, y, d in r["sources"]:
        b += struct.pack("<ffB", x, y, d)
    b += struct.pack("<ffff", r["p194"], r["lfe"], r["size"], r["div"])
    b += struct.pack("<ff", r["p1bc"], r["p1c0"])
    b += struct.pack("<ii", r["mode"], r["layout"])
    b += struct.pack("<BBB", r["flag_a"], r["flag_b"], r["flag_c"])
    b += struct.pack("<ffff", r["dist"], r["az"], r["ox"], r["oy"])
    b += struct.pack("<i", r["p1cc"])
    return bytes(b)


class RecEmu(Emu):
    """Emu with one extra stub: the inner stream read used by the reader wrapper."""

    def __init__(self, path):
        super().__init__(path)
        self.rec, self.pos = b"", 0
        self.uc.mem_write(REC_READ, b"\xc3")

    def _stub(self, uc, addr, size, _):
        if addr == REC_READ:
            dst, n = uc.reg_read(UC_X86_REG_RDX), uc.reg_read(UC_X86_REG_R8)
            chunk = self.rec[self.pos:self.pos + n]
            if len(chunk) != n:
                raise RuntimeError("record short by %d bytes" % (n - len(chunk)))
            uc.mem_write(dst, chunk)
            self.pos += n
            return
        return super()._stub(uc, addr, size, _)

    def read_record(self, r):
        """Run the reader on `r`; returns the record object P."""
        self.rec, self.pos = encode(r), 0
        p = self.alloc(P_SIZE)
        inner = self.alloc(0x20)
        vt = self.alloc(0x40)
        self.write(vt + 0x10, "Q", REC_READ)
        self.write(inner, "Q", vt)
        wrap = self.alloc(0x20)
        self.write(wrap + 8, "Q", inner)
        self.write(wrap + 0x10, "i", 1)  # little-endian: no byte swap in the reader
        self.call(READER, p, wrap, 0x70, limit=2_000_000)
        if self.pos != len(self.rec):
            raise RuntimeError("reader consumed %d of %d bytes" % (self.pos, len(self.rec)))
        return p

    def apply(self, obj, p):
        """The case-6 apply: vtable slot 25, the flag at +0xc, then the setter with P's fields."""
        vt = self.read(obj, "Q")
        self.call(self.read(vt + 200, "Q"), obj, limit=2_000_000)
        self.write(obj + 0xC, "B", 0)
        spk, src = p + 8, p + 0xCC
        f = lambda off: self.read(p + off, "f")  # noqa: E731
        u = lambda off: self.read(p + off, "I")  # noqa: E731
        ub = lambda off: self.read(p + off, "B")  # noqa: E731
        floats = {3: f(0x19C)}  # xmm3 for position 3 (SIZE)
        stack = [
            f32bits(f(0x1A0)), f32bits(f(0x194)), f32bits(f(0x198)),  # DIVERGENCE, p6, LFE dB
            ub(0x1A4), ub(0x1A5), ub(0x1A6),
            u(0x1BC), u(0x1B8), u(0x1C0), u(0x1C4), u(0x1C8),
            f32bits(f(0x1B0)), f32bits(f(0x1B4)), f32bits(f(0x1A8)), u(0x1AC), 0,
        ]
        self.call(SETTER, obj, spk, src, 0, *stack, f=floats, limit=5_000_000)

    def dump_record(self, p):
        """The record object after the reader, at the offsets the setter and the caller read."""
        return {hex(o): self.read(p + o, "I") for o in (0x8, 0xC8, 0xCC, 0x18C, 0x194, 0x198,
                0x19C, 0x1A0, 0x1A4, 0x1A5, 0x1A6, 0x1A8, 0x1AC, 0x1B0, 0x1B4, 0x1B8, 0x1BC,
                0x1C0, 0x1C4, 0x1C8, 0x1CC)}

    def dump(self, obj):
        return {k: self._field(obj, off) for k, off in FIELDS.items()}

    def _field(self, obj, off):
        raw = self.read(obj + off, "I")
        if off in INT_FIELDS:
            return raw
        return struct.unpack("<f", struct.pack("<I", raw))[0]


def run_case(e, name, rec, blocks=BLOCKS, sizes=None, sources=None, rate=48000.0):
    obj = drv.build(e)
    ins, outs = [], []
    for s in range(len(SPEAKERS)):
        buf = e.alloc(4 * N)
        e.call(drv.SET_INPUT, obj, buf, s, limit=2_000_000)
        ins.append(buf)
    for k in range(len(SPEAKERS)):
        buf = e.alloc(4 * N)
        e.call(drv.SET_OUTPUT, obj, buf, k, limit=2_000_000)
        outs.append(buf)
    scratch = e.alloc(drv.SCRATCH)
    e.write(obj + 0x120, "i", N)
    e.write(obj + 0x7B4, "f", rate)  # the setter divides by it: p6 * pi / rate (sample rate)
    p = e.read_record(rec)
    e.apply(obj, p)
    e.call(drv.RESET, obj, limit=2_000_000)
    row = {"name": name, "rec": dict(rec), "rate": rate, "p_dump": e.dump_record(p), "speakers": SPEAKERS,
           "blocks": [], "obj_after_apply": e.dump(obj)}
    src_pos = [list(s[:2]) for s in (sources or rec["sources"])]
    for blk in range(blocks):
        if sizes is not None and sizes[blk] != rec["size"]:
            rec2 = dict(rec, size=sizes[blk])
            p = e.read_record(rec2)
            e.apply(obj, p)
        for s, (x, y) in enumerate(src_pos):
            e.write(obj + 0x284 + 12 * s, "f", x, y, 0.0)
        inputs = [drv.sine(N, blk, s) if s < len(src_pos) else [0.0] * N for s in range(len(SPEAKERS))]
        for s, buf in enumerate(ins):
            e.write(buf, "f", *inputs[s])
        for buf in outs:
            e.write(buf, "f", *([0.0] * N))
        e.call(drv.DRIVER, obj, scratch, 0, limit=5_000_000)
        row["blocks"].append({"input": inputs, "out": [list(e.read(b, "f", N)) for b in outs],
                              "sources": src_pos, "params": e.dump(obj)})
    return row


def main():
    exe, dst = sys.argv[1:3]
    e = RecEmu(exe)
    e.call(drv.TABLES, limit=2_000_000)
    cases = [run_case(e, "base", dict(BASE))]
    # one-at-a-time sweep: each field moved from BASE, to see what the apply and the block output do
    sweep = {
        "size": 0.2, "div": 0.2, "lfe": -6.0, "p194": 0.7, "dist": 0.4, "az": 0.3, "ox": 0.2,
        "oy": -0.1, "mode": 1, "layout": 2, "p1bc": 1.0, "p1c0": 1.0, "flag_a": 1,
        "flag_b": 1, "flag_c": 1,
    }
    # flag_a (+0x1a4 -> +0xc8c) and flag_c (+0x1a6 -> +0x2498) at 1 raise an exception inside the
    # driver under this harness (the rebuild path 0x140ac1dc0); flag_b=1 gives NaN. Not swept.
    for k, v in sweep.items():
        if k in ("flag_a", "flag_b", "flag_c"):
            continue
        cases.append(run_case(e, "sweep_" + k, dict(BASE, **{k: v}), blocks=6))
    cases.append(run_case(e, "size_steps", dict(BASE), blocks=6,
                          sizes=[0.5, 0.5, 0.3, 0.3, 0.7, 0.7]))
    for rate in (44100.0, 96000.0):
        cases.append(run_case(e, "rate_%d" % rate, dict(BASE), blocks=6, rate=rate))
    cases.append(run_case(e, "sweep_p194_rate96000", dict(BASE, p194=0.7), blocks=6, rate=96000.0))
    with open(dst, "w") as f:
        json.dump({"exe_note": "Kontakt8.exe via kemu; see module docstring", "cases": cases},
                  f, separators=(",", ":"))
    for c in cases:
        print(c["name"], "blocks", len(c["blocks"]), "obj", {k: round(v, 4) for k, v in c["obj_after_apply"].items() if k in ("scale_348", "exp_34c", "off_x_350", "radius_358", "angle_35c", "gain_mode_394", "lfe_obj_7b4")}, flush=True)


if __name__ == "__main__":
    main()
