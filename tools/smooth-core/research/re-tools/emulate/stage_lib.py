"""Call-site snapshots of the normal-path hop stages (translate-analysis pass 3).

`Snap` hooks chosen call sites of the image: at the call it records the four Win64
register arguments, the 5th/6th stack arguments and the first `n` floats behind each
listed pointer argument; at the return address it records the same arrays again.
Each record is (site name, args, before arrays, after arrays). Used by
stage_fixture.py to write the stage fixtures the Rust tests read.

Evidence scope: isolated emulation, constructor state, synthetic host (process_lib).
"""

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import unicorn.x86_const as UC  # noqa: E402
from capstone import Cs, CS_ARCH_X86, CS_MODE_64  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

from pe_emu import IMAGE_BASE  # noqa: E402

REGS = {"rcx": UC.UC_X86_REG_RCX, "rdx": UC.UC_X86_REG_RDX, "r8": UC.UC_X86_REG_R8, "r9": UC.UC_X86_REG_R9}


def floats(emu, ptr, n):
    return list(struct.unpack(f"<{n}f", emu.rd(ptr, 4 * n)))


def _n(n, args):
    """Array length: an int, or a function of the call's arguments."""
    return n(args) if callable(n) else n


class Snap:
    def __init__(self, emu, sites):
        """sites: {name: (call-site RVA, {arg: n_floats or f(args)})}; arg is a register or 's5'/'s6'."""
        self.emu = emu
        self.records = []
        self.active = False
        md = Cs(CS_ARCH_X86, CS_MODE_64)
        for name, (rva, arrays) in sites.items():
            va = IMAGE_BASE + rva
            ins = next(md.disasm(emu.rd(va, 16), va))
            assert ins.mnemonic == "call", f"{name}: {rva:#x} is {ins.mnemonic}"
            pending = []
            emu.uc.hook_add(UC_HOOK_CODE, self._enter(name, arrays, pending), begin=va, end=va)
            ret = va + ins.size
            emu.uc.hook_add(UC_HOOK_CODE, self._leave(pending), begin=ret, end=ret)

    def _args(self):
        emu = self.emu
        a = {k: emu.reg(r) for k, r in REGS.items()}
        rsp = emu.reg(UC.UC_X86_REG_RSP)
        a["s5"], a["s6"] = (struct.unpack("<Q", emu.rd(rsp + 0x20 + 8 * i, 8))[0] for i in range(2))
        for i in range(4):
            raw = emu.uc.reg_read(getattr(UC, f"UC_X86_REG_XMM{i}")).to_bytes(16, "little")
            a[f"xmm{i}"] = struct.unpack("<f", raw[:4])[0]
        return a

    def _enter(self, name, arrays, pending):
        def cb(uc, addr, size, user):
            if not self.active:
                return
            a = self._args()
            before = {k: floats(self.emu, a[k], _n(n, a)) for k, n in arrays.items() if a[k]}
            rec = {"site": name, "args": a, "before": before, "after": None}
            pending.append((rec, arrays))
            self.records.append(rec)
        return cb

    def _leave(self, pending):
        def cb(uc, addr, size, user):
            if not pending:
                return
            rec, arrays = pending.pop()
            a = rec["args"]
            rec["after"] = {k: floats(self.emu, a[k], _n(n, a)) for k, n in arrays.items() if a[k]}
        return cb
