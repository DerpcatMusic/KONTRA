"""Replay fixture for the gain body 0x8e04c0 (src/reference/gain/body.rs).

For chosen body entries in several parameter configs this records:
- at entry: the analyzer scalars, the parameter block and flags, and every object view the
  body reads (weights, mask, curve, final multiplier, link multiplier, smoother states and
  coefficients, band state);
- the magnitude spectrum the front produces (frame building, IPP FFT, `0x6181b0` with IPP's
  rsqrtps-based sqrt tail, which QEMU does not round like hardware), per channel;
- each plug-in callee the body calls: the vectors it reads, at the call, and the vectors it
  writes, at its return;
- at return: the output view and the three state views;
- every libm call the body makes (`logf`/`expf`/`powf`/`pow`): code, arguments, result.

The Rust test runs the translated body with callees replayed from these records, checks
each callee's inputs against the recorded ones (a per-region check) and the outputs at the
end. Format: a flat binary of records `u32 name_len, name, u32 n, n x f32` (little endian),
names `case/entry/key`; integers are stored as f32 (all small).

Run with: python -I gain_fx_body.py  (writes src/reference/fixtures/gain_body.bin)
"""

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_R8, UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_RSP, UC_X86_REG_XMM0,
)

from gain_setup import BODY_HI, BODY_LO, PARAMS, extra_params, make_session, run_block, xmm_f32  # noqa: E402
from pe_emu import IMAGE_BASE  # noqa: E402

OUT = Path(__file__).resolve().parents[3] / "src/reference/fixtures/gain_body.bin"

OBJ_VIEWS = {
    "weights": 0x2C8, "mask": 0x358, "curve": 0x3E8, "final_mul": 0x478, "link_mul": 0x8F8,
    "spec_state": 0xA18, "spec_rise": 0xBC8, "spec_fall": 0xC58, "out_state": 0xAA8,
    "out_rise": 0xD78, "out_fall": 0xCE8, "band_state": 0xB38,
}
STATES = ("spec_state", "out_state", "band_state")

# call site -> (callee name, [(key, reg, kind)] read at the call, [(key, reg, kind)] read at return)
# kind: "arena" = 0x410 view, "obj" = 0x98 view, "bins" = raw pointer of `bins` floats.
SITES = {
    0x8E1650: ("flat_level", [("in", "rcx", "arena")], []),
    0x8E2314: ("bin_smooth", [("in", "r8", "bins")], [("out", "rdx", "bins")]),
    0x8E2378: ("bin_smooth", [("in", "r8", "bins")], [("out", "rdx", "bins")]),
    0x8E4D15: ("bin_smooth", [("in", "r8", "bins")], [("out", "rdx", "bins")]),
    0x8E5074: ("bin_smooth", [("in", "r8", "bins")], [("out", "rdx", "bins")]),
    0x8E264C: ("link", [("in", "rcx", "arena")], [("out", "rcx", "arena")]),
    0x8E46CC: ("link", [("in", "rcx", "arena")], [("out", "rcx", "arena")]),
    0x8E265E: ("link_multi", [("in", "rdx", "arena")], [("out", "rdx", "arena")]),
    0x8E46DE: ("link_multi", [("in", "rdx", "arena")], [("out", "rdx", "arena")]),
    0x8E2DA1: ("band_analysis", [("in", "r8", "arena")], [("out", "rdx", "arena")]),
    0x8E306E: ("band_state", [("in", "rdx", "arena")], [("out", "b38", "obj")]),
    0x8E3830: ("band_spread", [("in", "rcx", "arena")], [("out", "rcx", "arena")]),
    0x8E3F9E: ("band_synthesis", [("in", "r8", "arena")], [("out", "rdx", "arena")]),
    0x8E5141: ("out_smooth", [("in", "rcx", "arena")], [("out", "rcx", "arena")]),
    0x8E13C8: ("multi_pre", [], []),
    0x8E4BE8: ("multi_post", [], []),
}
# libm thunks: code, name; f32 ones take xmm0 (xmm1), pow takes doubles
LIBM = {0x1CBDF7B: (1, "logf"), 0x1CBDF57: (2, "expf"), 0x1CBDF81: (3, "powf"), 0x1CBDB35: (4, "pow")}
MAG_THUNK = IMAGE_BASE + 0x6181B0
MAG_RET = IMAGE_BASE + 0x8E13A8
REGS = {"rcx": UC_X86_REG_RCX, "rdx": UC_X86_REG_RDX, "r8": UC_X86_REG_R8}


