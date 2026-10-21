"""Block coverage of the gain body by the replay fixture (src/reference/gain/body.rs test).

Builds the static CFG of 0x8e04c0..0x8e53b1 (gain_cfg.py) and records which blocks run in the
body entries the fixture captures (gain_fx_body.CASES). A block that runs in a fixture entry is
checked by the replay test: its effect reaches a recorded callee input, output or state.
Prints totals and the never-executed blocks grouped into address ranges, with the reason
(path) where known.

Run with: python -I gain_coverage.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from unicorn import UC_HOOK_CODE  # noqa: E402

import gain_cfg  # noqa: E402
from gain_fx_body import CASES, reshape  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RCX  # noqa: E402
from gain_setup import BODY_LO, extra_params, make_session, run_block  # noqa: E402
from pe_emu import IMAGE_BASE, Emulator  # noqa: E402


def main():
    blocks = gain_cfg.build(gain_cfg.sweep(Emulator()))
    starts = set(blocks)
    seen = set()
    for case, params, extra, entries in CASES:
        sess = make_session(params + extra_params(extra))
        st = {"n": -1}

        def on_entry(uc, a, s, u, st=st, sess=sess, case=case, entries=entries):
            st["n"] += 1
            if st["n"] in entries:
                reshape(sess.emu, case, sess.emu.reg(UC_X86_REG_RCX), st["n"])

        def on_code(uc, a, s, u, st=st, entries=entries):
            r = a - IMAGE_BASE
            if r in starts and st["n"] in entries:
                seen.add(r)

        sess.emu.uc.hook_add(UC_HOOK_CODE, on_entry, begin=BODY_LO, end=BODY_LO)
        sess.emu.uc.hook_add(UC_HOOK_CODE, on_code, begin=IMAGE_BASE + gain_cfg.LO, end=IMAGE_BASE + gain_cfg.HI - 1)
        for b in range(max(entries) // 4 + 1):
            run_block(sess, block=b)
    total = len(starts)
    print(f"blocks {total}, executed in fixture entries {len(seen)} ({100 * len(seen) / total:.1f}%)")
    miss = sorted(starts - seen)
    ranges = []
    for s in miss:
        if ranges and blocks[ranges[-1][1]]["end"] >= s - 0x10:
            ranges[-1][1] = s
        else:
            ranges.append([s, s])
    for a, b in ranges:
        n = sum(1 for s in miss if a <= s <= b)
        print(f"  not run: {a:#x}..{blocks[b]['end']:#x} ({n} blocks)")


if __name__ == "__main__":
    main()
