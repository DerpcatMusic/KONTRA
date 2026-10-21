"""Shared setup for the gain-body harnesses: a 48 kHz session with non-identity parameters.

Parameters go through the real setter 0x894370 (ids and transforms from the setter table,
`RunScratch/smooth/param-translate/table.md`), so dirty flags and derived fields are the
binary's own. Input: white noise plus a strong 1 kHz sine, both channels.

Evidence scope: isolated emulation, synthetic host; the parameter values are test values.
"""

import math
import struct
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

from unicorn.x86_const import UC_X86_REG_XMM0  # noqa: E402

from pe_emu import image_va  # noqa: E402
from process_lib import PROCESS_THUNK, Session, alloc_frames  # noqa: E402
from reference_output_fixture import ParamSetter  # noqa: E402

BODY_LO = image_va(0x8E04C0)
BODY_HI = image_va(0x8E4BE2)
N = 512
RATE = 48000

# (id, value, lo, hi, scale, offset): value is the plain value the descriptor maps x to.
NORM = (0.0, 1.0, 1.0, 0.0)
PARAMS = [
    (0, 0.8, *NORM),  # depth (norm)
    (3, 0.0, 0.0, 1.0, 1.0, 0.0),  # mode soft
    (5, 0.5, *NORM),  # focus
    (10, 5.0, 0.0, 10.0, 10.0, 0.0),  # detail
    (11, 5.0, 0.0, 10.0, 10.0, 0.0),  # attack
    (12, 3.0, 0.0, 10.0, 10.0, 0.0),  # release
    (17, 0.5, *NORM), (18, 0.5, *NORM),  # detail tilt
    (123, 0.5, *NORM), (124, 0.5, *NORM), (125, 0.5, *NORM), (126, 0.5, *NORM),
    (92, 12.0, 0.0, 24.0, 24.0, 0.0),  # max cut dB
    # band 1
    (19, 1.0, 0.0, 1.0, 1.0, 0.0), (20, 1.0, 0.0, 1.0, 1.0, 0.0),
    (21, 1000.0, 20.0, 20000.0, 19980.0, 20.0),
    (22, 0.9, *NORM),  # depth 24x-12
    (23, 0.707, 0.1, 10.0, 9.9, 0.1),
    (24, 0.5, *NORM), (25, 0.5, 0.0, 1.0, 1.0, 0.0), (26, 0.0, 0.0, 7.0, 7.0, 0.0),
]


def xmm_f32(emu, i):
    raw = emu.uc.reg_read(UC_X86_REG_XMM0 + i)
    raw = raw if isinstance(raw, bytes) else int(raw).to_bytes(16, "little")
    return struct.unpack("<f", raw[:4])[0]


def make_session(params=PARAMS, rate=RATE):
    sess = Session(rate)
    sess.prepare()
    setter = ParamSetter(sess)
    for pid, value, lo, hi, scale, offset in params:
        setter.set(pid, value, lo, hi, scale, offset)
    return sess


def test_input(n=N, seed=1, block=0):
    rng = np.random.default_rng(seed + block)
    t = np.arange(block * n, (block + 1) * n)
    sig = 0.05 * rng.standard_normal(n) + 0.7 * np.sin(2 * math.pi * 1000.0 * t / RATE)
    return sig.astype(np.float32).tolist()


def run_block(sess, block=0, n=N):
    emu = sess.emu
    x = test_input(n, block=block)
    tin, _ = alloc_frames(emu, 2, n, [x, x])
    tout, bufs = alloc_frames(emu, 2, n, [[0.0] * n, [0.0] * n])
    emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, n, max_insns=800_000_000)
    return bufs
