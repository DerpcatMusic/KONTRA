"""Native phase-form vectors for the forms that call libm (sync, bend, quantize, exp/log).

Same pinned Kontakt 8 PE as verify_wavetable_forms.py. The native phase switch
0x140567aa0 calls api-ms-win-crt-math helpers through IAT slots. Those slots are
pointed at Python stubs, so the vectors record native branch structure and
constants with helper-substituted math (float64 evaluated, rounded to f32). They
are recorded as such in the output, never as helper-free parity.

    verify_wavetable_phase_libm.py KONTRA_PE OUT.json
"""
import argparse
import hashlib
import json
import math
import pathlib
import struct

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_HOOK_CODE, UC_MODE_64
from unicorn.x86_const import (
    UC_X86_REG_MXCSR, UC_X86_REG_R8, UC_X86_REG_RIP, UC_X86_REG_RSP,
    UC_X86_REG_XMM0, UC_X86_REG_XMM1,
)

SHA = "0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8"
ENTRY = 0x140567AA0
STUB = 0x2400000
STACK = 0x2200000
STOP = 0x2300000
MODES = [1, 2, 3, 4, 5, 6, 13, 24, 25, 26, 28, 29, 30]
AMOUNTS = [0.0, 0.125, 0.5, 0.875, 1.0]
PHASES = [0.0, 0.125, 0.25, 0.5, 0.75, 0.875, 0.999999]
HELPERS = ["asinf", "cosf", "fmodf", "powf", "pow", "roundf", "fmaxf"]


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def bits32(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def float_arg(cpu, reg):
    return struct.unpack("<f", struct.pack("<I", cpu.reg_read(reg) & 0xFFFFFFFF))[0]


def double_arg(cpu, reg):
    return struct.unpack("<d", struct.pack("<Q", cpu.reg_read(reg) & 0xFFFFFFFFFFFFFFFF))[0]


def round_half_away(x):
    return math.copysign(math.floor(abs(x) + 0.5), x)


def helper(name, cpu):
    if name == "pow":
        r = math.pow(double_arg(cpu, UC_X86_REG_XMM0), double_arg(cpu, UC_X86_REG_XMM1))
        return struct.unpack("<Q", struct.pack("<d", r))[0], False
    x = float_arg(cpu, UC_X86_REG_XMM0)
    y = float_arg(cpu, UC_X86_REG_XMM1)
    if name == "cosf":
        r = math.cos(x)
    elif name == "asinf":
        r = math.asin(x)
    elif name == "fmodf":
        r = math.fmod(x, y)
    elif name == "powf":
        r = math.pow(x, y)
    elif name == "roundf":
        r = round_half_away(x)
    elif name == "fmaxf":
        r = max(x, y)
    else:
        raise ValueError(name)
    return bits32(f32(r)), True


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("binary", type=pathlib.Path)
    ap.add_argument("output", type=pathlib.Path)
    args = ap.parse_args()
    raw = args.binary.read_bytes()
    sha = hashlib.sha256(raw).hexdigest()
    assert sha == SHA, sha

    pe = pefile.PE(data=raw, fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]])
    base = pe.OPTIONAL_HEADER.ImageBase
    slots = {}
    for entry in pe.DIRECTORY_ENTRY_IMPORT:
        for imp in entry.imports:
            if imp.name and imp.name.decode() in HELPERS:
                slots[imp.name.decode()] = imp.address

    cpu = Uc(UC_ARCH_X86, UC_MODE_64)
    cpu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 4095) & ~4095)
    cpu.mem_write(base, pe.get_memory_mapped_image())
    cpu.mem_map(STACK, 0x10000)
    cpu.mem_map(STOP, 0x1000)
    cpu.mem_map(STUB, 0x1000)
    cpu.mem_write(STOP, b"\xcc")

    # Each helper slot points at its own stub address; the stub hook computes the result.
    stub_of = {}
    for i, name in enumerate(HELPERS):
        if name not in slots:
            continue
        stub = STUB + 0x10 * i
        stub_of[stub] = name
        cpu.mem_write(slots[name], struct.pack("<Q", stub))

    def hook(uc, address, size, user):
        name = stub_of[address]
        value, is_float = helper(name, uc)
        if is_float:
            uc.reg_write(UC_X86_REG_XMM0, value & 0xFFFFFFFF)
        else:
            uc.reg_write(UC_X86_REG_XMM0, value)
        ret = struct.unpack("<Q", uc.mem_read(uc.reg_read(UC_X86_REG_RSP), 8))[0]
        uc.reg_write(UC_X86_REG_RSP, uc.reg_read(UC_X86_REG_RSP) + 8)
        uc.reg_write(UC_X86_REG_RIP, ret)

    for stub in stub_of:
        cpu.hook_add(UC_HOOK_CODE, hook, begin=stub, end=stub + 1)
    rows = []
    for mode in MODES:
        for amount in AMOUNTS:
            for phase in PHASES:
                rsp = STACK + 0xFF00 - 8
                cpu.mem_write(rsp, struct.pack("<Q", STOP))
                cpu.reg_write(UC_X86_REG_RSP, rsp)
                cpu.reg_write(UC_X86_REG_MXCSR, 0x1F80)
                cpu.reg_write(UC_X86_REG_XMM0, bits32(f32(phase)))
                cpu.reg_write(UC_X86_REG_XMM1, bits32(f32(amount)))
                cpu.reg_write(UC_X86_REG_R8, mode)
                cpu.emu_start(ENTRY, STOP, timeout=100000, count=10000)
                assert cpu.reg_read(UC_X86_REG_RIP) == STOP
                value = struct.unpack("<f", struct.pack("<I", cpu.reg_read(UC_X86_REG_XMM0) & 0xFFFFFFFF))[0]
                rows.append([mode, phase, amount, value])
    args.output.write_text(json.dumps({
        "sha256": sha,
        "entry": "0x140567aa0",
        "helper_substitutions": HELPERS,
        "helper_precision": "float64 libm evaluated, rounded to f32",
        "cases": rows,
    }, indent=2) + "\n")
    print("Native cases", len(rows), "helpers substituted", len(stub_of))


if __name__ == "__main__":
    main()
