"""Kolor shaper case 4 under kemu: direct calls to the wrapper 14197a150 on a snapshot of a real shaper object.

Case 4 is the dispatch case `switch(shaper[0x6f0])` -> `14197a150(shaper, out, &x)`. The wrapper reads and writes
the 7-float recurrent state at shaper[0..0x1c] and returns the scratch pointer; the dispatch epilogue reads the
output as float `out[0]` (rdx = rsp+0x20 in the caller frame).

Tables are runtime-initialised: four static init functions fill the Kolor template data at 0x14a64b*-0x14a64c*
before the constructor copies it into the shaper. Without them the matrices are zero and the output is zero.

    python3 -I tools/re/kolor_shaper4_kemu.py KONTAKT8_EXE [OUT_JSON]

Module use: `e = load(exe)`, `snap = mode4_snapshot(e)` (the 0x800-byte shaper object as the wrapper sees it),
then `out, state = call(e, snap, state, x)`.
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as kp  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RCX, UC_X86_REG_R8, UC_X86_REG_RDX  # noqa: E402

WRAPPER = 0x14197A150
AFTER_WRAPPER = 0x14197A2A2  # after the state write-back, before the stack check
STATIC_INITS = (0x1404AC840, 0x1404ADFA0, 0x1404AE740, 0x1404AEBA0)  # template tables; see module doc
SHAPER_BYTES = 0x800
STATE_FLOATS = 7


def load(exe):
    """kemu with the Kolor template tables initialised (static init functions run once per process)."""
    e = kp.make(exe)
    for fn in STATIC_INITS:
        kp.run(e, fn)
    return e


def mode4_snapshot(e):
    """Construct, apply, prepare, and return the mode-4 shaper object (k=1) as the wrapper sees it after a block."""
    obj, core = kp.build(e, mode=4)
    shaper = core + 0x800 + 9 * 0x700  # param + 0x800 + (mode*2 + k)*0x700, mode 4, k 1
    return bytes(e.uc.mem_read(shaper, SHAPER_BYTES))


def call(e, snap, state, x):
    """One wrapper call on a fresh copy of `snap` with `state` (7 floats) and input `x`.

    Returns (scratch floats[8], new state floats[7]).
    """
    obj = e.alloc(SHAPER_BYTES)
    e.uc.mem_write(obj, snap)
    e.write(obj, 'f', *state)
    scratch = e.alloc(64)
    xp = e.alloc(8)
    e.write(xp, 'f', x)
    xpp = e.alloc(8)
    e.write(xpp, 'Q', xp)
    e.call(WRAPPER, obj, scratch, xpp, limit=50_000_000)
    return list(e.read(scratch, 'f', 8)), list(e.read(obj, 'f', STATE_FLOATS))


def record_block(e, core, left, right):
    """Run the real per-block driver and record every case-4 wrapper call (entry state, x, exit state, out[0]).

    Entry and exit are hooked on the wrapper; exit is the instruction after the state write-back, so the
    state read there is the one the next call sees. Calls alternate between the two shapers (k=0, k=1) per sample.
    """
    cur = {}
    rec = []

    def entry(uc, addr, size, _):
        rcx = uc.reg_read(UC_X86_REG_RCX)
        xp = struct.unpack('<Q', bytes(uc.mem_read(uc.reg_read(UC_X86_REG_R8), 8)))[0]
        cur.update(shaper=rcx, scratch=uc.reg_read(UC_X86_REG_RDX),
                   x=struct.unpack('<f', bytes(uc.mem_read(xp, 4)))[0],
                   before=list(struct.unpack('<7f', bytes(uc.mem_read(rcx, 28)))))

    def exit_(uc, addr, size, _):
        rec.append(dict(k=(cur['shaper'] - core - 0x800) // 0x700 - 8,
                        x=cur['x'], state_before=cur['before'],
                        state_after=list(struct.unpack('<7f', bytes(uc.mem_read(cur['shaper'], 28)))),
                        out0=struct.unpack('<f', bytes(uc.mem_read(cur['scratch'], 4)))[0]))

    e.uc.hook_add(UC_HOOK_CODE, entry, begin=WRAPPER, end=WRAPPER)
    e.uc.hook_add(UC_HOOK_CODE, exit_, begin=AFTER_WRAPPER, end=AFTER_WRAPPER)
    kp.block(e, core, left, right)
    return rec


def main():
    exe = sys.argv[1]
    out_dir = sys.argv[2] if len(sys.argv) > 2 else None
    e = load(exe)
    obj, core = kp.build(e, mode=4)
    left, right = kp.signal(256)
    block = record_block(e, core, left, right)
    print('block: %d wrapper calls, out0 max |%.3g|, final state' % (
        len(block), max(abs(r['out0']) for r in block)), [round(v, 4) for v in block[-1]['state_after']])
    e2 = load(exe)
    snap = mode4_snapshot(e2)
    state, sweep = [0.0] * STATE_FLOATS, []
    for x in [1e-6, 1e3, -1e3, 1e6, -1e6, 3e38, -3e38, 0.0, 0.0] + [0.5 * ((i * 0.37) % 2.0 - 1.0) * (1 + i % 5) for i in range(256)]:
        out, state = call(e2, snap, state, x)
        sweep.append({'x': x, 'out0': out[0]})
    print('sweep: max |out0|', max(abs(r['out0']) for r in sweep), 'final state', [round(v, 4) for v in state])
    if out_dir:
        meta = {'case': 4, 'mode': 4, 'source': 'Kontakt8.exe, kolor_kemu_probe build/block (static inits run)',
                'output_rule': 'out0 = float at the dispatch output slot after 14197a150 (rdx = scratch)'}
        json.dump(dict(meta, block=block), open(os.path.join(out_dir, 'kolor_shaper_4_block.json'), 'w'))
        json.dump(dict(meta, sweep=sweep), open(os.path.join(out_dir, 'kolor_shaper_4_sweep.json'), 'w'))


if __name__ == '__main__':
    main()
