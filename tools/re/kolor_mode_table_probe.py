"""Kolor mode -> shaper selector -> dispatch case, observed under kemu.

For each mode 0..9: construct + apply + prepare (kolor_kemu_probe.build), then run one 32-sample block.
Reports the signed selector byte of each of the 20 shaper objects (n = mode*2 + k, byte at
object + 0x800 + n*0x700 + 0x6f0) and which dispatch case bodies actually executed during the block.

    python3 -I tools/re/kolor_mode_table_probe.py KONTAKT8_EXE

Selector base is the core object (obj + 0x1c0), as in 14197a2c0 (param_1 + 0x800 + n*0x700).

Case bodies (from docs/re/disasm/kolor-shaper-dispatch-1419749f0.txt): case 0 inline at 141974a2e,
case 1 at 141974b75, case 3 at 141974c02 (inline, hooked at their first instruction), case 2 = 14197adb0,
case 4 = 14197a150, case 5 = 14197ae80, cases 6..8 = 14194cfb0 (shared; the selector byte tells them apart),
case 9 = 1419748b0.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as kp  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

SHAPER_BASE, SHAPER_STRIDE, SEL_OFF = 0x800, 0x700, 0x6F0
CASE_ENTRY = {
    0x141974A2E: 'case0',
    0x141974B75: 'case1',
    0x141974C02: 'case3',
    0x14197ADB0: 'case2',
    0x14197AE80: 'case5',
    0x14197A150: 'case4',
    0x14194CFB0: 'case6-8',
    0x1419748B0: 'case9',
}


def main():
    exe = sys.argv[1]
    print('mode | selectors n=0..19 (signed byte at +0x6f0) | dispatch cases executed')
    for mode in range(10):
        e = kp.make(exe)
        obj, core = kp.build(e, mode=mode)
        hits = {}
        for addr, name in CASE_ENTRY.items():
            def cb(uc, a, size, _, name=name):
                hits[name] = hits.get(name, 0) + 1
            e.uc.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)
        left, right = kp.signal(32)
        kp.block(e, core, left, right)
        sels = [e.read(core + SHAPER_BASE + n * SHAPER_STRIDE + SEL_OFF, 'b') for n in range(20)]
        print(f'{mode} | {sels} | {hits}')


if __name__ == '__main__':
    main()
