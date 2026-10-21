"""Differential fixture for the gain frame builders (soothe3 0x894a70, 0x89ba70, 0x8e67a0).

Drives the emulated callees and writes the observable results as f32/u32 bit patterns for
`tools/smooth-core/src/reference/gain/frame.rs`:

  cursor    0x894a70  view copy with the write position moved back by a signed length:
                      pos' = (pos + delta) & (cap - 1)          (cap = ring length)
  segments  0x89ba70  ring split: [pos, cap) then [0, rest) when pos + n > cap
  window    0x8e67a0  dst[k] = ring-segment[k] * window[k], per segment, with the window
                      broadcast from one channel and no-op for other channel mismatches
  xform     0x8e6da0  for each record of the vector at analyzer+0x10 (0x18-byte records, channel
                      index at +8, flag at +0x10): flag == 1 leaves the frame alone (the binary's
                      copy is a self-copy); otherwise the pair (ch, ch+1) becomes
                      (0.5*(a+b), 0.5*(a-b)) in place, a = frame[ch], b = frame[ch+1]

  arena     0x61b7e0  stack view with nch channels bump-allocated from the arena (0x61c370):
                      channel k starts at the cursor; cursor = roundup16(cursor + len) per channel
  subview   0x8ebbb0  ring view copy keeping channels [start, start + nch) and the position
  scale     0x8ec1d0  dst[k] = src[k] * window[k] (or window[0] for a mono window), no segments

Synthetic cases use random rings and random pos/len. In-situ cases are captured from the gain
body's own calls (base configuration and the sidechain configuration: setter ids 3, 14 and 16
set to 1), with the ring rebuilt as a canonical ring whose chronological samples equal the
captured ones (the callee only reads samples in chronological order, so this is exact).

Evidence scope: isolated emulation of the restored binary with synthetic host and the
stand-in parameters of gain_setup.py. Values are test values, not plug-in defaults.

Run with:  python -I gain_fx_frame.py <out.json>
Writes ../../../src/reference/fixtures/gain_frame.json when given that path.
"""

import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_R8, UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_RSP,
)

import gain_setup  # noqa: E402
from gain_setup import BODY_HI, BODY_LO, make_session, run_block  # noqa: E402
from pe_emu import image_va  # noqa: E402

CURSOR = image_va(0x894A70)
SEGMENTS = image_va(0x89BA70)
WINDOW = image_va(0x8E67A0)
XFORM = image_va(0x8E6DA0)
ARENA = image_va(0x61B7E0)
SUBVIEW = image_va(0x8EBBB0)
SCALE = image_va(0x8EC1D0)
SC_EXTRA = [(3, 1.0, 0.0, 1.0, 1.0, 0.0), (14, 1.0, 0.0, 1.0, 1.0, 0.0), (16, 1.0, 0.0, 1.0, 1.0, 0.0)]


def bits(arr):
    return [struct.unpack("<I", struct.pack("<f", float(v)))[0] for v in arr]


def f32s(vals):
    return struct.pack(f"<{len(vals)}f", *vals)


def ring_views(emu, nch, cap, pos, data):
    """Ring R (cap samples per channel) and ring view V (0x98) as the binary lays them out."""
    R = emu.arena(0x100)
    chans = []
    for k in range(nch):
        buf = emu.arena(4 * cap + 64)
        emu.wr(buf, f32s(data[k]))
        emu.w64(R + 8 * k, buf)
        chans.append(buf)
    emu.w32(R + 0x80, pos)
    emu.w32(R + 0x88, cap)
    emu.w32(R + 0x8C, nch)
    V = emu.arena(0x98)
    emu.w32(V, nch)
    emu.w64(V + 8, R)
    for k in range(nch):
        emu.w64(V + 0x10 + 8 * k, chans[k])
    emu.w32(V + 0x90, pos)
    return V


def frame_view_s(emu, chans, n):
    """Stack view (0x410): ptrs +8k, nch +0x400, len +0x404, bytes +0x408."""
    S = emu.arena(0x410)
    for k, c in enumerate(chans):
        emu.w64(S + 8 * k, c)
    emu.w32(S + 0x400, len(chans))
    emu.w32(S + 0x404, n)
    emu.w64(S + 0x408, 4 * n)
    return S


