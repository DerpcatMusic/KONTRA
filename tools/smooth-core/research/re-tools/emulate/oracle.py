"""Constructor-state oracle: latency and group layout per sample rate.

Runs the DSP constructor (0x88fa90) under emulation with a synthetic host, then
calls getLatency (0x88f610) on the result. Writes JSON to the path given as
argument 1 (default: stdout).

Evidence scope: isolated emulation of one constructor with a fake host, a
reference-class CPU profile and the libm stubs. It is not a plug-in run and
does not run the process path. The numbers describe the constructor state only.

Run with: python -I oracle.py [out.json] [--mode 4|1] [--rates 44100,48000,...]
"""

import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from fake_host import CTOR, OBJ_SIZE, f32_at, make_cfg, make_host, install_tls, u32_at  # noqa: E402
from pe_emu import Emulator, image_va  # noqa: E402

GETLATENCY = image_va(0x88F610)
MODE_WRITER = image_va(0x30D310)
CPU_FLAG = image_va(0x26E00EC)
DEFAULT_RATES = [44100.0, 48000.0, 64000.0, 88200.0, 96000.0, 110250.0, 132300.0, 176400.0, 192000.0]


def run_constructor(rate, mode=4):
    """Build one DSP object at `rate`. mode 4 sets CPU flag bit 0; mode 1 clears it."""
    emu = Emulator()
    install_tls(emu)
    emu.w32(CPU_FLAG, 1 if mode == 4 else 0)
    emu.call(MODE_WRITER, max_insns=1000)
    log = []
    host = make_host(emu, log)
    cfg = make_cfg(emu, {})
    obj = emu.arena(OBJ_SIZE)
    emu.call(CTOR, obj, host, cfg, floats=(0.0, 0.0, 0.0, rate), max_insns=400_000_000)
    return emu, obj


def get_latency(emu, obj):
    # getLatency reads X = [rcx], so the argument is a pointer to the object pointer.
    holder = emu.arena(8)
    emu.w64(holder, obj)
    return emu.call(GETLATENCY, holder, max_insns=10_000_000)


def measure(rate, mode=4):
    emu, obj = run_constructor(rate, mode)
    row = {
        "rate": rate,
        "mode": mode,
        "groups": u32_at(emu, obj + 0x2C),
        "vec_count": u32_at(emu, obj + 0x248),
        "base": u32_at(emu, obj + 0x278),
        # H = [a+0x2c78] + [a+0x30] with a = obj+0x5f8 (the single-branch value; see README)
        "h": u32_at(emu, obj + 0x5F8 + 0x2C78) + u32_at(emu, obj + 0x5F8 + 0x30),
        "latency": get_latency(emu, obj),
        # Entry 14 of group 0 is a double at +0xb7e0, not a float (see README).
        "group0_double_b7e0": struct.unpack("<d", emu.rd(obj + 0xB7E0, 8))[0],
        "aborted": emu.aborted,
    }
    row["group0_float_e1"] = f32_at(emu, obj + 0xB7A8 + 4)
    return row


def main(argv):
    out = None
    mode = 4
    rates = DEFAULT_RATES
    args = list(argv)
    while args:
        a = args.pop(0)
        if a == "--mode":
            mode = int(args.pop(0))
        elif a == "--rates":
            rates = [float(r) for r in args.pop(0).split(",")]
        else:
            out = a
    rows = [measure(r, mode) for r in rates]
    text = json.dumps({"source": "constructor-state emulation (fake host, cpu profile)", "rows": rows}, indent=2)
    if out:
        Path(out).write_text(text + "\n")
    else:
        print(text)


if __name__ == "__main__":
    main(sys.argv[1:])
