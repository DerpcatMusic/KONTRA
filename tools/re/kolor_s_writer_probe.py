"""Who fills the Kolor shaper source block S (core+0x4c8) and the shaper parameter block (shaper+0x30).

    python3 -I tools/re/kolor_s_writer_probe.py KONTAKT8_EXE [MODE]

Hooks memory writes with kemu and prints the writing PC. Findings (2026-10-23, mode 2):
- S = this[9] is a 14-float knob mirror (index = knob id). The writes come from the wrapper setter
  path at PC 0x1409aab0a during apply (S+0x10 = mode/9, S+0x34 = knob 13 = 0.5, ...), not from a static table.
- Copy B 0x141979b10 copies S into the notify blocks (X = this[6], P = this[0], P2 = this[0xc]).
- The shaper parameter block (shaper+0x30, +0x2c0) gets no writes during apply, prepare or a block,
  and the static init 0x1404ac840 does not fill it either. Its source is still unknown.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kolor_kemu_probe as kp  # noqa: E402
from unicorn import UC_HOOK_MEM_WRITE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

COPY_B = 0x141979B10


def main():
    exe = sys.argv[1]
    mode = int(sys.argv[2]) if len(sys.argv) > 2 else 2
    e = kp.make(exe)
    holder = e.alloc(16)
    kp.run(e, kp.CTOR, holder)
    obj = e.read(holder, 'Q')
    core = obj + kp.CORE_OFF
    s_ptr = e.read(core + 0x4C8, 'Q')
    shaper = core + 0x800 + (mode * 2) * 0x700
    writes = {'S': {}, 'shaper': {}}

    def hook(uc, access, addr, size, value, _):
        pc = hex(uc.reg_read(UC_X86_REG_RIP))
        if s_ptr <= addr < s_ptr + 0x40:
            writes['S'][pc] = writes['S'].get(pc, 0) + 1
        if shaper <= addr < shaper + 0x6E0:
            writes['shaper'][pc] = writes['shaper'].get(pc, 0) + 1

    e.uc.hook_add(UC_HOOK_MEM_WRITE, hook, begin=s_ptr, end=s_ptr + 0x3F)
    e.uc.hook_add(UC_HOOK_MEM_WRITE, hook, begin=shaper, end=shaper + 0x6DF)
    kp.run(e, kp.APPLY, obj, kp.record(e, mode, {}))
    kp.run(e, kp.vslot(e, core, kp.PREPARE_SLOT), core, f={1: kp.RATE})
    kp.run(e, COPY_B, core)
    left, right = kp.signal(64)
    kp.block(e, core, left, right)
    print('S knobs:', [round(x, 4) for x in e.read(s_ptr, 'f', 14)])
    print('S writer PCs (apply..block):', writes['S'])
    print('shaper writer PCs:', writes['shaper'] or 'none')
    print('notify P (this[0]) after copy B:', [round(x, 4) for x in e.read(e.read(core + 0x480, 'Q'), 'f', 4)])


if __name__ == '__main__':
    main()
