"""Differential fixture for the gain body's band-state callees (branch smooth/gain-band-state).

Covers these pieces of soothe3's gain body `0x8e04c0`:
  - `0x8ef8b0` -> `0x8f7630`  band analysis:  E[c][b] = dot(D[c][s_b..], W_b[s_b..])
  - `0x8f03f0` -> `0x8f78e0`  band synthesis: G[c][k] = sum_b L[c][b] * S_b[k]
  - `0x8ef9c0`                attack/release update of the band-energy state
  - `0x8f0210`                forward-backward one-pole smoother over the band gains
  - `0x89ad70`                maximum over channels (IPP max per channel)
  - attack coefficient        xmm2 at the call site `0x8e306e` (inline code `0x8e2eac..0x8e2f50`)

Cases come from two sources:
  1. in-situ: the emulated gain body runs process blocks in three configs (base, quality 1,
     mode 1). Each callee call is captured at entry and its outputs at the return address.
     The 4096-point analyzer (bins 2049) runs only with mode 1; `0x89ad70` only with quality 1.
  2. random: callees are called directly on the same session memory (in-situ filter tables,
     random views and inputs).

Filter tables are read from session memory. A band record is: `start` (i32 at +8b), `len`
(i32 at +0x80+8b), weight pointer (u64 at +0x100+8b), count (i32 at +0x190, or +0x180 when
that is zero). The binary reads only `W_b[start..start+len]`, so only that slice is stored.

Values are f32 bit patterns (u32). Evidence scope: isolated emulation with synthetic parameter
values and one synthetic 48 kHz input; random cases check the callees on arbitrary inputs.

Run with:  python -I gain_fx_band_state.py [out.json]
Default out: ../../../src/reference/fixtures/gain_band_state.json
"""

import json
import random
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import numpy as np  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_R13, UC_X86_REG_R8, UC_X86_REG_RCX, UC_X86_REG_RDX,
)

from gain_setup import BODY_LO, PARAMS, make_session, run_block, xmm_f32  # noqa: E402
from pe_emu import IMAGE_BASE, image_va  # noqa: E402

# Call sites (RVA). The return address of each call is the site plus its 5-byte length.
ANALYSIS_CALL, ANALYSIS_RET = 0x8E2DA1, 0x8E2DA6  # call 0x8ef8b0 (rcx = filterbank)
SYNTH_CALL, SYNTH_RET = 0x8E3F9E, 0x8E3FA3  # call 0x8f03f0 (rcx = filterbank)
ATTACK_CALL, ATTACK_RET = 0x8E306E, 0x8E3073  # call 0x8ef9c0 (xmm2 = attack, xmm3 = release)
SMOOTH_CALL, SMOOTH_RET = 0x8E3830, 0x8E3835  # call 0x8f0210
MAX_CALL, MAX_RET = 0x8E1650, 0x8E1655  # call 0x89ad70 (float result in xmm0)

ANALYSIS_FN, SYNTH_FN = image_va(0x8EF8B0), image_va(0x8F03F0)
ATTACK_FN, SMOOTH_FN, MAX_FN = image_va(0x8EF9C0), image_va(0x8F0210), image_va(0x89AD70)

CONFIGS = {
    "base": list(PARAMS),
    "quality1": list(PARAMS) + [(2, 1.0, 0.0, 2.0, 2.0, 0.0)],
    "mode1": [p for p in PARAMS if p[0] != 3] + [(3, 1.0, 0.0, 1.0, 1.0, 0.0)],
}
ARENA_BYTES = 1 << 20
STATE_STRIDE = 32  # analyzer+0xb38 channels are 32 floats apart; 16 are bands, 16 are padding
OUT_DEFAULT = HERE.parents[1] / "src/reference/fixtures/gain_band_state.json"
LABEL = {257: "512", 2049: "4096"}  # analyzer name by bin count (512-point and 4096-point)


def bits(arr):
    return [int(x) for x in np.asarray(arr, dtype=np.float32).view(np.uint32)]


def f32_bits(v):
    return struct.unpack("<I", struct.pack("<f", float(v)))[0]


