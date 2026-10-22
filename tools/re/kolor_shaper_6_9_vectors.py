"""Native vectors for Kolor shaper cases 6-9 (the cfb0 / cc70 stage), from Kontakt8.exe under kemu.

    python3 -I tools/re/kolor_shaper_6_9_vectors.py KONTAKT8_EXE OUT_DIR

Writes:
  kolor_shaper_6_9_cc70.json   cc70 stage vectors: random parameter block p (161 floats, P = shaper+0x30)
                               and V = [x, s0..s7]; expected scalar y from the emulator.
  kolor_shaper_6_9_cfb0.json   cfb0 vectors: random shaper block (176 floats), x, expected returned
                               sample out[0]. cfb0 reads S = shaper[0..8] and P = shaper+0x30.
  kolor_shaper_{6..9}_wrapper.json  the real wrapper drive (kolor_kemu_probe) for one mode each.

Status: cc70 and cfb0's returned sample are verified against the emulator. The state update
(shaper[0..8] after cfb0, computed by c830) is NOT modelled here; see docs/re/engine/kolor-shaper-cases.md.
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as probe  # noqa: E402
from kemu import Emu  # noqa: E402

CFB0, CC70 = 0x14194CFB0, 0x14194CC70
P_FLOATS = 0x284 // 4  # 0x000..0x280 inclusive: M1, b1, M2, off, w, c
SHAPER_FLOATS = 0x2C0 // 4
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
