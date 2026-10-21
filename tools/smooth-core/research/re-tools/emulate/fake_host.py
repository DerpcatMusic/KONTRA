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


def make_host(emu, log):
    def on_register(emu, ev):
        name = read_cstr(emu, ev.r8)
        log.append(("register", ev.rdx, name, ev.r9 & 0xFFFFFFFF))
        return 0

    def on_get(emu, ev):
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


def run_ctor(rate, cfg_fields=None):
    emu = Emulator()
    install_tls(emu)
    log = []
    host = make_host(emu, log)
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
