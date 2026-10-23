"""Kolor notify B `1419421c0` under kemu: random P/X/D/P2 blocks in, native blocks and phase out.

Builds one Kolor per mode (kolor_kemu_probe.build), reads the six 0x40-byte block pointers of the notify
object `this = core + 0x480` (this[0]=P, [3]=D, [6]=X, [9]=S source, [0xc]=P2, [0xf]=Z), writes random
lanes into S, P, D, P2, Z plus the phase fields, runs B once and records every block and field after it.

    python3 -I tools/re/kolor_notify_b_probe.py KONTAKT8_EXE [OUT_JSON]

Output (default tools/re/fixtures/kolor_notify_b.json) is read by
`crates/sampler-core/src/dsp/color/kolor/notify.rs` (test `notify_b_matches_native`).
"""
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as kp  # noqa: E402

NOTIFY_B = 0x1419421C0
THIS_OFF = 0x480
PTRS = {'P': 0x00, 'D': 0x18, 'X': 0x30, 'S': 0x48, 'P2': 0x60, 'Z': 0x78}
CASES_PER_MODE = 16
LANES = 16


def f32(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]


def lane_pool(rnd, base):
    """Mix of special values and copies of `base` so both branch outcomes occur."""
    choice = rnd.random()
    if choice < 0.2:
        return base
    if choice < 0.3:
        return 0.0
    if choice < 0.35:
        return -0.0
    if choice < 0.4:
        return f32(rnd.choice([1e-40, -1e-40, 1.4e-45]))
    if choice < 0.5:
        return f32(base + rnd.choice([1.0, -1.0]) * 2.0 ** rnd.randint(-24, 0))
    return f32(rnd.uniform(-4.0, 4.0) * 10.0 ** rnd.randint(-6, 3))


def random_blocks(rnd):
    # Denormal P lanes (bit pattern < 2^24) are the only ones where the snap test can pass.
    p = [f32(rnd.choice([1.4e-45, 1e-40, 3e-42, -2.8e-45])) if rnd.random() < 0.25 else f32(rnd.uniform(-2.0, 2.0))
         for _ in range(LANES)]
    if rnd.random() < 0.15:  # silent block: every lane equal, so the activity sum is zero
        s = list(p)
    else:
        s = [lane_pool(rnd, p[i]) for i in range(LANES)]
    d = [f32(rnd.uniform(-1.0, 1.0)) for _ in range(LANES)]
    if rnd.random() < 0.5:  # tiny deltas, so the snap test sees both outcomes on denormal lanes
        d = [f32(rnd.choice([0.0, 1.4e-45, -1e-40])) for _ in range(LANES)]
    p2 = [f32(rnd.uniform(-2.0, 2.0)) for _ in range(LANES)]
    z = [f32(rnd.uniform(-1.0, 1.0)) for _ in range(LANES)]
    return {'P': p, 'S': s, 'D': d, 'P2': p2, 'Z': z}


def main():
    exe = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), 'fixtures', 'kolor_notify_b.json')
    rnd = random.Random(20261023)
    cases = []
    for mode in range(10):
        e = kp.make(exe)
        _, core = kp.build(e, mode=mode)
        this = core + THIS_OFF
        ptr = {k: e.read(this + off, 'Q') for k, off in PTRS.items()}
        for _ in range(CASES_PER_MODE):
            blocks = random_blocks(rnd)
            for name, vals in blocks.items():
                e.write(ptr[name], 'f', *vals)
            # Phase fields as the tick leaves them; B overwrites phase, +0x95 and +0x98.
            e.write(this + 0x90, 'i', 4)
            e.write(this + 0x95, 'B', 0)
            e.write(this + 0x9C, 'f', 0.75)
            before = {'mode': mode, **blocks, 'phase': 4, 'flag95': 0, 'f9c': 0.75, 'X': list(e.read(ptr['X'], 'f', LANES))}
            # X is the copy target of the S source; keep its pre-value as recorded.
            e.write(ptr['X'], 'f', *blocks['S'])
            e.write(this + 0x90, 'i', 4)
            kp.run(e, NOTIFY_B, this)
            after = {
                'P': list(e.read(ptr['P'], 'f', LANES)),
                'D': list(e.read(ptr['D'], 'f', LANES)),
                'X': list(e.read(ptr['X'], 'f', LANES)),
                'P2': list(e.read(ptr['P2'], 'f', LANES)),
                'Z': list(e.read(ptr['Z'], 'f', LANES)),
                'phase': e.read(this + 0x90, 'i'),
                'flag95': e.read(this + 0x95, 'B'),
                'f98': e.read(this + 0x98, 'f'),
            }
            cases.append({'before': before, 'after': after})
    with open(out, 'w') as fh:
        json.dump({'lanes': LANES, 'cases': cases}, fh)
    print(f'{len(cases)} cases -> {out}')


if __name__ == '__main__':
    main()