def with_overrides(over):
    out = []
    for e in PARAMS:
        if e[0] in over:
            e = (e[0], over[e[0]], *e[2:])
        out.append(e)
    return out


CASES = [
    # name, params, extra (discrete), entries (4..7 short, 8 long after two blocks)
    ("base", PARAMS, "", [4, 8]),
    ("q1", with_overrides({0: 0.9, 5: 0.3}), "2=1", [4]),
    ("flag53", PARAMS, "15=1", [4]),
    ("flag52", with_overrides({0: 0.6, 5: 0.8}), "16=1", [4]),
    ("side", PARAMS, "14=1", [4]),
    # per-channel views: one-channel object views widened to two distinct channels at entry
    ("perchan", PARAMS, "", [4]),
    ("perchan_q1", with_overrides({0: 0.9, 5: 0.3}), "2=1", [4]),
    # broadcast: two-channel object views narrowed to their channel 0 at entry
    ("narrow", PARAMS, "", [4, 8]),
    ("narrow_q1", with_overrides({0: 0.9, 5: 0.3}), "2=1", [4]),
]
NARROW = ("link_mul", "final_mul", "band_state")
# views widened in the perchan cases (channel 1 = channel 0 scaled by 0.9..1.1 per bin)
WIDEN = ("weights", "mask", "curve", "final_mul", "link_mul", "spec_rise", "spec_fall", "out_rise", "out_fall")

def f32s(emu, ptr, n):
    return np.frombuffer(emu.rd(ptr, 4 * n), np.float32).copy()


def arena_view(emu, base):
    ch = emu.r32(base + 0x400)
    ln = emu.r32(base + 0x404)
    return [f32s(emu, emu.r64(base + 8 * c), ln) for c in range(ch)]


def obj_view(emu, base):
    ch = emu.r32(base + 0x80)
    ln = emu.r32(base + 0x88)
    return [f32s(emu, emu.r64(base + 8 * c), ln) for c in range(ch)]


def widen(emu, an, seed):
    """Make each one-channel view in WIDEN two-channel (channel 1 = channel 0 * U(0.9, 1.1))."""
    rng = np.random.default_rng(seed)
    for name in WIDEN:
        base = an + OBJ_VIEWS[name]
        if emu.r32(base + 0x80) != 1:
            continue
        ln = emu.r32(base + 0x88)
        v0 = f32s(emu, emu.r64(base), ln)
        v1 = (v0 * rng.uniform(0.9, 1.1, ln)).astype(np.float32)
        p = emu.arena(4 * ln + 64)
        p = (p + 15) // 16 * 16
        emu.wr(p, v1.tobytes())
        emu.w64(base + 8, p)
        emu.w32(base + 0x80, 2)


def narrow(emu, an):
    """Make each two-channel view in NARROW one-channel (keeps channel 0)."""
    for name in NARROW:
        base = an + OBJ_VIEWS[name]
        if emu.r32(base + 0x80) == 2:
            emu.w32(base + 0x80, 1)


def reshape(emu, case, an, seed):
    if case.startswith("perchan"):
        widen(emu, an, seed)
    elif case.startswith("narrow"):
        narrow(emu, an)


