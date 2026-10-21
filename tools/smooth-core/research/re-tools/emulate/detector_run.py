"""Run the reduction detector (0x619fc0) on a host-filled state, fault by fault.

Chain (see ../../re-latency-fft-scale.md sections 9-14):
  ReductionCurve::slot2 (0x6b96f0) -> detector 0x619fc0(frame, engine, host, rect)
  engine+0x648 = S (detector state). S holds std::map trees and bin/weight vectors.
  Record dispatcher 0x621d50 fills S: type 0 -> bins1 (S+0x770), 1 -> bins2 (S+0x788),
  2 -> W1 (S+0x7a0), 3 -> W2 (S+0x7b8).

Each run stops at the first fault. The report gives the faulting PC, its function
(from the corpus manifest), and the instructions around it, so the next fix can be
read from the code. Nothing here guesses a plug-in default: the host values are our
hypotheses and are printed.

Run with: python -I detector_run.py [--rate 48000] [--bins 1025]
"""

import bisect
import math
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from capstone import Cs, CS_ARCH_X86, CS_MODE_64  # noqa: E402

from fake_host import OBJ_SIZE, make_host, make_cfg, run_ctor  # noqa: E402
from pe_emu import IMAGE_BASE, image_va  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_RAX,
    UC_X86_REG_RBP,
    UC_X86_REG_RBX,
    UC_X86_REG_RCX,
    UC_X86_REG_RDX,
    UC_X86_REG_RSI,
    UC_X86_REG_RSP,
)

REG_NAMES = {
    "rax": UC_X86_REG_RAX,
    "rbx": UC_X86_REG_RBX,
    "rcx": UC_X86_REG_RCX,
    "rdx": UC_X86_REG_RDX,
    "rsi": UC_X86_REG_RSI,
    "rsp": UC_X86_REG_RSP,
    "rbp": UC_X86_REG_RBP,
}

DETECTOR = image_va(0x619FC0)
DISPATCH = image_va(0x621D50)
MANIFEST = Path("/data/dev/workspace/RunScratch/smooth/re-corpus/corpus/manifest.tsv")
PLOT_W = 1024.0
PLOT_H = 512.0

S_OFF = 0x648
BINS1 = 0x770
BINS2 = 0x788
W1 = 0x7A0
W2 = 0x7B8


def load_manifest():
    rows = []
    with MANIFEST.open() as f:
        next(f)
        for line in f:
            rva, end, _size, _sec = line.split()
            rows.append((int(rva, 16), int(end, 16)))
    rows.sort()
    return rows


_MANIFEST = None


def function_of(pc):
    """Return (rva_start, rva_end) of the function containing pc, or None."""
    global _MANIFEST
    if _MANIFEST is None:
        _MANIFEST = load_manifest()
    rva = pc - IMAGE_BASE
    starts = [r[0] for r in _MANIFEST]
    i = bisect.bisect_right(starts, rva) - 1
    if i < 0:
        return None
    start, end = _MANIFEST[i]
    return (start, end) if rva < end else None


def disasm_around(emu, pc, before=8, after=6):
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    lo = pc - 0x40
    code = emu.rd(lo, 0x80)
    out = []
    for ins in md.disasm(code, lo):
        if ins.address > pc + after * 8:
            break
        if ins.address >= pc - before * 4:
            marker = "=>" if ins.address == pc else "  "
            out.append(f"{marker} {ins.address:#x}  {ins.mnemonic} {ins.op_str}")
    return "\n".join(out)


def report_fault(emu, exc):
    text = str(exc)
    print("FAULT:", text)
    if "pc=" in text:
        pc = int(text.split("pc=")[1].split()[0].rstrip(","), 16)
        fn = function_of(pc)
        if fn:
            print(f"  in function rva {fn[0]:#x}..{fn[1]:#x}")
        print(disasm_around(emu, pc))
        regs = {n: emu.reg(c) for n, c in REG_NAMES.items()}
        print("  regs:", " ".join(f"{n}={v:#x}" for n, v in regs.items()))


def f32s(values):
    return struct.pack(f"<{len(values)}f", *values)


