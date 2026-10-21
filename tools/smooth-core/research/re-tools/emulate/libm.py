"""Real implementations for the C math imports the soothe3 DSP path calls.

The default import stub returns 0 in RAX and leaves XMM0 alone, so a float
result from an unimplemented import is whatever XMM0 held before the call. The
block-size function 0x1cbdb35 dispatches to `pow` through a slot (0x181db0888),
and with the default stub every bin boundary came out wrong (2048 bins gave 32
blocks). These handlers return the C result in XMM0 (float or double) or RAX.

Win64 ABI: float/double args come from XMM0..3 by position, integer args from
RCX/RDX/R8/R9 by position. Handlers return None when they set the result
themselves, so the caller's RAX is not overwritten.
"""

import math
import struct

from unicorn.x86_const import (
    UC_X86_REG_EDX,
    UC_X86_REG_RAX,
    UC_X86_REG_XMM0,
    UC_X86_REG_XMM1,
    UC_X86_REG_XMM2,
)


def _xmm(emu, reg):
    raw = emu.uc.reg_read(reg)
    if isinstance(raw, int):
        return raw.to_bytes(16, "little")
    return bytes(raw)[:16]


def _d(emu, reg):
    return struct.unpack("<d", _xmm(emu, reg)[:8])[0]


def _f(emu, reg):
    return struct.unpack("<f", _xmm(emu, reg)[:4])[0]


def _set_d(emu, value):
    emu.uc.reg_write(UC_X86_REG_XMM0, int.from_bytes(struct.pack("<d", value) + bytes(8), "little"))


def _set_f(emu, value):
    emu.uc.reg_write(UC_X86_REG_XMM0, int.from_bytes(struct.pack("<f", value) + bytes(12), "little"))


def _safe(fn):
    """Map Python math domain/overflow errors to IEEE results (NaN / inf)."""

    def run(*args):
        try:
            return fn(*args)
        except ValueError:
            return float("nan")
        except OverflowError:
            return float("inf")

    return run


def _pow(x, y):
    try:
        return math.pow(x, y)
    except ValueError:
        return float("nan")
    except OverflowError:
        return float("inf")


def _exp(x):
    try:
        return math.exp(x)
    except OverflowError:
        return float("inf")


def _log(x):
    if x == 0.0:
        return float("-inf")
    if x < 0.0 or math.isnan(x):
        return float("nan")
    return math.log(x)


def _log2(x):
    if x == 0.0:
        return float("-inf")
    if x < 0.0 or math.isnan(x):
        return float("nan")
    return math.log2(x)


def _log10(x):
    if x == 0.0:
        return float("-inf")
    if x < 0.0 or math.isnan(x):
        return float("nan")
    return math.log10(x)


def _round_half_away(x):
    if math.isnan(x) or math.isinf(x):
        return x
    return math.copysign(math.floor(abs(x) + 0.5), x)


def _fmod(x, y):
    try:
        return math.fmod(x, y)
    except ValueError:
        return float("nan")


