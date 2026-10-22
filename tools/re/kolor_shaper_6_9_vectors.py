"""Native vectors for Kolor shaper cases 6-9 (the cfb0 / cc70 stage), from Kontakt8.exe under kemu.

    python3 -I tools/re/kolor_shaper_6_9_vectors.py KONTAKT8_EXE OUT_DIR

Writes:
  kolor_shaper_6_9_cc70.json   cc70 stage vectors: random parameter block p (161 floats, P = shaper+0x30)
                               and V = [x, s0..s7]; expected scalar y from the emulator.
  kolor_shaper_6_9_cfb0.json   cfb0 vectors: random shaper block (176 floats), x, expected returned
                               sample out[0]. cfb0 reads S = shaper[0..8] and P = shaper+0x30.
  kolor_shaper_{6..9}_wrapper.json  the real wrapper drive (kolor_kemu_probe) for one mode each.
  kolor_shaper_6_9_tanh.json      141943490 (ptanh, 4-lane) and 141943870 (sigmoid, 8-lane) sweeps.
  kolor_shaper_6_9_c830.json      c830 state update alone: P2 (0x108 floats), x, s -> s'.
  kolor_shaper_6_9_sequence.json  warmed cfb0 runs, state carried across 12 samples; steady and
                                  moving parameters (blocks rewritten between samples).

Status: cc70, cfb0's sample, c830 and the multi-block state carry are verified against the emulator.
Case 9 (1419748b0) is not in here; see docs/re/engine/kolor-shaper-cases.md.
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as probe  # noqa: E402
from kemu import Emu  # noqa: E402

CFB0, CC70, C830 = 0x14194CFB0, 0x14194CC70, 0x14194C830
PTANH, SIG = 0x141943490, 0x141943870
P_FLOATS = 0x284 // 4  # 0x000..0x280 inclusive: M1, b1, M2, off, w, c
SHAPER_FLOATS = 0x2C0 // 4
GRU_FLOATS = 0x108  # c830 P2: x-gains, biases, 24x8 M, 24 bias (shaper+0x2c0 .. +0x6e0)
SHAPER_BLOCK_FLOATS = 0x6E0 // 4  # full shaper block the sequence runs use
MODES = (6, 7, 8, 9)


def cc70_vector(e, p, v9):
    """Run cc70 on parameter block p (list of floats) and V = v9; returns the scalar."""
    pa = e.alloc(4 * P_FLOATS)
    e.write(pa, 'f', *p)
    vb = e.alloc(64)
    e.write(vb, 'f', *v9)
    vpp = e.alloc(16)
    e.write(vpp, 'Q', vb)
    out = e.alloc(16)
    e.call(CC70, pa, out, vpp, limit=50_000_000)
    return e.read(out, 'f')


def cfb0_vector(e, shaper_vals, x):
    """Run cfb0 on a shaper block; returns (out[0], state after the call)."""
    sh = e.alloc(4 * SHAPER_FLOATS)
    e.write(sh, 'f', *shaper_vals)
    fp, xp, out = e.alloc(16), e.alloc(16), e.alloc(64)
    e.write(fp, 'f', x)
    e.write(xp, 'Q', fp)
    e.call(CFB0, sh, out, xp, limit=50_000_000)
    return e.read(out, 'f'), list(e.read(sh, 'f', 8))


def tanh_vectors(e, rnd):
    """ptanh (4 lanes, 141943490) and sigmoid (8 lanes, 141943870) over a log-spaced sweep."""
    cases = []
    xs = [rnd.uniform(-9, 9) * 10 ** rnd.uniform(-4, 0) for _ in range(64)] + [0.0, 0.0004, -0.0004, 8.0, -8.0]
    for i in range(0, len(xs) - 3, 4):
        quad = xs[i:i + 4]
        inp, out = e.alloc(16), e.alloc(16)
        e.write(inp, 'f', *quad)
        e.call(PTANH, 0, out, inp, limit=10_000_000)
        ptv = e.read(out, 'f', 4)
        inp8, out8 = e.alloc(32), e.alloc(32)
        e.write(inp8, 'f', *(quad + quad))
        e.call(SIG, out8, inp8, limit=10_000_000)
        sg = e.read(out8, 'f', 8)
        for k, x in enumerate(quad):
            cases.append({'x': x, 'tanh': ptv[k], 'sigmoid': sg[k]})
    return cases


def c830_vector(e, p2, x, s):
    """c830 alone: P2 (0x108 floats), x, state s -> next state (8 floats)."""
    pb = e.alloc(4 * 0x200)
    e.write(pb, 'f', *p2)
    xv, xp = e.alloc(8), e.alloc(8)
    e.write(xv, 'f', x)
    e.write(xp, 'Q', xv)
    sv, spp = e.alloc(32), e.alloc(8)
    e.write(sv, 'f', *s)
    e.write(spp, 'Q', sv)
    out = e.alloc(32)
    e.call(C830, pb, out, xp, spp, limit=50_000_000)
    return list(e.read(out, 'f', 8))


def sequence_case(e, rnd, moving):
    """One warmed shaper: init block, 12 cfb0 samples; `moving` rewrites P/P2 at steps 3, 6, 9."""
    init = [rnd.uniform(-0.5, 0.5) for _ in range(SHAPER_BLOCK_FLOATS)]
    sh = e.alloc(4 * SHAPER_BLOCK_FLOATS)
    e.write(sh, 'f', *init)
    steps = []
    for k in range(12):
        writes = []
        if moving and k in (3, 6, 9):
            writes.append({'at': 0xB0, 'vals': [rnd.uniform(-0.5, 0.5) for _ in range(GRU_FLOATS)]})
        if moving and k in (6, 9):
            writes.append({'at': 12, 'vals': [rnd.uniform(-0.6, 0.6) for _ in range(P_FLOATS)]})
        for w in writes:
            e.write(sh + 4 * w['at'], 'f', *w['vals'])
        x = rnd.uniform(-1.0, 1.0)
        fp, xp, out = e.alloc(16), e.alloc(16), e.alloc(64)
        e.write(fp, 'f', x)
        e.write(xp, 'Q', fp)
        e.call(CFB0, sh, out, xp, limit=50_000_000)
        steps.append({'writes': writes, 'x': x, 'y': e.read(out, 'f'), 'state': list(e.read(sh, 'f', 8))})
    return {'init': init, 'steps': steps}


def main():
    exe, outdir = sys.argv[1], sys.argv[2]
    rnd = random.Random(0x6A9)
    e = Emu(exe)

    cc70_cases = []
    for _ in range(24):
        p = [rnd.uniform(-0.6, 0.6) for _ in range(P_FLOATS)]
        v9 = [rnd.uniform(-1.0, 1.0)] + [rnd.uniform(-0.5, 0.5) for _ in range(8)]
        cc70_cases.append({'p': p, 'v': v9, 'y': cc70_vector(e, p, v9)})
    with open(os.path.join(outdir, 'kolor_shaper_6_9_cc70.json'), 'w') as fh:
        json.dump({'stage': 'cc70', 'layout': 'M1 8x9 col-major @0x000, b1 @0x120, M2 8x8 col-major @0x140, '
                   'off @0x240, w @0x260, c @0x280; V = [x, s0..s7]', 'cases': cc70_cases}, fh)

    cfb0_cases = []
    for _ in range(12):
        shaper = [rnd.uniform(-0.5, 0.5) for _ in range(SHAPER_FLOATS)]
        x = rnd.uniform(-1.0, 1.0)
        y, _ = cfb0_vector(e, shaper, x)
        cfb0_cases.append({'shaper': shaper, 'x': x, 'y': y})
    with open(os.path.join(outdir, 'kolor_shaper_6_9_cfb0.json'), 'w') as fh:
        json.dump({'stage': 'cfb0_sample', 'note': 'returned out[0] only; state update not modelled',
                   'cases': cfb0_cases}, fh)

    rnd2 = random.Random(0x830)
    with open(os.path.join(outdir, 'kolor_shaper_6_9_tanh.json'), 'w') as fh:
        json.dump({'stage': 'ptanh/sigmoid', 'cases': tanh_vectors(e, rnd2)}, fh)

    c830_cases = []
    for _ in range(12):
        p2 = [rnd.uniform(-0.5, 0.5) for _ in range(GRU_FLOATS)]
        x = rnd.uniform(-1.0, 1.0)
        s = [rnd.uniform(-0.8, 0.8) for _ in range(8)]
        c830_cases.append({'p2': p2, 'x': x, 's': s, 's_next': c830_vector(e, p2, x, s)})
    with open(os.path.join(outdir, 'kolor_shaper_6_9_c830.json'), 'w') as fh:
        json.dump({'stage': 'c830', 'cases': c830_cases}, fh)

    seq = [sequence_case(e, rnd2, moving=m) for m in (False, False, True, True)]
    with open(os.path.join(outdir, 'kolor_shaper_6_9_sequence.json'), 'w') as fh:
        json.dump({'stage': 'cfb0 sequence', 'cases': seq}, fh)

    for mode in MODES:
        em = probe.make(exe)
        obj, core = probe.build(em, mode=mode)
        left, right = probe.signal(512)
        steady_l, steady_r = probe.block(em, core, left, right)
        with open(os.path.join(outdir, f'kolor_shaper_{mode}_wrapper.json'), 'w') as fh:
            json.dump({'rate': probe.RATE, 'block': probe.SEGMENT, 'mode': mode,
                       'input': 'signal(512) in tools/re/kolor_kemu_probe.py',
                       'steady_L': steady_l, 'steady_R': steady_r}, fh)
    print('wrote', outdir)


if __name__ == '__main__':
    main()
