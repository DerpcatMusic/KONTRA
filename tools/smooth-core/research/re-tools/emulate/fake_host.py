"""Fake-host run of the DSP constructor (0x88fa90) and its builder (0x88f430).

The constructor registers parameter names through host vtable slot +0x18 and
reads the sidechain value back through slot +0x20. This script supplies a
host whose two slots are Python hooks: it logs every registration and every
get, and answers gets with a default. It then runs the constructor for several
sample rates and dumps the object fields the constructor derives from them.

Evidence scope: isolated emulation of one constructor against a synthetic host.
The host defaults below are our guesses, not values read from the plug-in.
Run with: python -I fake_host.py
"""

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from pe_emu import TEB_BASE, Emulator, image_va  # noqa: E402

CTOR = image_va(0x88FA90)
BUILDER = image_va(0x88F430)
OBJ_SIZE = 0xC1A8
HOST_SLOT_REGISTER = 3  # +0x18
HOST_SLOT_GET = 4  # +0x20

# Getter defaults. Only id 0xd is read by the constructor; the rest are
# recorded as requests.
GET_DEFAULTS = {0xD: 0}


def read_cstr(emu, addr, limit=128):
    out = bytearray()
    for i in range(limit):
        b = emu.rd(addr + i, 1)[0]
        if b == 0:
            break
        out.append(b)
    return out.decode("latin-1")


def make_host(emu, log, host_values=None):
    """host_values: {id: float} written as float32 by get(id, out). Ids not listed get GET_DEFAULTS.

    The constructor reads ids 10 (input_channel_layout) and 12 (samplerate) as floats and sizes
    its ring buffers from them (ring setup in 0x88fa90, decompile 0088fa90.c lines ~403-420:
    layout -> channels, samplerate -> groups, length = groups << 12). A host that returns 0 for
    both builds zero-channel rings (section 17).
    """
    values = host_values or {}

    def on_register(emu, ev):
        name = read_cstr(emu, ev.r8)
        log.append(("register", ev.rdx, name, ev.r9 & 0xFFFFFFFF))
        return 0

    def on_get(emu, ev):
        if ev.rdx in values:
            emu.wr(ev.r8, struct.pack("<f", values[ev.rdx]) + b"\0" * 4)
            log.append(("get", ev.rdx, values[ev.rdx], ev.r9 & 0xFFFFFFFF))
            return 0
        value = GET_DEFAULTS.get(ev.rdx, 0)
        emu.wr(ev.r8, struct.pack("<Q", value))
        log.append(("get", ev.rdx, value, ev.r9 & 0xFFFFFFFF))
        return 0

    # The constructor calls `call [host+0x18]` and `call [host+0x20]` with rcx = host
    # (0x88fb67, 0x88fc80). The host holds the function pointers directly; there is
    # no vtable indirection at this call site.
    host = emu.arena(0x40)
    emu.w64(host + 0x18, emu.stub("Host::register", on_register))
    emu.w64(host + 0x20, emu.stub("Host::get", on_get))
    return host


def make_cfg(emu, fields):
    cfg = emu.arena(0x40)
    for off, value in fields.items():
        emu.w64(cfg + off, value)
    return cfg


def install_tls(emu):
    """TEB+0x58 is the TLS array; the constructor reads per-thread slots from it."""
    arr = emu.arena(8 * 256)
    for i in range(256):
        emu.w64(arr + 8 * i, emu.arena(0x200))
    emu.w64(TEB_BASE + 0x58, arr)


def set_cpu_mode(emu):
    """Run the mode writer 0x30d310 with its input flag set, as CPU detection would.

    0x30d310 sets the analysis mode (0x26d7bb8) to 4 when bit 0 of the cpuid leaf-1
    feature word (0x26e00ec) is set, else 1. The detection routine 0x88c5f0 is not
    run: its record loop does not reproduce under emulation (open). The flag is
    set to what a reference CPU reports (SSE3 present).
    """
    emu.w32(image_va(0x26E00EC), 1)
    emu.call(image_va(0x30D310), max_insns=1000)


def run_ctor(rate, cfg_fields=None, host_values=None):
    """Run the constructor. host_values defaults to layout 2 (stereo) and samplerate = rate."""
    emu = Emulator()
    install_tls(emu)
    set_cpu_mode(emu)
    log = []
    if host_values is None:
        host_values = {10: 2.0, 12: float(rate)}
    host = make_host(emu, log, host_values)
    cfg = make_cfg(emu, cfg_fields or {})
    obj = emu.arena(OBJ_SIZE)
    emu.call(CTOR, obj, host, cfg, floats=(0.0, 0.0, 0.0, rate), max_insns=400_000_000)
    return emu, obj, log


def f32_at(emu, addr):
    return struct.unpack("<f", emu.rd(addr, 4))[0]


def u32_at(emu, addr):
    return struct.unpack("<I", emu.rd(addr, 4))[0]


def main():
    for rate in (44100.0, 48000.0, 96000.0, 192000.0):
        emu, obj, log = run_ctor(rate)
        if rate == 44100.0:
            print("registrations (id, name, flag):")
            for entry in log:
                if entry[0] == "register":
                    print(f"  {entry[1]:2d} {entry[2]:32s} flag={entry[3]}")
            gets = [e for e in log if e[0] == "get"]
            print("getter requests:", [(e[1], e[2]) for e in gets])
        print(f"rate {rate:.0f}")
        print(f"  obj+0x10 (cad0 *cfg)   = {u32_at(emu, obj + 0x10)}")
        print(f"  obj+0x14 (cad0 cfg[2]) = {u32_at(emu, obj + 0x14)}")
        print(f"  obj+0x28 rate          = {f32_at(emu, obj + 0x28):.1f}")
        print(f"  obj+0x2c groups/m      = {u32_at(emu, obj + 0x2C)}")
        print(f"  obj+0x248 vec count    = {u32_at(emu, obj + 0x248)}")
        print(f"  obj+0xb7a8 group0      = {[round(f32_at(emu, obj + 0xB7A8 + 4 * i), 3) for i in range(16)]}")
        print(f"  obj+0xbf28 coeffs      = {[round(f32_at(emu, obj + 0xBF28 + 4 * i), 6) for i in range(0, 0x30, 4)]}")
        print(f"  events: {len(emu.events)} stub calls, aborted={emu.aborted}")


if __name__ == "__main__":
    main()