def install(emu):
    d1 = {
        "IAT!exp": lambda e, ev: _set_d(e, _exp(_d(e, UC_X86_REG_XMM0))),
        "IAT!exp2": lambda e, ev: _set_d(e, _pow(2.0, _d(e, UC_X86_REG_XMM0))),
        "IAT!log": lambda e, ev: _set_d(e, _log(_d(e, UC_X86_REG_XMM0))),
        "IAT!log2": lambda e, ev: _set_d(e, _log2(_d(e, UC_X86_REG_XMM0))),
        "IAT!log10": lambda e, ev: _set_d(e, _log10(_d(e, UC_X86_REG_XMM0))),
        "IAT!sin": lambda e, ev: _set_d(e, math.sin(_d(e, UC_X86_REG_XMM0))),
        "IAT!cos": lambda e, ev: _set_d(e, math.cos(_d(e, UC_X86_REG_XMM0))),
        "IAT!tan": lambda e, ev: _set_d(e, math.tan(_d(e, UC_X86_REG_XMM0))),
        "IAT!acos": lambda e, ev: _set_d(e, _safe(math.acos)(_d(e, UC_X86_REG_XMM0))),
        "IAT!asinh": lambda e, ev: _set_d(e, math.asinh(_d(e, UC_X86_REG_XMM0))),
        "IAT!ceil": lambda e, ev: _set_d(e, float(math.ceil(_d(e, UC_X86_REG_XMM0))) if math.isfinite(_d(e, UC_X86_REG_XMM0)) else _d(e, UC_X86_REG_XMM0)),
        "IAT!floor": lambda e, ev: _set_d(e, float(math.floor(_d(e, UC_X86_REG_XMM0))) if math.isfinite(_d(e, UC_X86_REG_XMM0)) else _d(e, UC_X86_REG_XMM0)),
        "IAT!round": lambda e, ev: _set_d(e, _round_half_away(_d(e, UC_X86_REG_XMM0))),
        "IAT!pow": lambda e, ev: _set_d(e, _pow(_d(e, UC_X86_REG_XMM0), _d(e, UC_X86_REG_XMM1))),
        "IAT!fmod": lambda e, ev: _set_d(e, _fmod(_d(e, UC_X86_REG_XMM0), _d(e, UC_X86_REG_XMM1))),
        "IAT!hypot": lambda e, ev: _set_d(e, math.hypot(_d(e, UC_X86_REG_XMM0), _d(e, UC_X86_REG_XMM1))),
        "IAT!ldexp": lambda e, ev: _set_d(e, math.ldexp(_d(e, UC_X86_REG_XMM0), _s32(e, UC_X86_REG_EDX))),
        "IAT!lround": lambda e, ev: _ret_i(e, _round_half_away(_d(e, UC_X86_REG_XMM0))),
    }
    f1 = {
        "IAT!expf": lambda e, ev: _set_f(e, _exp(_f(e, UC_X86_REG_XMM0))),
        "IAT!logf": lambda e, ev: _set_f(e, _log(_f(e, UC_X86_REG_XMM0))),
        "IAT!log10f": lambda e, ev: _set_f(e, _log10(_f(e, UC_X86_REG_XMM0))),
        "IAT!sinf": lambda e, ev: _set_f(e, math.sin(_f(e, UC_X86_REG_XMM0))),
        "IAT!cosf": lambda e, ev: _set_f(e, math.cos(_f(e, UC_X86_REG_XMM0))),
        "IAT!tanf": lambda e, ev: _set_f(e, math.tan(_f(e, UC_X86_REG_XMM0))),
        "IAT!roundf": lambda e, ev: _set_f(e, _round_half_away(_f(e, UC_X86_REG_XMM0))),
        "IAT!truncf": lambda e, ev: _set_f(e, float(math.trunc(_f(e, UC_X86_REG_XMM0))) if math.isfinite(_f(e, UC_X86_REG_XMM0)) else _f(e, UC_X86_REG_XMM0)),
        "IAT!powf": lambda e, ev: _set_f(e, _pow(_f(e, UC_X86_REG_XMM0), _f(e, UC_X86_REG_XMM1))),
        "IAT!fmodf": lambda e, ev: _set_f(e, _fmod(_f(e, UC_X86_REG_XMM0), _f(e, UC_X86_REG_XMM1))),
        "IAT!atan2f": lambda e, ev: _set_f(e, math.atan2(_f(e, UC_X86_REG_XMM0), _f(e, UC_X86_REG_XMM1))),
        "IAT!fmaf": lambda e, ev: _set_f(e, _f(e, UC_X86_REG_XMM0) * _f(e, UC_X86_REG_XMM1) + _f(e, UC_X86_REG_XMM2)),
        "IAT!lroundf": lambda e, ev: _ret_i(e, _round_half_away(_f(e, UC_X86_REG_XMM0))),
    }
    for name, fn in {**d1, **f1}.items():
        emu.stub(name, fn)
    emu.stub("IAT!_dsign", lambda e, ev: _ret_i(e, 1 if struct.unpack("<Q", _xmm(e, UC_X86_REG_XMM0)[:8])[0] >> 63 else 0))
    emu.stub("IAT!_dclass", lambda e, ev: _ret_i(e, _dclass(_d(e, UC_X86_REG_XMM0))))
    emu.stub("IAT!_fdclass", lambda e, ev: _ret_i(e, _dclass(_f(e, UC_X86_REG_XMM0))))


def _s32(emu, reg):
    v = emu.uc.reg_read(reg) & 0xFFFFFFFF
    return v - (1 << 32) if v & 0x80000000 else v


def _ret_i(emu, value):
    emu.uc.reg_write(UC_X86_REG_RAX, int(value) & (2**64 - 1))
    return None


def _dclass(x):
    """MSVC _FPCLASS_* mask for a double or float value."""
    if math.isnan(x):
        return 0x2  # quiet NaN (signaling NaN is not distinguished here)
    neg = math.copysign(1.0, x) < 0
    if math.isinf(x):
        return 0x4 if neg else 0x200
    if x == 0.0:
        return 0x20 if neg else 0x40
    normal = abs(x) >= 2.2250738585072014e-308
    if neg:
        return 0x8 if normal else 0x10
    return 0x100 if normal else 0x80