def window_view(emu, data, n):
    """Frame view (0x98): ptrs +8k, nch +0x80, len +0x84, bytes +0x88."""
    W = emu.arena(0x98)
    for k, row in enumerate(data):
        buf = emu.arena(4 * max(n, 1) + 64)
        emu.wr(buf, f32s(row))
        emu.w64(W + 8 * k, buf)
    emu.w32(W + 0x80, len(data))
    emu.w32(W + 0x84, n)
    emu.w64(W + 0x88, 4 * n)
    return W


def chrono_model(ring, cap, pos, n):
    """Chronological n samples starting at pos (the split of 0x89ba70, flattened)."""
    idx = [(pos + i) % cap for i in range(n)]
    return ring[idx] if n else ring[:0]


def canonical_geometry(rng, n):
    """Ring length (power of two, >= 2n) and a random start position for a canonical ring."""
    cap = 2
    while cap < 2 * max(n, 1):
        cap *= 2
    return cap, rng.randrange(cap)


def synth_cursor(emu, rng, cases):
    for _ in range(60):
        cap = rng.choice([2, 4, 16, 64, 1024, 65536])
        pos = rng.randrange(cap)
        delta = rng.randrange(-cap, cap + 1)
        R = emu.arena(0x100)
        emu.w32(R + 0x88, cap)
        V = emu.arena(0x98)
        emu.w64(V + 8, R)
        emu.w32(V + 0x90, pos)
        out = emu.arena(0x98)
        emu.call(CURSOR, V, out, delta & 0xFFFFFFFF)
        got = emu.r32(out + 0x90)
        cases["cursor"].append({"cap": cap, "pos": pos, "delta": delta, "pos_out": got})


def synth_segments(emu, rng, cases):
    for _ in range(100):
        cap = rng.choice([2, 4, 16, 32, 64, 1024])
        pos = rng.randrange(cap)
        n = rng.randrange(0, cap + 1)
        nch = rng.choice([1, 2])
        data = [[rng.uniform(-1, 1) for _ in range(cap)] for _ in range(nch)]
        V = ring_views(emu, nch, cap, pos, data)
        out = emu.arena(0x200)
        emu.call(SEGMENTS, V, out, n)
        seg1_ptr = emu.r64(out)
        seg2_ptr = emu.r64(out + 0x90)
        base = emu.r64(V + 0x10)
        len1 = emu.r32(out + 0x84)
        len2 = emu.r32(out + 0x114)
        start1 = (seg1_ptr - base) // 4
        start2 = (seg2_ptr - base) // 4 if len2 else 0
        cases["segments"].append({"cap": cap, "pos": pos, "n": n, "start1": start1, "len1": len1,
                                  "start2": start2, "len2": len2})


def synth_window(emu, rng, cases):
    for _ in range(60):
        cap = rng.choice([16, 32, 64])
        n = rng.randrange(0, cap + 1)
        pos = rng.randrange(cap)
        nch_r = rng.choice([1, 2])
        nch_w = rng.choice([1, 2])
        nch_d = rng.choice([1, 2])
        ring = [[rng.uniform(-1, 1) for _ in range(cap)] for _ in range(nch_r)]
        window = [[rng.uniform(-2, 2) for _ in range(n)] for _ in range(nch_w)]
        dst_in = [[rng.uniform(-5, 5) for _ in range(n)] for _ in range(nch_d)]
        cases["window"].append(run_window(emu, cap, pos, n, ring, window, dst_in, src="synthetic"))


def run_window(emu, cap, pos, n, ring, window, dst_in, src):
    V = ring_views(emu, len(ring), cap, pos, ring)
    W = window_view(emu, window, n)
    dchans = []
    for row in dst_in:
        buf = emu.arena(4 * max(n, 1) + 64)
        emu.wr(buf, f32s(row))
        dchans.append(buf)
    D = frame_view_s(emu, dchans, n)
    emu.call(WINDOW, D, V, W, max_insns=50_000_000)
    dst_out = [list(struct.unpack(f"<{n}f", emu.rd(c, 4 * n))) for c in dchans]
    return {"src": src, "cap": cap, "pos": pos, "n": n,
            "ring": [bits(r) for r in ring], "window": [bits(w) for w in window],
            "dst_in": [bits(r) for r in dst_in], "dst_out": [bits(r) for r in dst_out]}


