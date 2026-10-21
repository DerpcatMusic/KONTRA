"""Call-tree trace of the normal (non-zero-latency) process path, for translation.

Runs K blocks of the process block (entry 0x890a00) on the constructor state from
process_lib.Session, with an instruction hook that fires only at `call` sites of the
image (and at their return addresses, to snapshot results). For the traced block it
writes one record per call: caller RVA, callee RVA or stub name, the four Win64
register arguments, the two stack arguments (5th and 6th), and the callee's rax on
return. Pointer arguments are followed by a memory snapshot of the first `--peek`
floats at call entry and after return, so the stage arrays can be checked against
the Rust translation.

Output: oracle/call_trace_<rate>_m<mode>.txt (one line per call, indented by depth).
Depth is the number of open calls whose return address has not been reached yet.

Run with: python -I call_trace.py --rate 48000 --block 2 [--peek 16]
"""

import argparse
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import unicorn.x86_const as UC  # noqa: E402
from capstone import Cs, CS_ARCH_X86, CS_MODE_64  # noqa: E402
from capstone.x86 import X86_OP_IMM, X86_OP_MEM, X86_OP_REG  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

from disasm_rva import name_of, load_starts  # noqa: E402
from pe_emu import IMAGE_BASE  # noqa: E402
from process_lib import Session, alloc_frames  # noqa: E402
from process_fixture import f32  # noqa: E402  (rounds to f32 like the fixtures)
import process_oracle as po  # noqa: E402

TEXT_LO, TEXT_HI = 0x1000, 0x1E00000  # .text RVA range (corpus rows run to about 0x1dae000)
ZL_OFFSET = po.ZL_OFFSET


def reg_const(name):
    return getattr(UC, "UC_X86_REG_" + name.upper())


def call_sites(emu):
    """RVA -> (VA, size) of every direct or indirect call instruction in the text range."""
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    sites = {}
    # Disassemble per corpus function: a linear sweep over the whole text stops at the
    # first data island, so each manifest row is decoded from its own start.
    for start, end in load_starts():
        if not (TEXT_LO <= start < TEXT_HI):
            continue
        raw = emu.rd(IMAGE_BASE + start, end - start)
        for ins in md.disasm(raw, IMAGE_BASE + start):
            if ins.mnemonic == "call":
                sites[ins.address] = ins.size
    return sites