def build_state(emu, obj, rate, bins):
    """Allocate S, point obj+0x648 at it, and fill the bin and weight vectors."""
    S = emu.arena(0x1000)
    emu.w64(obj + S_OFF, S)
    # Map lookups 0x563970(tree = *(S+t), key = *(S+k)) return at once when the
    # tree's flag byte (+0x48) is 0, so an empty flag-clear tree is a no-op
    # stand-in. The real trees are filled by the host (not decoded yet).
    for tree_off in (0x550, 0x4B0, 0x570):
        tree = emu.arena(0x80)
        emu.w64(S + tree_off, tree)
    # *(S+0x540) is the lookup's result object (read at +0x2c/+0x3c).
    emu.w64(S + 0x540, emu.arena(0x100))
    # **(S+0x560) is a byte flag: S+0x560 -> cell holding that byte.
    flag_cell = emu.arena(16)
    emu.w64(S + 0x560, flag_cell)
    # **(S+0x4a0) is the group count: S+0x4a0 -> cell holding the int.
    groups_cell = emu.arena(16)
    emu.w32(groups_cell, 1)
    emu.w64(S + 0x4A0, groups_cell)
    return S


def dispatch(emu, S, rtype, payload):
    """Call the record dispatcher 0x621d50 with one record of `payload` floats."""
    holder = emu.arena(8)
    emu.w64(holder, S)
    data = emu.arena(max(4 * len(payload), 16))
    emu.wr(data, f32s(payload))
    rec = emu.arena(0x40)
    emu.w32(rec, rtype)
    emu.w64(rec + 0x28, data)
    emu.w64(rec + 0x30, 4 * len(payload))
    emu.call(DISPATCH, holder, rec, max_insns=10_000_000)


def build_frame(emu, samples):
    """Channel list (the detector's arg1): +0x38 qword channel pointers, +0x40 count, +0x48 samples.

    The detector writes its working block at arg1+0x120..0x2190 (see section 16), so the
    object must be far larger than the channel list; a 0x100-byte arena let those writes
    run into the sample buffer. The samples are written here: an all-zero buffer is silence.
    """
    n = len(samples)
    frame = emu.arena(0x4000)
    buf = emu.arena(4 * n)
    emu.wr(buf, struct.pack(f"<{n}f", *samples))
    emu.w64(frame + 0x38, buf)
    emu.w32(frame + 0x40, 1)
    emu.w32(frame + 0x48, n)
    return frame


def main(argv):
    rate = 48000.0
    bins = 1025
    amp = 0.9
    groups = 1
    args = list(argv)
    while args:
        a = args.pop(0)
        if a == "--rate":
            rate = float(args.pop(0))
        elif a == "--bins":
            bins = int(args.pop(0))
        elif a == "--amp":
            amp = float(args.pop(0))
        elif a == "--groups":
            groups = int(args.pop(0))

    emu, obj, _log = run_ctor(rate)
    S = build_state(emu, obj, rate, bins)
    # **(S+0x4a0) is the group count the detector reads (1 at the stand-in default).
    emu.w32(emu.r64(S + 0x4A0), groups)
    step = rate / (2 * (bins - 1))
    freqs = [k * step for k in range(bins)]
    unity = [1.0] * bins
    dispatch(emu, S, 0, freqs)
    dispatch(emu, S, 1, freqs)
    dispatch(emu, S, 2, unity * groups)
    dispatch(emu, S, 3, unity * groups)
    print(f"host fill: rate={rate} bins={bins} groups={groups} N={2 * (bins - 1)} step={step:.3f} Hz")

    n = 2048
    samples = [amp * math.sin(2 * math.pi * 1000.0 * i / rate) for i in range(n)]
    frame = build_frame(emu, samples)
    host_ptr = emu.arena(8)
    host_block = emu.arena(0x2000)
    emu.w64(host_ptr, host_block)
    rect_lo = struct.unpack("<Q", struct.pack("<ff", 0.0, 0.0))[0]
    rect_hi = struct.unpack("<Q", struct.pack("<ff", PLOT_W, PLOT_H))[0]
    try:
        r = emu.call(DETECTOR, frame, obj, host_ptr, rect_lo, rect_hi, max_insns=400_000_000)
        print("detector returned", hex(r), "aborted=", emu.aborted)
        out = [emu.rf32(host_block + 0x68 + 4 * i) for i in range(2022)]
        print(f"host block 0x68..0x1ffe: min={min(out):.4f} max={max(out):.4f} (amp={amp}, groups={groups})")
    except RuntimeError as exc:
        report_fault(emu, exc)


if __name__ == "__main__":
    main(sys.argv[1:])