def capture(case, params, extra, entries, recs):
    sess = make_session(params + extra_params(extra))
    emu, uc = sess.emu, sess.emu.uc
    st = {"n": -1, "on": False, "calls": 0, "pending": {}, "fft": 0}

    def put(key, vecs):
        if isinstance(vecs, list):
            recs[f"{st['pfx']}/{key}/ch"] = np.array([len(vecs)], np.float32)
            for c, v in enumerate(vecs):
                recs[f"{st['pfx']}/{key}/{c}"] = v
        else:
            recs[f"{st['pfx']}/{key}"] = np.atleast_1d(np.asarray(vecs, np.float32))

    def read(spec, an):
        key, reg, kind = spec
        if kind == "obj":
            return key, obj_view(emu, an + 0xB38)
        ptr = emu.reg(REGS[reg])
        if kind == "arena":
            return key, arena_view(emu, ptr)
        return key, [f32s(emu, ptr, st["bins"])]

    def on_entry(uc_, addr, size, user):
        st["n"] += 1
        st["on"] = st["n"] in entries
        if not st["on"]:
            return
        rsp = emu.reg(UC_X86_REG_RSP)
        an = emu.reg(UC_X86_REG_RCX)
        st.update(an=an, out=emu.reg(UC_X86_REG_RDX), calls=0, fft=0, nlibm=0, pfx=f"{case}/{st['n']}")
        bins = emu.r64(an + 0x40)
        st["bins"] = bins
        cfg = emu.r64(an + 0x2C38)
        put("an/channels", emu.r64(an + 0x48))
        put("an/bins", bins)
        put("an/hop", emu.r64(an + 0x60))
        put("an/rate", struct.unpack("<f", emu.rd(an + 0x20, 4))[0])
        put("an/long", emu.rd(an + 0x58, 1)[0])
        p = lambda o: struct.unpack("<f", emu.rd(cfg + o, 4))[0]  # noqa: E731
        put("p/depth", p(0))
        put("p/focus", p(4))
        put("p/link", p(0x28))
        put("p/out_smooth", p(0x3C))
        put("p/max_cut_db", p(0x4C))
        put("p/flag52", emu.rd(cfg + 0x52, 1)[0])
        put("p/flag53", emu.rd(cfg + 0x53, 1)[0])
        put("p/quality1", emu.rd(rsp + 0x40, 1)[0])  # arg8 (stack slot 8)
        reshape(emu, case, an, st["n"])
        for name, off in OBJ_VIEWS.items():
            put(f"pre/{name}", obj_view(emu, an + off))
        ret = emu.r64(rsp)
        if ret not in st["pending"]:
            st["pending"][ret] = None
            uc.hook_add(UC_HOOK_CODE, on_ret, begin=ret, end=ret)

    def on_ret(uc_, addr, size, user):
        if not st["on"]:
            return
        put("post/out", arena_view(emu, st["out"]))
        for name in STATES:
            put(f"post/{name}", obj_view(emu, st["an"] + OBJ_VIEWS[name]))
        st["on"] = False

    def on_site(uc_, addr, size, user):
        if not st["on"]:
            return
        name, ins, outs = SITES[addr - IMAGE_BASE]
        k = st["calls"]
        st["calls"] += 1
        base = f"call/{k:03}"
        recs[f"{st['pfx']}/{base}/fn/{name}"] = np.zeros(1, np.float32)
        rcx = emu.reg(UC_X86_REG_RCX)
        if name == "bin_smooth":
            rec = rcx - st["an"]
            bank = 1 if rec >= 0x20B8 else 0
            c = (rec - (0x20B8 if bank else 0x15B8)) // 0xB0
            put(f"{base}/bank", bank)
            put(f"{base}/c", c)
            put(f"{base}/alias", int(emu.reg(UC_X86_REG_RDX) == emu.reg(UC_X86_REG_R8)))
        for spec in ins:
            key, v = read(spec, st["an"])
            put(f"{base}/{key}", v)
        ret = addr + 5
        st["site_outs"] = (base, outs, {s[1]: emu.reg(REGS[s[1]]) for s in outs if s[1] in REGS}, name)
        if ret not in st["pending"]:
            st["pending"][ret] = None
            uc.hook_add(UC_HOOK_CODE, on_site_ret, begin=ret, end=ret)

    def on_site_ret(uc_, addr, size, user):
        if not st["on"] or "site_outs" not in st:
            return
        base, outs, saved, name = st.pop("site_outs")
        if name == "flat_level":
            put(f"{base}/ret", xmm_f32(emu, 0))
        for key, reg, kind in outs:
            if kind == "obj":
                put(f"{base}/{key}", obj_view(emu, st["an"] + 0xB38))
            elif kind == "arena":
                put(f"{base}/{key}", arena_view(emu, saved[reg]))
            else:
                put(f"{base}/{key}", [f32s(emu, saved[reg], st["bins"])])

    def xmm_raw(i):
        raw = emu.uc.reg_read(UC_X86_REG_XMM0 + i)
        return raw if isinstance(raw, bytes) else int(raw).to_bytes(16, "little")

    def as_f32_words(raw, dbl):
        # f32: the low lane; f64: the two 32-bit halves, stored as f32 bit patterns
        return list(np.frombuffer(raw[:8 if dbl else 4], "<f4"))

    def on_libm(uc_, addr, size, user):
        if not st["on"]:
            return
        ret = emu.r64(emu.reg(UC_X86_REG_RSP))
        if not (BODY_LO <= ret < BODY_HI):
            return
        code, _ = LIBM[addr - IMAGE_BASE]
        dbl = code == 4
        st["libm"] = (code, dbl, as_f32_words(xmm_raw(0), dbl) + (as_f32_words(xmm_raw(1), dbl) if code >= 3 else []))
        if ret not in st["pending"]:
            st["pending"][ret] = None
            uc.hook_add(UC_HOOK_CODE, on_libm_ret, begin=ret, end=ret)

    def on_libm_ret(uc_, addr, size, user):
        if not st["on"] or "libm" not in st:
            return
        code, dbl, args = st.pop("libm")
        k = st.setdefault("nlibm", 0)
        st["nlibm"] = k + 1
        recs[f"{st['pfx']}/libm/{k:03}"] = np.array([code, *args, *as_f32_words(xmm_raw(0), dbl)], np.float32)

    def on_mag(uc_, addr, size, user):
        if not st["on"] or emu.r64(emu.reg(UC_X86_REG_RSP)) != MAG_RET:
            return
        st["mag_dst"] = emu.reg(UC_X86_REG_RCX)

    def on_mag_ret(uc_, addr, size, user):
        if not st["on"] or "mag_dst" not in st:
            return
        put(f"mag/{st['fft']}", f32s(emu, st.pop("mag_dst"), st["bins"]))
        st["fft"] += 1
        put("mag/ch", st["fft"])

    uc.hook_add(UC_HOOK_CODE, on_entry, begin=BODY_LO, end=BODY_LO)
    for s in SITES:
        uc.hook_add(UC_HOOK_CODE, on_site, begin=IMAGE_BASE + s, end=IMAGE_BASE + s)
    uc.hook_add(UC_HOOK_CODE, on_mag, begin=MAG_THUNK, end=MAG_THUNK)
    for t in LIBM:
        uc.hook_add(UC_HOOK_CODE, on_libm, begin=IMAGE_BASE + t, end=IMAGE_BASE + t)
    uc.hook_add(UC_HOOK_CODE, on_mag_ret, begin=MAG_RET, end=MAG_RET)
    for b in range(max(entries) // 4 + 1):
        run_block(sess, block=b)


def main():
    recs = {}
    for case, params, extra, entries in CASES:
        capture(case, params, extra, entries, recs)
        print(case, "done", len(recs))
    with open(OUT, "wb") as f:
        for name, v in recs.items():
            nb = name.encode()
            v = np.asarray(v, np.float32)
            f.write(struct.pack("<I", len(nb)) + nb + struct.pack("<I", v.size) + v.astype("<f4").tobytes())
    print(OUT, OUT.stat().st_size, "bytes,", len(recs), "records")


if __name__ == "__main__":
    main()
