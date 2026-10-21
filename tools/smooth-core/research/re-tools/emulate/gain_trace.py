"""Semantic trace of one call of a function (default: the gain body 0x8e04c0): blocks, calls, IPP ops.

IPP dispatch thunks were identified by black-box emulation (selector 0, see section 21 of the
notes); each is decoded with its argument registers and the head of its input/output vectors.
Plugin callees are listed with their register arguments. Block entries are printed in order,
with repeats of a loop folded into a count.

Run with: python -I gain_trace.py [entry_index] [blocks] [lo_rva hi_rva]
(GAIN_EXTRA="id=value,..." adds discrete parameter overrides, e.g. GAIN_EXTRA=2=1 for quality 1.)
"""

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
from capstone import CS_ARCH_X86, CS_MODE_64, Cs  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_R8, UC_X86_REG_R9, UC_X86_REG_RAX, UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_RSP,
)

from gain_setup import PARAMS, extra_params, make_session, run_block, xmm_f32  # noqa: E402
from pe_emu import IMAGE_BASE  # noqa: E402

R = {"rcx": UC_X86_REG_RCX, "rdx": UC_X86_REG_RDX, "r8": UC_X86_REG_R8, "r9": UC_X86_REG_R9}

# name, inputs [(reg, len_reg)], outputs [(reg, len_reg)], scalars
IPP = {
    0x10D0: ("Zero", [], [("rcx", "rdx")], []),
    0x10A0: ("Copy", [("rcx", "r8")], [("rdx", "r8")], []),
    0x1100: ("Threshold_I", [("rcx", "rdx")], [("rcx", "rdx")], ["x2", "r9"]),
    0x1250: ("Threshold", [("rcx", "r8")], [("rdx", "r8")], ["x3", "s20"]),
    0x1160: ("SubC_I", [("rdx", "r8")], [("rdx", "r8")], ["x0"]),
    0x1220: ("MulC_I", [("rdx", "r8")], [("rdx", "r8")], ["x0"]),
    0x12E0: ("AddC_I", [("rdx", "r8")], [("rdx", "r8")], ["x0"]),
    0x1190: ("Mul", [("rcx", "r9"), ("rdx", "r9")], [("r8", "r9")], []),
    0x1340: ("Mul_I", [("rcx", "r8"), ("rdx", "r8")], [("rdx", "r8")], []),
    0x13A0: ("Sub_I(dst-=src)", [("rcx", "r8"), ("rdx", "r8")], [("rdx", "r8")], []),
    0x1430: ("Div_I(dst/=src)", [("rcx", "r8"), ("rdx", "r8")], [("rdx", "r8")], []),
    0x1550: ("Div(dst=s2/s1)", [("rcx", "r9"), ("rdx", "r9")], [("r8", "r9")], []),
    0x11C0: ("Sub(dst=s2-s1)", [("rcx", "r9"), ("rdx", "r9")], [("r8", "r9")], []),
    0x1580: ("AddProduct(dst+=s1*s2)", [("rcx", "r9"), ("rdx", "r9"), ("r8", "r9")], [("r8", "r9")], []),
    0x1400: ("DotProd", [("rcx", "r8"), ("rdx", "r8")], [("r9", 1)], []),
    0x11F0: ("AddProductC(dst+=src*v)", [("rcx", "r9"), ("r8", "r9")], [("r8", "r9")], ["x1"]),
    0x1C8500: ("Ln", [("rcx", "r8")], [("rdx", "r8")], []),
    0x1C84A0: ("Exp", [("rcx", "r8")], [("rdx", "r8")], []),
    0x1010: ("FFTFwdR", [("rcx", 512)], [("rdx", 514)], []),
    0x6181B0: ("Magnitude", [("rdx", 514)], [("rcx", "r8")], []),
    0x17BB8E2: ("memset", [], [], []),
    0x1CBDF7B: ("logf", [], [], ["x0"]),
    0x1CBDF57: ("expf", [], [], ["x0"]),
    0x1CBDF81: ("powf", [], [], ["x0", "x1"]),
    0x1CBDB35: ("pow", [], [], []),
}


def head(emu, ptr, n, k=4):
    if not ptr or n <= 0 or n > 1 << 20:
        return "-"
    v = np.frombuffer(emu.rd(ptr, 4 * n), np.float32)
    return f"[{', '.join(f'{x:.6g}' for x in v[:k])}{', ...' if n > k else ''}] n={n} sum={float(np.sum(v.astype(np.float64))):.6g}"


