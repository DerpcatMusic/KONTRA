"""Native vectors for Kolor shaper cases 0, 1 and 3 (dispatch `1419749f0`, selector byte shaper+0x6f0), from Kontakt8.exe under kemu.

    python3 -I tools/re/kolor_shaper_0_1_3_vectors.py KONTAKT8_EXE OUT_DIR

Drives the real per-sample entry: the dispatch the wrapper `14197a2c0` calls, with x through `ctx+0x10`
(`*ctx[2]`) and the shaper block in rdx, y returned in xmm0, state written back to shaper[0..8].

Writes:
  kolor_shaper_0_sequence.json    selector 0 (`14194cc70` + `14194c830` inline): warmed runs, 12 samples,
                                  steady and moving P/P2. Each run: `init` (shaper block, 0x700 bytes), `steps`
                                  with `writes`, `x`, `y`, `state` (shaper[0..8] after the step).
  kolor_shaper_1_3_sequence.json  selector 1 and selector 3 (same layout, `sel` per case).

Moving steps rewrite P2 (shaper+0xb0 floats, 0x108 floats) at 3, 6, 9 and P (shaper+0x0c, 161 floats) at 6, 9,
as in `kolor_shaper_6_9_vectors.sequence_case`.
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import Emu  # noqa: E402
import kolor_shaper_6_9_vectors as k69  # noqa: E402

DISPATCH = 0x1419749F0
SEL_OFF = 0x6F0
BLOCK_BYTES = 0x700  # the dispatch reads the selector byte at +0x6f0
P2_AT, P_AT = 0xB0, 12  # float indices of P2 (shaper+0x2c0) and P (shaper+0x30)


def dispatch_step(e, sh, sel, x):
    """One sample through the dispatch with selector `sel`; returns (y, state after the call)."""
    e.write(sh + SEL_OFF, 'i', sel)
    xv, ctx = e.alloc(8), e.alloc(32)
    e.write(xv, 'f', x)
    e.write(ctx + 0x10, 'Q', xv)
    e.call(DISPATCH, ctx, sh, limit=50_000_000)
    return e.retf(), list(e.read(sh, 'f', 8))


def sequence(e, rnd, sel, moving):
    """One warmed shaper with selector `sel`: init block, 12 samples, state carried through the dispatch."""
    init = [rnd.uniform(-0.5, 0.5) for _ in range(BLOCK_BYTES // 4)]
    init[SEL_OFF // 4] = 0  # overwritten by dispatch_step; the selector byte is set per call
    sh = e.alloc(BLOCK_BYTES)
    e.write(sh, 'f', *init)
    steps = []
    for k in range(12):
        writes = []
        if moving and k in (3, 6, 9):
            writes.append({'at': P2_AT, 'vals': [rnd.uniform(-0.5, 0.5) for _ in range(k69.GRU_FLOATS)]})
        if moving and k in (6, 9):
            writes.append({'at': P_AT, 'vals': [rnd.uniform(-0.6, 0.6) for _ in range(k69.P_FLOATS)]})
        for w in writes:
            e.write(sh + 4 * w['at'], 'f', *w['vals'])
        x = rnd.uniform(-1.0, 1.0)
        y, state = dispatch_step(e, sh, sel, x)
        steps.append({'writes': writes, 'x': x, 'y': y, 'state': state})
    return {'sel': sel, 'init': init, 'steps': steps}


def main():
    exe, outdir = sys.argv[1], sys.argv[2]
    rnd = random.Random(0x0013)
    e = Emu(exe)

    case0 = [sequence(e, rnd, 0, moving=m) for m in (False, False, True, True)]
    with open(os.path.join(outdir, 'kolor_shaper_0_sequence.json'), 'w') as fh:
        json.dump({'stage': 'dispatch case 0', 'cases': case0}, fh)

    cases13 = [sequence(e, rnd, sel, moving=m) for sel in (1, 3) for m in (False, True) for _ in range(2)]
    with open(os.path.join(outdir, 'kolor_shaper_1_3_sequence.json'), 'w') as fh:
        json.dump({'stage': 'dispatch cases 1 and 3', 'cases': cases13}, fh)
    print('wrote', outdir)


if __name__ == '__main__':
    main()