class Mem:
    def __init__(self, emu):
        self.emu = emu

    def i32(self, a):
        return struct.unpack("<i", self.emu.rd(a, 4))[0]

    def u64(self, a):
        return struct.unpack("<Q", self.emu.rd(a, 8))[0]

    def floats(self, a, n):
        return np.frombuffer(self.emu.rd(a, 4 * n), np.float32).copy()


def read_view(mem, addr, stack_view):
    """Pointers and channel data of a view.

    Stack/arena views (0x410 bytes): channels i32 at +0x400, length at +0x404, pointers at +8c.
    Object views (0x98 bytes): channels i32 at +0x80, length at +0x88, pointers at +8c.
    """
    ch_off, ln_off = (0x400, 0x404) if stack_view else (0x80, 0x88)
    ch, ln = mem.i32(addr + ch_off), mem.i32(addr + ln_off)
    ptrs = [mem.u64(addr + 8 * c) for c in range(ch)]
    return ptrs, [mem.floats(p, ln) for p in ptrs]


def read_bands(mem, t):
    count = mem.i32(t + 0x190) or mem.i32(t + 0x180)
    bands = []
    for b in range(count):
        start, ln = mem.i32(t + 8 * b), mem.i32(t + 0x80 + 8 * b)
        wp = mem.u64(t + 0x100 + 8 * b)
        bands.append({"start": start, "len": ln, "weights": bits(mem.floats(wp + 4 * start, ln))})
    return bands


def make_view(emu, arrays, stack_view, bands=None):
    """Allocate channel buffers and a view over them. `arrays` are f32 arrays of one length.

    Object views (0x98) also carry the per-channel band count at +0x84 (the attack callee's loop
    length, read from `[rbp-0x4c]` = `[view+0x84]`) and the stride/length at +0x88.
    """
    v = emu.arena(0x410 if stack_view else 0x98)
    for c, arr in enumerate(arrays):
        p = emu.arena(4 * len(arr) + 64)
        emu.wr(p, np.asarray(arr, np.float32).tobytes())
        emu.w64(v + 8 * c, p)
    ch_off, ln_off = (0x400, 0x404) if stack_view else (0x80, 0x88)
    emu.w32(v + ch_off, len(arrays))
    emu.w32(v + ln_off, len(arrays[0]))
    if not stack_view:
        emu.w32(v + 0x84, bands)
    return v


def make_arena(emu):
    """Arena struct used as the callee's scratch: base pointer at +0, bump cursor (i32) at +0x18."""
    a = emu.arena(0x20)
    emu.w64(a, emu.alloc(ARENA_BYTES))
    emu.w32(a + 0x18, 0)
    return a


def capture(name, params, rate, blocks):
    """Run the gain body in-situ and record every callee call. Returns (session, cases, fbs)."""
    sess = make_session(params, rate=rate)
    sess, cases, fbs = capture_on(sess, name, rate, blocks)
    return sess, cases, fbs


