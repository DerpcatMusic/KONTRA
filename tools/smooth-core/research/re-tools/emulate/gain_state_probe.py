"""Field listing for the gain body 0x8e04c0..0x8e4be2 (section 20 of ../../re-latency-fft-scale.md).

Runs process blocks on the stand-in state (same setup as gain_chain_probe.py) and records
every memory access made while the program counter is inside the body. Each access is
attributed to a base region that was live at the body entry:

  obj   rcx at entry (the DSP object, gate byte at +0x2bc8)
  tabA  rdx at entry (channel-pointer table)
  tabB  r8 at entry (channel-pointer table, the input frames)
  opt   r9 at entry (third pointer; null in the stand-in)
  frame the stack frame of the body (rsp at entry, 0x4ae8 + prologue below it)
  other anything else (heap/arena not reached from the bases above)

Pointers loaded from a base (8-byte reads) become sub-regions named `<base>[+off]`, so
the per-channel buffers reached through tabA/tabB are listed as their own regions.

Output: a JSON listing (one row per region, offset, size, op) with pcs and sample values,
written to the path given on the command line (default: gain_state_listing.json next to
this file). Evidence scope: isolated emulation, stand-in state, default parameters. The
stand-in is the identity band state, so fields that only matter for non-zero band state
will not appear here (see section 17.8 of the notes).

Run with: python -I gain_state_probe.py [out.json] [blocks]
"""

import collections
import json
import math
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from unicorn import UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE, UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_R9, UC_X86_REG_RSP, UC_X86_REG_RIP,
)

from process_lib import PROCESS_THUNK, Session, alloc_frames  # noqa: E402
from pe_emu import IMAGE_BASE, image_va  # noqa: E402

BODY_LO = image_va(0x8E04C0)
BODY_HI = image_va(0x8E4BE2)
ENTRY = image_va(0x8E04C0)
N = 512
FRAME_SPAN = 0x6000  # body frame 0x4ae8 plus pushes and the call-in slack
OBJ_SPAN = 0x4000  # covers the 0x2bc8 gate and the fields after it
TAB_SPAN = 0x100
MAX_CHILD_SPAN = 0x2000


class Listing:
    def __init__(self):
        self.rows = collections.defaultdict(lambda: {"pcs": set(), "values": []})
        self.entries = []

    def add(self, region, off, size, op, pc, value):
        key = (region, off, size, op)
        row = self.rows[key]
        row["pcs"].add(pc - IMAGE_BASE)
        if len(row["values"]) < 4:
            row["values"].append(value)

    def dump(self, path):
        out = []
        for (region, off, size, op), row in sorted(self.rows.items()):
            out.append({
                "region": region,
                "offset": off,
                "size": size,
                "op": op,
                "pcs": [f"{p:#x}" for p in sorted(row["pcs"])][:8],
                "samples": row["values"],
            })
        Path(path).write_text(json.dumps({"entries": self.entries, "accesses": out}, indent=1))


def main():
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("gain_state_listing.json")
    blocks = int(sys.argv[2]) if len(sys.argv) > 2 else 4
    sess = Session(48000)
    emu = sess.emu
    uc = emu.uc
    sess.prepare()
    listing = Listing()
    state = {"active": False, "regions": []}

    def in_body():
        pc = emu.reg(UC_X86_REG_RIP)
        return BODY_LO <= pc < BODY_HI

    def classify(addr):
        for lo, hi, name in state["regions"]:
            if lo <= addr < hi:
                return name, addr - lo
        return "other", addr

    def on_entry(uc_, addr, size, user):
        rcx = emu.reg(UC_X86_REG_RCX)
        rdx = emu.reg(UC_X86_REG_RDX)
        r8 = emu.reg(UC_X86_REG_R8)
        r9 = emu.reg(UC_X86_REG_R9)
        rsp = emu.reg(UC_X86_REG_RSP)
        state["regions"] = [
            (rcx, rcx + OBJ_SPAN, "obj"),
            (rdx, rdx + TAB_SPAN, "tabA"),
            (r8, r8 + TAB_SPAN, "tabB"),
            (rsp - FRAME_SPAN, rsp + 0x100, "frame"),
        ]
        if r9:
            state["regions"].append((r9, r9 + TAB_SPAN, "opt"))
        state["active"] = True
        listing.entries.append({"rcx": hex(rcx), "rdx": hex(rdx), "r8": hex(r8), "r9": hex(r9), "rsp": hex(rsp)})

    def learn_pointers(addr, size, base_name):
        # an 8-byte read of a pointer inside a table: expose the pointee as its own region
        if size != 8:
            return
        raw = bytes(uc.mem_read(addr, 8))
        ptr = struct.unpack("<Q", raw)[0]
        if ptr and not any(lo <= ptr < hi for lo, hi, _ in state["regions"]):
            name = f"{base_name}[+{addr - base_name_lo(base_name):#x}]"
            state["regions"].append((ptr, ptr + MAX_CHILD_SPAN, name))

    def base_name_lo(name):
        for lo, hi, n in state["regions"]:
            if n == name:
                return lo
        return 0

    def on_read(uc_, access, addr, size, value, user):
        if not in_body():
            return
        name, off = classify(addr)
        if name in ("obj", "tabA", "tabB", "opt"):
            learn_pointers(addr, size, name)
        raw = bytes(uc.mem_read(addr, size))
        listing.add(name, off, size, "R", emu.reg(UC_X86_REG_RIP), raw.hex())

    def on_write(uc_, access, addr, size, value, user):
        if not in_body():
            return
        name, off = classify(addr)
        listing.add(name, off, size, "W", emu.reg(UC_X86_REG_RIP), f"{value & ((1 << (8 * size)) - 1):#x}")

    uc.hook_add(UC_HOOK_CODE, on_entry, begin=ENTRY, end=ENTRY)
    uc.hook_add(UC_HOOK_MEM_READ, on_read)
    uc.hook_add(UC_HOOK_MEM_WRITE, on_write)

    tin, _ = alloc_frames(emu, 2, N, [[math.sin(0.05 * i) * 0.5 for i in range(N)]] * 2)
    tout, _ = alloc_frames(emu, 2, N, [[0.0] * N, [0.0] * N])
    for _ in range(blocks):
        emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, N, max_insns=400_000_000)
    listing.dump(out_path)
    print(f"{len(listing.entries)} body entries, {len(listing.rows)} access rows -> {out_path}")


if __name__ == "__main__":
    main()