def main():
    want = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    BODY_LO, BODY_HI = (IMAGE_BASE + int(a, 16) for a in (sys.argv[3:5] if len(sys.argv) > 4 else ('0x8e04c0', '0x8e53b1')))
    blocks = int(sys.argv[2]) if len(sys.argv) > 2 else 1
    sess = make_session(PARAMS + extra_params())
    emu, uc = sess.emu, sess.emu.uc
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    calls = {}
    leaders = set()
    for ins in md.disasm(emu.rd(BODY_LO, BODY_HI - BODY_LO), BODY_LO):
        if ins.mnemonic == "call":
            try:
                calls[ins.address] = (ins.size, int(ins.op_str, 16) - IMAGE_BASE)
            except ValueError:
                calls[ins.address] = (ins.size, None)
        if ins.mnemonic.startswith("j"):
            leaders.add(ins.address + ins.size)
            try:
                leaders.add(int(ins.op_str, 16))
            except ValueError:
                pass
    st = {"n": -1, "on": False, "pending": {}, "last_blk": None, "rep": 0, "rsp0": 0}
    out = []

    def regv(name, site_rsp):
        if name.startswith("x"):
            return xmm_f32(emu, int(name[1:]))
        if name == "s20":
            return emu.r32(site_rsp + 0x20)
        return emu.reg(R[name])

    def lenv(spec):
        return spec if isinstance(spec, int) else emu.reg(R[spec]) & 0xFFFFFFFF

    def flush_blk():
        if st["last_blk"] is not None:
            out.append(f"B {st['last_blk'] - IMAGE_BASE:#x}" + (f" x{st['rep'] + 1}" if st["rep"] else ""))
        st["last_blk"] = None
        st["rep"] = 0

    def on_code(uc_, addr, size, user):
        if addr == BODY_LO:
            st["n"] += 1
            st["on"] = st["n"] == want
            st["rsp0"] = emu.reg(UC_X86_REG_RSP)
            if st["on"]:
                out.append(f"ENTRY rcx={emu.reg(UC_X86_REG_RCX):#x} rdx={emu.reg(UC_X86_REG_RDX):#x} "
                           f"r8={emu.reg(UC_X86_REG_R8):#x} r9={emu.reg(UC_X86_REG_R9):#x} rsp={st['rsp0']:#x}")
        if not st["on"]:
            return
        if addr in st["pending"]:
            site, tgt, outs = st["pending"].pop(addr)
            res = []
            for reg, ln in outs:
                res.append(head(emu, reg, ln))
            extra = ""
            if tgt in (0x1CBDF7B, 0x1CBDF57, 0x1CBDF81, 0x1CBDB35):
                extra = f" -> {xmm_f32(emu, 0):.8g}"
            if res or extra:
                out.append(f"   <- {' | '.join(res)}{extra}")
        if addr == BODY_LO or addr in leaders:
            if st["last_blk"] == addr:
                st["rep"] += 1
            else:
                flush_blk()
                st["last_blk"] = addr
        if addr in calls:
            flush_blk()
            size_, tgt = calls[addr]
            rsp = emu.reg(UC_X86_REG_RSP)
            regs = " ".join(f"{k}={emu.reg(v):#x}" for k, v in R.items())
            if tgt not in IPP:
                xm = " ".join(f"x{i}={xmm_f32(emu, i):.6g}" for i in range(4))
                stk = " ".join(f"{emu.r64(rsp + 0x20 + 8 * i):#x}" for i in range(5))
                out.append(f"C {addr - IMAGE_BASE:#x} fn_{(tgt or 0):x} {regs} {xm} stk[{stk}]")
                st["pending"][addr + size_] = (addr, tgt, [])

    def on_ipp(uc_, addr, size, user):
        if not st["on"]:
            return
        tgt = addr - IMAGE_BASE
        rsp = emu.reg(UC_X86_REG_RSP)
        ret = emu.r64(rsp)
        site = ret - IMAGE_BASE
        inner = "" if BODY_LO <= ret < BODY_HI else "  "
        name, ins_, outs_, sc = IPP[tgt]
        regs = " ".join(f"{k}={emu.reg(v):#x}" for k, v in R.items())
        scal = " ".join(f"{s}={regv(s, rsp + 8):.8g}" if s.startswith("x") else f"{s}={regv(s, rsp + 8)}" for s in sc)
        ins_s = " ; ".join(f"{r}:{head(emu, emu.reg(R[r]), lenv(ln))}" for r, ln in ins_)
        flush_blk()
        out.append(f"{inner}I ret={site:#x} {name} {regs} {scal} {ins_s}")
        outs = [(emu.reg(R[r]), lenv(ln)) for r, ln in outs_]

        def done(uc2, a2, s2, u2, h=[None]):
            pass
        st["ipp_pending"].append((ret, tgt, outs, inner))

    def on_any_ret(uc_, addr, size, user):
        pend = st["ipp_pending"]
        if pend and pend[-1][0] == addr:
            ret, tgt, outs, inner = pend.pop()
            res = [head(emu, p, n) for p, n in outs]
            extra = f" -> {xmm_f32(emu, 0):.8g}" if tgt in (0x1CBDF7B, 0x1CBDF57, 0x1CBDF81, 0x1CBDB35) else ""
            if res or extra:
                out.append(f"{inner}   <- {' | '.join(res)}{extra}")

    st["ipp_pending"] = []
    uc.hook_add(UC_HOOK_CODE, on_code, begin=BODY_LO, end=BODY_HI - 1)
    for t in IPP:
        uc.hook_add(UC_HOOK_CODE, on_ipp, begin=IMAGE_BASE + t, end=IMAGE_BASE + t)
    uc.hook_add(UC_HOOK_CODE, on_any_ret, begin=IMAGE_BASE + 0x5E0000, end=IMAGE_BASE + 0x900000)
    for b in range(blocks):
        run_block(sess, block=b)
    flush_blk()
    print("\n".join(out))


if __name__ == "__main__":
    main()