def capture_on(sess, name, rate, blocks=1):
    emu, uc = sess.emu, sess.emu.uc
    mem = Mem(emu)
    cases = {"analysis": [], "synthesis": [], "attack": [], "smooth": [], "max": [], "coef": []}
    fbs = {}
    pending = []
    armed = {"on": True}

    def analyzer_label(fb):
        bins = mem.i32(fb - 0xF28 + 0x40)
        return LABEL.get(bins, str(bins))

    def on_analysis(uc_, addr, size, user):
        if armed["on"]:
            fb, e_v, d_v = emu.reg(UC_X86_REG_RCX), emu.reg(UC_X86_REG_RDX), emu.reg(UC_X86_REG_R8)
            label = analyzer_label(fb)
            fbs[label] = fb
            e_ptrs, _ = read_view(mem, e_v, True)
            _, d_arr = read_view(mem, d_v, True)
            pending.append(("analysis", label, e_ptrs, d_arr))

    def on_analysis_ret(uc_, addr, size, user):
        if armed["on"] and pending:
            _, label, e_ptrs, d_arr = pending.pop()
            e_out = [mem.floats(p, 16) for p in e_ptrs]
            cases["analysis"].append({"analyzer": label, "config": name, "rate": rate,
                                      "spectrum": [bits(x) for x in d_arr],
                                      "energies": [bits(x) for x in e_out]})

    def on_synth(uc_, addr, size, user):
        if armed["on"]:
            fb, g_v, l_v = emu.reg(UC_X86_REG_RCX), emu.reg(UC_X86_REG_RDX), emu.reg(UC_X86_REG_R8)
            label = analyzer_label(fb)
            fbs[label] = fb
            g_ptrs, _ = read_view(mem, g_v, True)
            _, l_arr = read_view(mem, l_v, True)
            pending.append(("synthesis", label, g_ptrs, l_arr, mem.i32(g_v + 0x404)))

    def on_synth_ret(uc_, addr, size, user):
        if armed["on"] and pending:
            _, label, g_ptrs, l_arr, bins = pending.pop()
            g_out = [mem.floats(p, bins) for p in g_ptrs]
            cases["synthesis"].append({"analyzer": label, "config": name, "rate": rate,
                                       "levels": [bits(x) for x in l_arr],
                                       "out": [bits(x) for x in g_out]})

    def on_attack(uc_, addr, size, user):
        if armed["on"]:
            # State channels are 32 floats apart (16 bands + 16 padding); read the whole stride.
            s_view = emu.reg(UC_X86_REG_RCX)
            s_ptrs = [mem.u64(s_view + 8 * c) for c in range(mem.i32(s_view + 0x80))]
            s_arr = [mem.floats(p, STATE_STRIDE) for p in s_ptrs]
            _, e_arr = read_view(mem, emu.reg(UC_X86_REG_RDX), True)
            assert mem.i32(s_view + 0x84) == len(e_arr[0]), "state band count differs from energies"
            a, b = xmm_f32(emu, 2), xmm_f32(emu, 3)
            an = emu.reg(UC_X86_REG_R13)
            rate_bits = f32_bits(struct.unpack("<f", emu.rd(an + 0x20, 4))[0])
            cases["coef"].append({"config": name, "rate": rate_bits, "hop": mem.u64(an + 0x60),
                                  "coef": f32_bits(a)})
            pending.append(("attack", s_ptrs, s_arr, e_arr, a, b))

    def on_attack_ret(uc_, addr, size, user):
        if armed["on"] and pending:
            _, s_ptrs, s_arr, e_arr, a, b = pending.pop()
            s_out = [mem.floats(p, STATE_STRIDE) for p in s_ptrs]
            cases["attack"].append({"config": name, "rate": rate,
                                    "attack": f32_bits(a), "release": f32_bits(b),
                                    "state_in": [bits(x) for x in s_arr],
                                    "energies": [bits(x) for x in e_arr],
                                    "state_out": [bits(x) for x in s_out]})

    def on_smooth(uc_, addr, size, user):
        if armed["on"]:
            ptrs, arr = read_view(mem, emu.reg(UC_X86_REG_RCX), True)
            pending.append(("smooth", ptrs, arr))

    def on_smooth_ret(uc_, addr, size, user):
        if armed["on"] and pending:
            _, ptrs, arr = pending.pop()
            outs = [mem.floats(p, len(x)) for p, x in zip(ptrs, arr)]
            cases["smooth"].append({"config": name, "input": [bits(x) for x in arr],
                                    "output": [bits(x) for x in outs]})

    def on_max(uc_, addr, size, user):
        if armed["on"]:
            _, arr = read_view(mem, emu.reg(UC_X86_REG_RCX), True)
            pending.append(("max", arr))

    def on_max_ret(uc_, addr, size, user):
        if armed["on"] and pending:
            _, arr = pending.pop()
            cases["max"].append({"config": name, "input": [bits(x) for x in arr],
                                 "output": f32_bits(xmm_f32(emu, 0))})

    for call_va, cb in [(ANALYSIS_CALL, on_analysis), (ANALYSIS_RET, on_analysis_ret),
                        (SYNTH_CALL, on_synth), (SYNTH_RET, on_synth_ret),
                        (ATTACK_CALL, on_attack), (ATTACK_RET, on_attack_ret),
                        (SMOOTH_CALL, on_smooth), (SMOOTH_RET, on_smooth_ret),
                        (MAX_CALL, on_max), (MAX_RET, on_max_ret)]:
        uc.hook_add(UC_HOOK_CODE, cb, begin=IMAGE_BASE + call_va, end=IMAGE_BASE + call_va)
    for blk in range(blocks):
        run_block(sess, block=blk)
    armed["on"] = False
    return sess, cases, fbs


