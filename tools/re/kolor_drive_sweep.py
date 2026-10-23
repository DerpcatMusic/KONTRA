"""Kolor native vectors per (mode, drive) for the drive-shaper port (kemu, no Kontakt run).

    python3 -I tools/re/kolor_drive_sweep.py KONTAKT8_EXE OUT_JSON

One warmed run per (mode, drive): the signal of `kolor_kemu_probe.signal` (256 frames) through the
native block driver with knob 3 (drive) set and knob 7 (wet) at its default 0.5. Signals: `sine` (the
probe's stereo pair), `impulse` (unit impulse at frame 0, left only), `noise` (seeded LCG, +-0.5).
Output: {"rate", "block", "wet", "cases": [{mode, drive, signal, L, R}]}. Consumed by the Rust test in
`dsp/color/kolor/chain.rs` (`drive_sweep_matches_native`).
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as kp  # noqa: E402

N = 256
DRIVES = (0.0, 0.25, 0.5, 0.75, 1.0)
WET = 0.5


def noise(n, seed=12345):
    x, out = seed, []
    for _ in range(n):
        x = (1103515245 * x + 12345) & 0x7FFFFFFF
        out.append(0.5 * (x / 0x7FFFFFFF * 2.0 - 1.0))
    return out


def inputs():
    sine_l, sine_r = kp.signal(N)
    return {
        'sine': (sine_l, sine_r),
        'impulse': ([1.0] + [0.0] * (N - 1), [0.0] * N),
        'noise': (noise(N), noise(N, seed=777)),
    }


def main():
    exe, out = sys.argv[1], sys.argv[2]
    cases = []
    for mode in range(10):
        for drive in DRIVES:
            for name, (left, right) in inputs().items():
                e = kp.make(exe)
                _, core = kp.build(e, mode=mode, knobs={3: drive, 7: WET})
                l, r = kp.block(e, core, left, right)
                cases.append({'mode': mode, 'drive': drive, 'signal': name, 'L': l, 'R': r})
            print(f'mode {mode} drive {drive}: done')
    with open(out, 'w') as fh:
        json.dump({'rate': kp.RATE, 'block': kp.SEGMENT, 'wet': WET, 'cases': cases}, fh)
    print('wrote', out)


if __name__ == '__main__':
    main()
