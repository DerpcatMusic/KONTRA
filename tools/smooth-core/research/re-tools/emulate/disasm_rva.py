"""Disassemble an RVA range of the restored image, annotating call targets.

Reads bytes through the emulator loader (the same bytes the emulator executes), so
IAT slots are resolved to their stub names. Corpus function starts come from
manifest.tsv. Usage (from this directory):
  python -I disasm_rva.py 0x890a00 0x40        # 0x40 bytes from RVA 0x890a00
  python -I disasm_rva.py 0x890a00 0x40 --insns  # count is an instruction count
"""

import bisect
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from capstone import Cs, CS_ARCH_X86, CS_MODE_64  # noqa: E402
from capstone.x86 import X86_OP_IMM, X86_OP_MEM  # noqa: E402

from pe_emu import IMAGE_BASE, Emulator  # noqa: E402

MANIFEST = Path("/data/dev/workspace/RunScratch/smooth/re-corpus/corpus/manifest.tsv")


def load_starts():
    rows = []
    with MANIFEST.open() as f:
        next(f)
        for line in f:
            rva, end, _size, _sec = line.split()
            rows.append((int(rva, 16), int(end, 16)))
    rows.sort()
    return rows


def name_of(emu, va, starts):
    """Stub or IAT name for a VA, else the corpus function that contains it."""
    if va in emu.stub_names:
        return emu.stub_names[va]
    rva = va - IMAGE_BASE
    i = bisect.bisect_right([s for s, _ in starts], rva) - 1
    if i >= 0 and rva < starts[i][1]:
        return f"fn_{starts[i][0]:06x}"
    return None


def disasm(emu, start_rva, count, as_insns=False):
    starts = load_starts()
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    va = IMAGE_BASE + start_rva
    raw = emu.rd(va, count * 16 if as_insns else count)
    out = []
    n = 0
    for ins in md.disasm(raw, va):
        note = ""
        if ins.mnemonic in ("call", "jmp") and ins.operands:
            op = ins.operands[0]
            if op.type == X86_OP_IMM:
                tgt = name_of(emu, op.imm, starts)
                note = f"  ; {tgt or hex(op.imm)}"
            elif op.type == X86_OP_MEM and op.mem.base == 41:  # rip-relative
                slot = ins.address + ins.size + op.mem.disp
                tgt = emu.iat_slots.get(slot)
                note = f"  ; [{hex(slot)}] {tgt or ''}".rstrip()
        out.append(f"{ins.address - IMAGE_BASE:#08x}  {ins.bytes.hex():<20} {ins.mnemonic} {ins.op_str}{note}")
        n += 1
        if as_insns and n >= count:
            break
        if not as_insns and ins.address - va + ins.size >= count:
            break
    return "\n".join(out)


def main(argv):
    start = int(argv[0], 16)
    count = int(argv[1], 16) if argv[1].startswith("0x") else int(argv[1])
    as_insns = "--insns" in argv
    emu = Emulator()
    print(disasm(emu, start, count, as_insns))


if __name__ == "__main__":
    main(sys.argv[1:])