def capture_poked(name, params, rate, hop):
    """Like `capture` for one block, with the 4096 analyzer's rate and hop overwritten at entry."""
    sess = make_session(params, rate=48000)
    emu, uc = sess.emu, sess.emu.uc
    mem = Mem(emu)

    def on_body(uc_, addr, size, user):
        an = emu.reg(UC_X86_REG_RCX)
        if mem.i32(an + 0x40) == 2049:
            emu.wr(an + 0x20, struct.pack("<f", rate))
            emu.w64(an + 0x60, hop)

    uc.hook_add(UC_HOOK_CODE, on_body, begin=BODY_LO, end=BODY_LO)
    cases = {"coef": [], "attack": []}
    # Reuse the capture hooks on the same session by running the one-block capture logic.
    _, c, _ = capture_on(sess, name, rate)
    cases["coef"] = c["coef"]
    cases["attack"] = c["attack"]
    return cases


def tables_of(mem, fb):
    return {"analysis": read_bands(mem, fb + 0x1A0), "synthesis": read_bands(mem, fb + 0x340)}


def random_cases(sess, fbs, rng):
    emu = sess.emu
    mem = Mem(emu)
    out = {"analysis": [], "synthesis": [], "attack": [], "smooth": [], "max": []}
    for key, fb in sorted(fbs.items()):
        bins = {"512": 257, "4096": 2049}[key]
        for _ in range(1 if bins > 1000 else 2):
            d = [rng.standard_normal(bins).astype(np.float32) ** 2 for _ in range(2)]
            d_view = make_view(emu, d, True)
            e_view = make_view(emu, [np.zeros(16, np.float32)] * 2, True)
            emu.call(ANALYSIS_FN, fb, e_view, d_view, make_arena(emu), max_insns=50_000_000)
            e_out = [mem.floats(p, 16) for p in read_view(mem, e_view, True)[0]]
            out["analysis"].append({"analyzer": key, "config": "random", "rate": 48000,
                                    "spectrum": [bits(x) for x in d],
                                    "energies": [bits(x) for x in e_out]})
            lv = [rng.uniform(0.0, 4.0, 16).astype(np.float32) for _ in range(2)]
            g_view = make_view(emu, [np.full(bins, 7.0, np.float32)] * 2, True)
            l_view = make_view(emu, lv, True)
            emu.call(SYNTH_FN, fb, g_view, l_view, max_insns=50_000_000)
            g_out = [mem.floats(p, bins) for p in read_view(mem, g_view, True)[0]]
            out["synthesis"].append({"analyzer": key, "config": "random", "rate": 48000,
                                     "levels": [bits(x) for x in lv],
                                     "out": [bits(x) for x in g_out]})
    for i in range(4):
        ch = 2 if i < 3 else 1
        s = [rng.uniform(0.0, 3.0, STATE_STRIDE).astype(np.float32) for _ in range(ch)]
        e = [rng.uniform(0.0, 3.0, 16).astype(np.float32) for _ in range(ch)]
        if i == 2:  # one band exactly equal, one band zero: the comparison edges
            s[0][3] = e[0][3]
            e[0][5] = 0.0
        a = np.float32(rng.uniform(0.01, 0.9))
        b = np.float32(rng.uniform(0.01, 0.9))
        s_view = make_view(emu, s, False, bands=16)
        e_view = make_view(emu, e, True)
        emu.call(ATTACK_FN, s_view, e_view, 0, 0, make_arena(emu), floats=(0.0, 0.0, float(a), float(b)),
                 max_insns=50_000_000)
        s_out = [mem.floats(p, STATE_STRIDE) for p in read_view(mem, s_view, False)[0]]
        out["attack"].append({"config": "random", "rate": 48000, "attack": f32_bits(a),
                              "release": f32_bits(b), "state_in": [bits(x) for x in s],
                              "energies": [bits(x) for x in e], "state_out": [bits(x) for x in s_out]})
    lengths = [1, 2, 3, 4, 5, 7, 8, 9, 16, 17, 33]
    for n in lengths:
        ch = 1 + (n % 3)
        arrs = [rng.uniform(-2.0, 2.0, n).astype(np.float32) for _ in range(ch)]
        view = make_view(emu, arrs, True)
        emu.call(SMOOTH_FN, view, max_insns=50_000_000)
        outs = [mem.floats(p, n) for p in read_view(mem, view, True)[0]]
        out["smooth"].append({"config": "random", "input": [bits(x) for x in arrs],
                              "output": [bits(x) for x in outs]})
    for n in [1, 2, 5, 16, 33]:
        ch = 1 + (n % 3)
        arrs = [rng.uniform(-5.0, 5.0, n).astype(np.float32) for _ in range(ch)]
        view = make_view(emu, arrs, True)
        emu.call(MAX_FN, view, max_insns=50_000_000)
        out["max"].append({"config": "random", "input": [bits(x) for x in arrs],
                           "output": f32_bits(xmm_f32(emu, 0))})
    return out


