"""Kolor shaper parameter blocks under the CRT static initialisers (kemu, no Kontakt run).

The shaper parameter blocks `P` (shaper+0x30, 161 floats: cc70 readout) and `P2` (shaper+0x2c0, 264 floats:
c830 GRU) are not written by the constructor or apply. The constructor copies them from template builders
(`141975310`, `141976210`, `141976510`, `141975610`, `141976810`) that read `.data` tables at 0x14a64xxxx.
Those tables are in the zero-filled tail of `.data`; the Windows CRT fills them at process start, from
float immediates in the 109 static-initialiser functions below (`1404a1250` .. `1404c1490`, each writes
`.data` from `.rdata` constants or from immediates). Without this step every shaper block reads zero.

    python3 -I tools/re/kolor_static_init.py params KONTAKT8_EXE OUT_JSON   (blocks per mode, fixture)
    python3 -I tools/re/kolor_static_init.py sweep KONTAKT8_EXE PRIMED_OUT  (drive sweep, primed)

`params` writes `{"modes": {mode: {"P": [161], "P2": [264]}}, "init_failures": n}` for the live shaper blocks
(shaper k = 2 * mode, the second channel k + 1 is identical). Findings and their evidence are in
docs/re/engine/kolor-shaper-cases.md.
"""
import json
import struct
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as kp  # noqa: E402

# `grep -o 'FUN_1404[ab]....'` of the decompile writers into 0x14a64xxxx, sorted, inside 1404a1000..1404c2000.
INIT_WRITERS = [int(a, 16) for a in """
0x1404a1250, 0x1404a15e0, 0x1404a17c0, 0x1404a1de0, 0x1404a2080, 0x1404a22f0, 0x1404a2750, 0x1404a2b60,
0x1404a3130, 0x1404a3390, 0x1404a3550, 0x1404a3700, 0x1404a3970, 0x1404a3a70, 0x1404a3ca0, 0x1404a3fc0,
0x1404a40b0, 0x1404a41f0, 0x1404a4580, 0x1404a45c0, 0x1404a4840, 0x1404a4930, 0x1404a49c0, 0x1404a4b50,
0x1404a4d10, 0x1404a5240, 0x1404a5770, 0x1404a5880, 0x1404a5980, 0x1404a5ae0, 0x1404a5c60, 0x1404a5f30,
0x1404a6020, 0x1404a6330, 0x1404a6640, 0x1404a6700, 0x1404a68a0, 0x1404a6a90, 0x1404a6ea0, 0x1404a7170,
0x1404a7270, 0x1404a7330, 0x1404a7420, 0x1404a7830, 0x1404a79f0, 0x1404a7b20, 0x1404a7e20, 0x1404a8110,
0x1404a8440, 0x1404a8740, 0x1404a8a30, 0x1404a8d60, 0x1404a9740, 0x1404a9ad0, 0x1404a9c00, 0x1404a9f30,
0x1404aa910, 0x1404aaca0, 0x1404aadd0, 0x1404ab110, 0x1404abaf0, 0x1404abe80, 0x1404abfb0, 0x1404ac2e0,
0x1404ac5e0, 0x1404ac760, 0x1404ac840, 0x1404ad220, 0x1404ad5b0, 0x1404ad6e0, 0x1404ada30, 0x1404add30,
0x1404adeb0, 0x1404adfa0, 0x1404ae740, 0x1404aea60, 0x1404aeba0, 0x1404aeed0, 0x1404af8b0, 0x1404afc40,
0x1404afd70, 0x1404b00a0, 0x1404b0a80, 0x1404b0e10, 0x1404b0f40, 0x1404b1270, 0x1404b1c50, 0x1404b1fe0,
0x1404b2110, 0x1404b2440, 0x1404b2e20, 0x1404b31b0, 0x1404b32e0, 0x1404b3610, 0x1404b3ff0, 0x1404b4380,
0x1404b44b0, 0x1404b47e0, 0x1404b51c0, 0x1404b5550, 0x1404b5680, 0x1404b59b0, 0x1404b6390, 0x1404b6720,
0x1404b6850, 0x1404b6c40, 0x1404c0ac0, 0x1404c1030, 0x1404c1490
""".replace('\n', ' ').split(', ') if a.strip()]

MODES = (0, 1, 3, 4, 6, 7, 8, 9)  # 2 and 5 have no parameter blocks (stateless)


def prime(exe):
    """A fresh emulator with the static initialisers run. Returns (emulator, failures)."""
    e = kp.make(exe)
    failures = 0
    for fn in INIT_WRITERS:
        try:
            e.call(fn, 0, limit=20_000_000)
        except Exception:  # atexit registration imports: the data writes precede them
            failures += 1
    return e, failures


def blocks(exe):
    e, failures = prime(exe)
    _, core = kp.build(e, mode=9)
    out = {'modes': {}, 'init_failures': failures}
    for mode in MODES:
        base = core + 0x800 + 2 * mode * 0x700
        vals = struct.unpack('<%df' % (0x700 // 4), bytes(e.uc.mem_read(base, 0x700)))
        out['modes'][mode] = {'P': list(vals[0x30 // 4:0x2B4 // 4]), 'P2': list(vals[0x2C0 // 4:0x6E0 // 4])}
    return out


def sweep(exe, out):
    """The drive sweep of `kolor_drive_sweep.py` with the static initialisers run for every case."""
    cases = []
    import kolor_drive_sweep as ds
    for mode in range(10):
        for drive in ds.DRIVES:
            for name, (left, right) in ds.inputs().items():
                e, _ = prime(exe)
                _, core = kp.build(e, mode=mode, knobs={3: drive, 7: ds.WET})
                l, r = kp.block(e, core, left, right)
                cases.append({'mode': mode, 'drive': drive, 'signal': name, 'L': l, 'R': r})
        print(f'mode {mode}: done', flush=True)
    with open(out, 'w') as fh:
        json.dump({'rate': kp.RATE, 'block': kp.SEGMENT, 'wet': ds.WET, 'cases': cases}, fh)
    print('wrote', out)


def main():
    if sys.argv[1] == 'params':
        with open(sys.argv[3], 'w') as fh:
            json.dump(blocks(sys.argv[2]), fh)
        print('wrote', sys.argv[3])
    elif sys.argv[1] == 'sweep':
        sweep(sys.argv[2], sys.argv[3])
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main()