def synth_arena(emu, rng, cases):
    for _ in range(40):
        nch = rng.choice([1, 2, 3])
        n = rng.randrange(0, 300)
        cursor = rng.randrange(0, 2000)
        arena = emu.arena(0x40)
        base = emu.arena(4 * 4096)
        emu.w64(arena, base)
        emu.w32(arena + 0x18, cursor)
        out = emu.arena(0x410)
        emu.call(ARENA, arena, out, nch, n)
        starts = [(emu.r64(out + 8 * k) - base) // 4 for k in range(nch)]
        cases["arena"].append({"cursor": cursor, "nch": nch, "n": n, "starts": starts,
                               "cursor_out": emu.r32(arena + 0x18), "nch_out": emu.r32(out + 0x400),
                               "len_out": emu.r32(out + 0x404)})


def synth_subview(emu, rng, cases):
    for _ in range(30):
        nch_src = rng.choice([2, 3, 4])
        cap = 64
        pos = rng.randrange(cap)
        nch_out = rng.randrange(1, nch_src + 1)
        start = rng.randrange(0, nch_src - nch_out + 1)
        data = [[rng.uniform(-1, 1) for _ in range(cap)] for _ in range(nch_src)]
        V = ring_views(emu, nch_src, cap, pos, data)
        out = emu.arena(0x98)
        emu.call(SUBVIEW, V, out, nch_out, start)
        chans = [(emu.r64(out + 0x10 + 8 * k) - emu.r64(V + 0x10 + 8 * (start + k))) for k in range(nch_out)]
        cases["subview"].append({"nch_src": nch_src, "pos": pos, "nch_out": nch_out, "start": start,
                                 "ptr_diff": chans, "nch_field": emu.r32(out), "pos_out": emu.r32(out + 0x90)})


def synth_scale(emu, rng, cases):
    for _ in range(30):
        nch = rng.choice([1, 2])
        wnch = rng.choice([1, 2])
        n = rng.randrange(0, 40)
        src = [[rng.uniform(-2, 2) for _ in range(n)] for _ in range(nch)]
        win = [[rng.uniform(-2, 2) for _ in range(n)] for _ in range(wnch)]
        dst_in = [[rng.uniform(-5, 5) for _ in range(n)] for _ in range(nch)]
        sch = [emu.arena(4 * max(n, 1) + 64) for _ in range(nch)]
        for k, row in enumerate(src):
            emu.wr(sch[k], f32s(row))
        S = frame_view_s(emu, sch, n)
        W = window_view(emu, win, n)
        dch = [emu.arena(4 * max(n, 1) + 64) for _ in range(nch)]
        for k, row in enumerate(dst_in):
            emu.wr(dch[k], f32s(row))
        D = frame_view_s(emu, dch, n)
        emu.call(SCALE, D, S, W)
        out = [list(struct.unpack(f"<{n}f", emu.rd(c, 4 * n))) for c in dch]
        cases["scale"].append({"n": n, "src": [bits(r) for r in src], "window": [bits(r) for r in win],
                               "dst_in": [bits(r) for r in dst_in], "dst_out": [bits(r) for r in out]})


def capture_insitu(sess, label, blocks, rng, cases, limit):
    """Record every call of 0x8e67a0 during `blocks` process blocks (the callee inputs at entry)."""
    emu = sess.emu
    uc = emu.uc
    seen = []

    def on_entry(uc_, addr, size, user):
        if len(seen) >= limit:
            return
        rcx = emu.reg(UC_X86_REG_RCX)
        rdx = emu.reg(UC_X86_REG_RDX)
        r8 = emu.reg(UC_X86_REG_R8)
        ret = emu.r64(emu.reg(UC_X86_REG_RSP))
        nch_d = emu.r32(rcx + 0x400)
        n = emu.r32(rcx + 0x404)
        dchans = [emu.r64(rcx + 8 * k) for k in range(nch_d)]
        dst_in = [list(struct.unpack(f"<{n}f", emu.rd(c, 4 * n))) for c in dchans]
        nch_r = emu.r32(rdx)
        R = emu.r64(rdx + 8)
        cap = emu.r32(R + 0x88)
        pos = emu.r32(rdx + 0x90)
        ring = []
        for k in range(nch_r):
            ch = emu.r64(rdx + 0x10 + 8 * k)
            ring.append(np.frombuffer(emu.rd(ch, 4 * cap), dtype=np.float32).copy())
        chrono = [chrono_model(r, cap, pos, n) for r in ring]
        nch_w = emu.r32(r8 + 0x80)
        nw = emu.r32(r8 + 0x84)
        window = []
        for k in range(nch_w):
            ch = emu.r64(r8 + 8 * k)
            window.append(list(struct.unpack(f"<{nw}f", emu.rd(ch, 4 * nw)))[:n])
        if n == 0 or nch_w == 0 or nch_r == 0:
            return
        cap_c, pos_c = canonical_geometry(rng, n)
        ring_c = []
        for k in range(nch_r):
            arr = np.array([rng.uniform(-3, 3) for _ in range(cap_c)], dtype=np.float32)
            for i in range(n):
                arr[(pos_c + i) % cap_c] = chrono[k][i]
            ring_c.append(arr)
        seen.append({"ring": ring_c, "cap": cap_c, "pos": pos_c, "n": n, "window": window,
                     "wrap": pos + n > cap,
                     "dst_in": dst_in, "ret": ret, "dchans": dchans})

    def on_ret_factory(entry):
        def on_ret(uc_, addr, size, user):
            dchans = entry["dchans"]
            n = entry["n"]
            out = [list(struct.unpack(f"<{n}f", emu.rd(c, 4 * n))) for c in dchans]
            cases.append({"src": label, "cap": entry["cap"], "pos": entry["pos"], "n": n, "wrap": entry["wrap"],
                                    "ring": [bits(r) for r in entry["ring"]],
                                    "window": [bits(w) for w in entry["window"]],
                                    "dst_in": [bits(r) for r in entry["dst_in"]],
                                    "dst_out": [bits(r) for r in out]})
            uc.hook_del(entry["hook"])
        return on_ret

    def on_call(uc_, addr, size, user):
        n_before = len(seen)
        on_entry(uc_, addr, size, user)
        if len(seen) > n_before:
            entry = seen[-1]
            h = uc.hook_add(UC_HOOK_CODE, on_ret_factory(entry), begin=entry["ret"], end=entry["ret"])
            entry["hook"] = h

    uc.hook_add(UC_HOOK_CODE, on_call, begin=WINDOW, end=WINDOW)
    for b in range(blocks):
        run_block(sess, block=b)
    return len(seen)


def record_vector(emu, records):
    """Vector object (begin, end) over 0x18-byte records: ch at +8, flag at +0x10."""
    recs = emu.arena(0x18 * max(len(records), 1))
    for i, (ch, flag) in enumerate(records):
        emu.w64(recs + 0x18 * i + 8, ch)
        emu.w64(recs + 0x18 * i + 0x10, flag)
    vec = emu.arena(0x18)
    emu.w64(vec, recs)
    emu.w64(vec + 8, recs + 0x18 * len(records))
    return vec


def scratch_arena(emu, n):
    """Arena (base @0, cursor dword @0x18) with room for the transform's scratch."""
    Z = emu.arena(0x40)
    base = emu.arena(4 * (4 * max(n, 1) + 128))
    emu.w64(Z, base)
    emu.w32(Z + 0x18, 0)
    return Z


def synth_xform(emu, rng, cases):
    for _ in range(40):
        nch = rng.choice([2, 3, 4])
        n = rng.randrange(1, 65)
        frame = [[rng.uniform(-2, 2) for _ in range(n)] for _ in range(nch)]
        # a pair (ch, ch + 1) must lie inside the frame
        recs = [(rng.randrange(0, nch - 1), rng.choice([0, 1, 2])) for _ in range(rng.randrange(0, 4))]
        cases["xform"].append(run_xform(emu, n, frame, recs, src="synthetic"))


def run_xform(emu, n, frame, recs, src):
    chans = []
    for row in frame:
        buf = emu.arena(4 * max(n, 1) + 64)
        emu.wr(buf, f32s(row))
        chans.append(buf)
    X = frame_view_s(emu, chans, n)
    Y = record_vector(emu, recs)
    Z = scratch_arena(emu, n)
    emu.call(XFORM, X, Y, Z, max_insns=50_000_000)
    out = [list(struct.unpack(f"<{n}f", emu.rd(c, 4 * n))) for c in chans]
    return {"src": src, "n": n, "records": [list(r) for r in recs],
            "frame_in": [bits(r) for r in frame], "frame_out": [bits(r) for r in out]}


def capture_xform(sess, label, blocks, cases, limit=10**6):
    """Record every call of 0x8e6da0 during `blocks` blocks: frame, records, outputs."""
    emu = sess.emu
    uc = emu.uc

    def on_call(uc_, addr, size, user):
        rcx = emu.reg(UC_X86_REG_RCX)
        rdx = emu.reg(UC_X86_REG_RDX)
        r8 = emu.reg(UC_X86_REG_R8)
        ret = emu.r64(emu.reg(UC_X86_REG_RSP))
        n = emu.r32(rcx + 0x404)
        nch = emu.r32(rcx + 0x400)
        chans = [emu.r64(rcx + 8 * k) for k in range(nch)]
        frame_in = [list(struct.unpack(f"<{n}f", emu.rd(c, 4 * n))) for c in chans]
        begin, end = emu.r64(rdx), emu.r64(rdx + 8)
        recs = [[emu.r64(p + 8), emu.r64(p + 0x10)] for p in range(begin, end, 0x18)]
        if len(cases_seen) >= limit:
            return

        def on_ret(uc2, addr2, size2, user2):
            out = [list(struct.unpack(f"<{n}f", emu.rd(c, 4 * n))) for c in chans]
            cases.append({"src": label, "n": n, "records": recs,
                          "frame_in": [bits(r) for r in frame_in], "frame_out": [bits(r) for r in out]})
            uc.hook_del(hk[0])

        hk = [uc.hook_add(UC_HOOK_CODE, on_ret, begin=ret, end=ret)]
        cases_seen.append(ret)

    cases_seen = []
    uc.hook_add(UC_HOOK_CODE, on_call, begin=XFORM, end=XFORM)
    for b in range(blocks):
        run_block(sess, block=b)
    return len(cases_seen)


def select_insitu(rows, per_group=4):
    """At most `per_group` in-situ calls per (source, n), keeping both wrapped and unwrapped ones."""
    groups = {}
    for c in rows:
        groups.setdefault((c["src"], c["n"]), []).append(c)
    picked = []
    for _, group in sorted(groups.items()):
        for wrap in (True, False):
            picked.extend([c for c in group if c["wrap"] == wrap][: per_group // 2])
    return picked


def main():
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    rng = random.Random(20261021)
    cases = {"cursor": [], "segments": [], "window": [], "xform": [], "arena": [], "subview": [], "scale": []}
    base = make_session()
    synth_cursor(base.emu, rng, cases)
    synth_segments(base.emu, rng, cases)
    synth_window(base.emu, rng, cases)
    synth_xform(base.emu, rng, cases)
    synth_arena(base.emu, rng, cases)
    synth_subview(base.emu, rng, cases)
    synth_scale(base.emu, rng, cases)
    insitu = []
    n_base = capture_insitu(base, "insitu-base", 2, rng, insitu, limit=10**6)
    sc = make_session(gain_setup.PARAMS + SC_EXTRA)
    n_sc = capture_insitu(sc, "insitu-sidechain", 2, rng, insitu, limit=10**6)
    print(f"in-situ window calls: base={n_base} sidechain={n_sc}", file=sys.stderr)
    x_insitu = []
    x_sc = capture_xform(sc, "insitu-sidechain", 2, x_insitu)
    print(f"in-situ xform calls (sidechain): {x_sc}", file=sys.stderr)
    cases["xform"].extend(x_insitu[:6])
    cases["window"].extend(select_insitu([c for c in insitu if c["n"] <= 2048]))
    print("cases: " + ", ".join(f"{k}={len(v)}" for k, v in cases.items()), file=sys.stderr)
    doc = {"note": "gain frame builders 0x894a70, 0x89ba70, 0x8e67a0; f32 as u32 bit patterns",
           "cases": cases}
    text = json.dumps(doc, separators=(",", ":"))
    if out_path:
        out_path.write_text(text)
        print(f"wrote {out_path} ({len(text)} bytes)", file=sys.stderr)
    else:
        print(text[:200])


if __name__ == "__main__":
    main()
