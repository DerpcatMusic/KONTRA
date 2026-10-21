"""Output locus of the gain body 0x8e04c0: every write made by the body or its callees.

One process block at 48 kHz, non-identity band state set through the real setter 0x894370
(`gain_setup.configure`). For each body entry it records:
  - every call executed from the body: site, resolved target, rcx/rdx/r8/r9, xmm0..2,
  - every memory write while the body or a callee is active, classified as
      frame   below the body's entry rsp (its own 0x4ae8 frame and the callees' frames)
      caller  at or above the entry rsp (the caller's frame: home slots, out-params)
      obj     inside the DSP object
      heap    anything else
    and attributed to the body pc (`body`) or to the call site whose callee made it.
The JSON summarises external (non-frame) writes as merged address ranges per site.

Run with: python -I gain_io_probe.py [out.json]
"""

import collections
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from capstone import CS_ARCH_X86, CS_MODE_64, Cs  # noqa: E402
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_R8, UC_X86_REG_R9, UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_RIP, UC_X86_REG_RSP,
)

from gain_setup import BODY_HI, BODY_LO, make_session, run_block, xmm_f32  # noqa: E402
from pe_emu import IMAGE_BASE  # noqa: E402

OBJ_SIZE = 0xC1A8


def body_calls(emu):
    """{va: (len, target_va or None)} for every call instruction in the body."""
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    code = emu.rd(BODY_LO, BODY_HI - BODY_LO)
    out = {}
    for ins in md.disasm(code, BODY_LO):
        if ins.mnemonic == "call":
            try:
                tgt = int(ins.op_str, 16)
            except ValueError:
                tgt = None
            out[ins.address] = (ins.size, tgt)
    return out


def merge(ranges):
    ranges.sort()
    out = []
    for lo, hi in ranges:
        if out and lo <= out[-1][1]:
            out[-1][1] = max(out[-1][1], hi)
        else:
            out.append([lo, hi])
    return out


def main():
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("gain_io.json")
    sess = make_session()
    emu, uc = sess.emu, sess.emu.uc
    calls = body_calls(emu)
    st = {"entry": None, "site": None, "entries": []}

    def on_code(uc_, addr, size, user):
        if addr == BODY_LO and st["site"] is None:
            rsp = emu.reg(UC_X86_REG_RSP)
            e = {"rsp": rsp, "rcx": emu.reg(UC_X86_REG_RCX), "rdx": emu.reg(UC_X86_REG_RDX),
                 "r8": emu.reg(UC_X86_REG_R8), "r9": emu.reg(UC_X86_REG_R9),
                 "calls": [], "writes": collections.defaultdict(list)}
            st["entries"].append(e)
            st["entry"] = e
        st["site"] = None
        if addr in calls and st["entry"] is not None:
            st["site"] = addr
            st["entry"]["calls"].append({
                "site": addr - IMAGE_BASE,
                "target": (calls[addr][1] or 0) - IMAGE_BASE,
                "rcx": emu.reg(UC_X86_REG_RCX), "rdx": emu.reg(UC_X86_REG_RDX),
                "r8": emu.reg(UC_X86_REG_R8), "r9": emu.reg(UC_X86_REG_R9),
                "xmm": [xmm_f32(emu, i) for i in range(3)],
            })
            st["pending"] = addr

    def on_write(uc_, access, addr, size, value, user):
        e = st["entry"]
        if e is None:
            return
        pc = emu.reg(UC_X86_REG_RIP)
        if BODY_LO <= pc < BODY_HI:
            who = "body"
        else:
            who = f"{st.get('pending', 0) - IMAGE_BASE:#x}"
        rsp0 = e["rsp"]
        if rsp0 - 0x20000 <= addr < rsp0:
            kind = "frame"
        elif rsp0 <= addr < rsp0 + 0x4000:
            kind = "caller"
        elif e["rcx"] <= addr < e["rcx"] + OBJ_SIZE:
            kind = "obj"
        else:
            kind = "heap"
        if kind != "frame":
            e["writes"][(who, kind)].append((addr, addr + size))

    def on_ret(uc_, addr, size, user):
        # body returns to the caller: close the entry
        if st["entry"] is not None and addr == st["entry"].get("ret"):
            st["entry"] = None

    uc.hook_add(UC_HOOK_CODE, on_code, begin=BODY_LO, end=BODY_HI - 1)
    uc.hook_add(UC_HOOK_MEM_WRITE, on_write)

    def mark_ret(uc_, addr, size, user):
        e = st["entry"]
        if e is not None and "ret" not in e:
            e["ret"] = emu.r64(e["rsp"])
            uc.hook_add(UC_HOOK_CODE, on_ret, begin=e["ret"], end=e["ret"])

    uc.hook_add(UC_HOOK_CODE, mark_ret, begin=BODY_LO, end=BODY_LO)
    run_block(sess)

    doc = []
    for e in st["entries"]:
        doc.append({
            "rsp": hex(e["rsp"]), "rcx": hex(e["rcx"]), "rdx": hex(e["rdx"]), "r8": hex(e["r8"]), "r9": hex(e["r9"]),
            "calls": [{**c, "site": hex(c["site"]), "target": hex(c["target"]),
                       "rcx": hex(c["rcx"]), "rdx": hex(c["rdx"]), "r8": hex(c["r8"]), "r9": hex(c["r9"])}
                      for c in e["calls"]],
            "external_writes": {f"{w}/{k}": [[hex(a), hex(b), hex(b - a)] for a, b in merge(r)]
                                for (w, k), r in e["writes"].items()},
        })
    out_path.write_text(json.dumps(doc, indent=1))
    for i, e in enumerate(doc):
        print(f"entry {i}: rcx={e['rcx']} rdx={e['rdx']} r8={e['r8']} r9={e['r9']} rsp={e['rsp']} calls={len(e['calls'])}")
        for k, r in e["external_writes"].items():
            print(f"  {k}: {r[:6]}{' ...' if len(r) > 6 else ''}")


if __name__ == "__main__":
    main()