class Tracer:
    def __init__(self, emu, peek):
        self.emu = emu
        self.md = Cs(CS_ARCH_X86, CS_MODE_64)
        self.md.detail = True
        self.sites = call_sites(emu)
        self.pending = {}  # return VA -> list of records awaiting the return
        self.lines = []
        self.depth = 0
        self.peek = peek
        self.starts = load_starts()
        self.active = False
        self.calls = 0

    def target(self, addr):
        """Resolve the callee VA of the call instruction at addr (before it executes)."""
        emu = self.emu
        ins = next(self.md.disasm(emu.rd(addr, 16), addr))
        op = ins.operands[0]
        if op.type == X86_OP_IMM:
            return op.imm
        if op.type == X86_OP_REG:
            return emu.reg(reg_const(ins.reg_name(op.reg)))
        m = op.mem
        if m.base and ins.reg_name(m.base) == "rip":
            a = addr + ins.size + m.disp
        else:
            a = m.disp
            if m.base:
                a += emu.reg(reg_const(ins.reg_name(m.base)))
            if m.index:
                a += emu.reg(reg_const(ins.reg_name(m.index))) * m.scale
        return struct.unpack("<Q", emu.rd(a, 8))[0]

    def _fmt_name(self, va):
        if va in self.emu.stub_names:
            return self.emu.stub_names[va]
        n = name_of(self.emu, va, self.starts)
        return n or f"{va - IMAGE_BASE:#x}"

    def _peek(self, ptr):
        if not ptr or not (IMAGE_BASE < ptr < IMAGE_BASE + 0x40000000 or ptr > 0x10000):
            return None
        try:
            return struct.unpack(f"<{self.peek}f", self.emu.rd(ptr, 4 * self.peek))
        except Exception:
            return None

    def on_code(self, uc, addr, size, user):
        if not self.active:
            return
        if addr in self.pending:
            rax = self.emu.reg(UC.UC_X86_REG_RAX)
            for rec in self.pending.pop(addr):
                self.depth -= 1
                rec["ret_rax"] = rax
                if self.peek:
                    rec["after"] = {k: self._peek(rec["args"][k]) for k in ("rcx", "rdx", "r8", "r9")}
                self.lines.append(f"{'  ' * self.depth}<- {rec['callee']} rax={rax:#x}")
                if self.peek and rec.get("after"):
                    for k, v in rec["after"].items():
                        if v:
                            self.lines.append(f"{'  ' * (self.depth + 1)}after {k}={[round(x, 6) for x in v[:8]]}")
        if addr in self.sites:
            emu = self.emu
            tgt = self.target(addr)
            rsp = emu.reg(UC.UC_X86_REG_RSP)
            args = {
                "rcx": emu.reg(UC.UC_X86_REG_RCX), "rdx": emu.reg(UC.UC_X86_REG_RDX),
                "r8": emu.reg(UC.UC_X86_REG_R8), "r9": emu.reg(UC.UC_X86_REG_R9),
            }
            stack = [struct.unpack("<Q", emu.rd(rsp + 0x20 + 8 * i, 8))[0] for i in range(2)]
            callee = self._fmt_name(tgt)
            rec = {"caller": addr - IMAGE_BASE, "callee": callee, "args": args, "stack": stack}
            self.calls += 1
            head = f"{'  ' * self.depth}{addr - IMAGE_BASE:#08x} -> {callee}"
            args_s = " ".join(f"{k}={v:#x}" for k, v in args.items())
            self.lines.append(f"{head} {args_s} s5={stack[0]:#x} s6={stack[1]:#x}")
            if self.peek:
                rec["before"] = {k: self._peek(args[k]) for k in ("rcx", "rdx", "r8", "r9")}
                for k, v in rec["before"].items():
                    if v:
                        self.lines.append(f"{'  ' * (self.depth + 1)}before {k}={[round(x, 6) for x in v[:8]]}")
            ret = addr + self.sites[addr]
            self.pending.setdefault(ret, []).append(rec)
            self.depth += 1


def run(rate, mode, block, peek, out_dir):
    sess = Session(rate)
    sess.prepare()
    if mode == 1:
        sess.emu.wr(sess.obj + ZL_OFFSET, b"\x01")
    emu = sess.emu
    tr = Tracer(emu, peek)
    emu.uc.hook_add(UC_HOOK_CODE, tr.on_code, begin=IMAGE_BASE + TEXT_LO, end=IMAGE_BASE + TEXT_HI - 1)
    rnd = __import__("random").Random(20261021)
    total = (block + 1) * po.N
    inp = [[f32(rnd.uniform(-0.5, 0.5)) for _ in range(total)] for _ in range(2)]
    for k in range(block + 1):
        tr.active = k == block
        seg = [inp[c][k * po.N:(k + 1) * po.N] for c in range(2)]
        tin, _ = alloc_frames(emu, 2, po.N, seg)
        tout, _ = alloc_frames(emu, 2, po.N, [[0.0] * po.N, [0.0] * po.N])
        emu.call(po.PROCESS_THUNK, sess.wrapper, tout, tin, 0, po.N, max_insns=400_000_000)
    out = out_dir / f"call_trace_{rate}_m{mode}.txt"
    out.write_text("\n".join(tr.lines) + "\n")
    print(out, "calls", tr.calls, "lines", len(tr.lines))


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--rate", type=int, default=48000)
    ap.add_argument("--mode", type=int, default=0)
    ap.add_argument("--block", type=int, default=2)
    ap.add_argument("--peek", type=int, default=0)
    ap.add_argument("--out-dir", default="/data/dev/workspace/RunScratch/smooth/call-trace")
    a = ap.parse_args(argv)
    out_dir = Path(a.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    run(a.rate, a.mode, a.block, a.peek, out_dir)


if __name__ == "__main__":
    main(sys.argv[1:])
