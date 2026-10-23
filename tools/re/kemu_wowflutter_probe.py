"""Wow/Flutter owner-state probes (pass 7): which setup lets set-rate / prepare run under kemu.

    python3 tools/re/kemu_wowflutter_probe.py [EXE]

Each strategy replays the wrapper ctor 0x140a842b0 first, then tries one owner-side setup and
reports the first Stop. Nothing is written except the pointers named in the strategy.
Strategies (see docs/re/engine/k-wowflutter-kernel.md, pass 7):
  1 plain set-rate 0x140971930           -> read of kernel+0x2c8 (0x14196b8aa)
  2 + pointers at kernel+0x298/2c8/2e0/2f8 -> write through kernel+0x280 (0x14196b8ce)
  3 direct prepare 0x14099aac0, ring array at kernel+0x100 -> read at 0x141ee86ed
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kemu  # noqa: E402
from kemu import Emu  # noqa: E402

EXE = sys.argv[1] if len(sys.argv) > 1 else '/data/dev/workspace/re-binaries/Kontakt8.exe'
CTOR, SET_RATE, PREPARE = 0x140A842B0, 0x140971930, 0x14099AAC0
KERNEL = 0x1C0


def ctor():
    e = Emu(EXE)
    wrap = e.alloc(0x10000)
    cfg = e.alloc(16)
    e.write(cfg, 'i', 0)
    e.call(CTOR, wrap, cfg)
    return e, wrap + KERNEL


def stop_of(fn):
    try:
        fn()
        return 'ok'
    except kemu.Stop as s:
        return 'STOP ' + str(s)


if __name__ == '__main__':
    e, k = ctor()
    print('1 set-rate:', stop_of(lambda: e.call(SET_RATE, k - KERNEL, d={1: 48000.0})))

    e, k = ctor()
    for off in (0x298, 0x2C8, 0x2E0, 0x2F8):
        e.write(k + off, 'Q', e.alloc(64))
    print('2 set-rate + state pointers:', stop_of(lambda: e.call(SET_RATE, k - KERNEL, d={1: 48000.0})))

    e, k = ctor()
    e.write(k + 0x100, 'Q', e.alloc(0x100))
    print('3 prepare + ring array:', stop_of(lambda: e.call(PREPARE, k, 0, 32, f={1: 48000.0})))
