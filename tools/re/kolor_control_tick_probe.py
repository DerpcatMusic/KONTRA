"""Kolor control tick 14197ab90 under kemu: random control-field states in, native state out.

Builds one Kolor (kolor_kemu_probe.build), then for each random case writes the control fields of the
core object (phase, countdown, flags, gains, mode, knob 4 raw float at +0x430), runs the tick once and
records the fields after it, plus which notify lambdas and the per-mode seed fired. The tick's
notify calls write the P/P2 blocks, which are not compared; only the control fields are.

    python3 -I tools/re/kolor_control_tick_probe.py KONTAKT8_EXE [OUT_JSON]

Output (default tools/re/fixtures/kolor_control_tick.json) is read by
`crates/sampler-core/src/dsp/color/kolor/control.rs` (test `tick_matches_native`).
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as kp  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

NOTIFY_A, NOTIFY_B, SEED = 0x14193FAB0, 0x1419421C0, 0x14197ACC0
# name, offset (core-relative), struct format
FIELDS = [
    ('phase', 0x510, 'i'),      # countdown phase (0, 1, 2, 3, 4 ...)
    ('flag514', 0x514, 'B'),    # one-shot: run notify B
    ('count518', 0x518, 'i'),   # countdown; reaching 0 runs notify A
    ('pending7f1', 0x7F1, 'B'),
    ('gain7e8', 0x7E8, 'f'),
    ('dirty7f4', 0x7F4, 'B'),   # mode changed, seed on next tick
    ('gain7e4', 0x7E4, 'f'),
    ('flag7f0', 0x7F0, 'B'),    # low byte of the word at +0x7f0; +0x7f1 is its high byte (pending)
    ('mode7dc', 0x7DC, 'i'),
    ('knob430', 0x430, 'f'),    # raw knob 4 (mode/9 in host terms) used by the seed
]
HOOKS = {'notify_a': NOTIFY_A, 'notify_b': NOTIFY_B, 'seed': SEED}
CASES = 400


def random_case(rnd):
    return {
        'phase': rnd.choice([0, 0, 1, 2, 3, 4, 5]),
        'flag514': rnd.choice([0, 0, 1]),
        'count518': rnd.choice([-1, 0, 1, 2, 3]),
        'pending7f1': rnd.choice([0, 1, 1]),
        'gain7e8': rnd.choice([0.0, 0.0, 1.0, 0.5]),
        'dirty7f4': rnd.choice([0, 1]),
        'gain7e4': rnd.choice([0.0, 1.0, 0.3, 0.9]),
        'flag7f0': rnd.choice([0, 1]),
        'mode7dc': rnd.randrange(10),
        'knob430': rnd.choice([0.0, 0.05, 0.1, 0.45, 0.5, 0.79, 0.9, 1.0, rnd.random()]),
    }


def write_fields(e, core, values):
    for name, off, fmt in FIELDS:
        e.write(core + off, fmt, values[name])


def read_fields(e, core):
    return {name: e.read(core + off, fmt) for name, off, fmt in FIELDS}


def main():
    exe = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), 'fixtures', 'kolor_control_tick.json')
    e = kp.make(exe)
    _, core = kp.build(e, mode=4)
    hits = {}
    for name, addr in HOOKS.items():
        def cb(uc, a, size, _, name=name):
            hits[name] = hits.get(name, 0) + 1
        e.uc.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)
    rnd = random.Random(20261023)
    cases = []
    for _ in range(CASES):
        before = random_case(rnd)
        write_fields(e, core, before)
        hits.clear()
        kp.run(e, kp.TICK, core)
        after = read_fields(e, core)
        cases.append({'before': before, 'after': after, 'fired': dict(hits)})
    with open(out, 'w') as fh:
        json.dump({'fields': [f[0] for f in FIELDS], 'cases': cases}, fh)
    print(f'{len(cases)} cases -> {out}')
    print('notify/seed fire counts:', {k: sum(c['fired'].get(k, 0) for c in cases) for k in HOOKS})


if __name__ == '__main__':
    main()
