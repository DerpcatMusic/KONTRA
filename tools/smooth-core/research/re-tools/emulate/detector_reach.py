"""Does the process block ever execute the reduction detector (0x619fc0) or its callers?

Runs 4 blocks at 48 kHz in both latency modes with the instruction trace on, and reports the
executed image functions that fall in the detector's address span (0x619fc0 .. +6046 bytes,
corpus size) or in its known caller and callee neighbourhood (0x6b96f0, 0x621d50, 0x6181b0,
0x1280, 0x1010). Evidence scope: stand-in state, see process_oracle.py.
Run with: python -I detector_reach.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from detector_run import function_of  # noqa: E402
from pe_emu import IMAGE_BASE  # noqa: E402
from process_lib import PROCESS_THUNK, Session, alloc_frames  # noqa: E402

N = 512
BLOCKS = 4
SPAN = (0x619FC0, 0x619FC0 + 6046)
WATCH = (0x6B96F0, 0x621D50, 0x6181B0, 0x1280, 0x1010)


def main():
    for mode in (0, 1):
        sess = Session(48000.0)
        sess.prepare()
        if mode == 1:
            sess.emu.wr(sess.obj + 0x237, b"\x01")
        e = sess.emu
        hit_span = set()
        functions = set()
        for k in range(BLOCKS):
            src = [0.0] * N
            if k == 0:
                src[0] = 1.0
            tin, _ = alloc_frames(e, 2, N, [src, [0.0] * N])
            tout, _ = alloc_frames(e, 2, N, [[0.0] * N, [0.0] * N])
            pcs = set()
            e.trace_insns(pcs)
            e.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, N, max_insns=400_000_000)
            e.insn_trace = None
            for p in pcs:
                rva = p - IMAGE_BASE
                if SPAN[0] <= rva < SPAN[1]:
                    hit_span.add(rva)
                f = function_of(p)
                if f:
                    functions.add(f[0])
        watched = sorted(w for w in WATCH if w in functions)
        print(f"mode {mode}: executed image functions={len(functions)} "
              f"detector span pcs={len(hit_span)} watched fns executed={[hex(w) for w in watched]}")
        assert not hit_span, "detector body executed"


if __name__ == "__main__":
    main()