def main():
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT_DEFAULT
    rng = np.random.default_rng(20261021)
    cases = {k: [] for k in ("analysis", "synthesis", "attack", "smooth", "max", "coef")}
    tables = {}
    mode_sess, mode_fbs = None, {}
    for name in ("base", "quality1", "mode1"):
        blocks = 2 if name == "mode1" else 1
        sess, c, fbs = capture(name, CONFIGS[name], 48000, blocks)
        for k in cases:
            cases[k].extend(c.get(k, []))
        for key, fb in fbs.items():
            t = tables_of(Mem(sess.emu), fb)
            if key in tables and tables[key] != t:
                raise SystemExit(f"filter tables differ between configs for analyzer {key}")
            tables.setdefault(key, t)
        if name == "mode1":
            mode_sess, mode_fbs = sess, dict(fbs)
    # Attack coefficient sweep: the 4096 analyzer's rate (+0x20) and hop (+0x60) are poked at each
    # body entry, so the inline formula runs on several (rate, hop) pairs. The base 48 kHz/hop 512
    # pair is the unpoked run above. 96 kHz without poking never reaches the attack path.
    for rate, hop in [(44100.0, 512), (48000.0, 1024), (96000.0, 1024), (22050.0, 256),
                      (12345.6, 777), (48000.0, 333)]:
        c = capture_poked(f"mode1-poke-{rate}-{hop}", CONFIGS["mode1"], rate, hop)
        print(f"poke rate {rate} hop {hop}: attack-path samples {len(c['coef'])}", file=sys.stderr)
        cases["coef"].extend(c["coef"])
        cases["attack"].extend(c["attack"])
    rand = random_cases(mode_sess, mode_fbs, rng)
    for k in rand:
        cases[k].extend(rand[k])
    # Keep one in-situ call per (analyzer, config) for the filterbank cases: the size budget is 1 MB.
    for k in ("analysis", "synthesis"):
        seen, kept = set(), []
        for case in cases[k]:
            key = (case["analyzer"], case["config"])
            if case["config"] == "random" or key not in seen:
                seen.add(key)
                kept.append(case)
        cases[k] = kept
    doc = {
        "version": 1,
        "source": "soothe3 gain body 0x8e04c0 callees, isolated emulation (gain_fx_band_state.py)",
        "tables": tables,
        "cases": cases,
    }
    text = json.dumps(doc, separators=(",", ":"))
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(text)
    counts = {k: len(v) for k, v in cases.items()}
    print(f"wrote {out_path} ({len(text)} bytes) cases={counts}", file=sys.stderr)


if __name__ == "__main__":
    main()
