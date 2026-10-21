"""Whole-memory diff across each call of the gain body 0x8e04c0 (its output locus).

Snapshots the heap, the arena (DSP object and host structs) and the live stack above the
body's entry rsp when the body is entered, and again at its return address; prints every
changed range. Writes below the entry rsp are the body's own frame and are not listed.

Run with: python -I gain_diff_probe.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RSP  # noqa: E402

from gain_setup import BODY_LO, make_session, run_block  # noqa: E402
from pe_emu import ARENA_BASE, HEAP_BASE, STACK_TOP  # noqa: E402


def snapshot(emu, rsp):
    return {
        "heap": (HEAP_BASE, np.frombuffer(emu.rd(HEAP_BASE, emu.heap_next - HEAP_BASE), np.uint8).copy()),
        "arena": (ARENA_BASE, np.frombuffer(emu.rd(ARENA_BASE, emu.arena_next - ARENA_BASE), np.uint8).copy()),
        "stack": (rsp, np.frombuffer(emu.rd(rsp, STACK_TOP - rsp), np.uint8).copy()),
    }


def diff(emu, before):
    out = []
    for name, (base, old) in before.items():
        new = np.frombuffer(emu.rd(base, len(old)), np.uint8)
        idx = np.flatnonzero(old != new)
        if not len(idx):
            continue
        # merge into ranges with gaps <= 16 bytes
        starts = [idx[0]]
        ends = []
        for a, b in zip(idx[:-1], idx[1:]):
            if b - a > 16:
                ends.append(a + 1)
                starts.append(b)
        ends.append(idx[-1] + 1)
        out += [(name, base + s, e - s) for s, e in zip(starts, ends)]
    return out


def main():
    sess = make_session()
    emu, uc = sess.emu, sess.emu.uc
    st = {"snap": None, "ret": None, "n": 0, "hook": None}

    def on_entry(uc_, addr, size, user):
        rsp = emu.reg(UC_X86_REG_RSP)
        st["snap"] = snapshot(emu, rsp + 8)
        st["ret"] = emu.r64(rsp)
        st["obj"] = emu.reg(0) if False else None
        if st["hook"] is None:
            st["hook"] = uc.hook_add(UC_HOOK_CODE, on_ret, begin=st["ret"], end=st["ret"])

    def on_ret(uc_, addr, size, user):
        if st["snap"] is None:
            return
        print(f"body call {st['n']}:")
        for name, a, n in diff(emu, st["snap"]):
            print(f"  {name} {a:#x} +{n:#x}")
        st["snap"] = None
        st["n"] += 1

    uc.hook_add(UC_HOOK_CODE, on_entry, begin=BODY_LO, end=BODY_LO)
    blocks = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    for b in range(blocks):
        run_block(sess, block=b)


if __name__ == "__main__":
    main()
