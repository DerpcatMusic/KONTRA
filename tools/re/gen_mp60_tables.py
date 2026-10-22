#!/usr/bin/env python3
"""Extract the MP60 coefficient block and render constants from Kontakt 8.

Reads the object constructor FUN_140cd8ea0 (BMPC60) from the Ghidra decompile
export, collecting every field store into the object at a byte offset, and reads
the float/double constants the render FUN_140cddf50 compares against from .rdata.
Prints binary32 / float64 bit patterns; the stage topology lives in
crates/sampler-core/src/dsp/machine.rs and is not generated here.

    python3 tools/re/gen_mp60_tables.py <Kontakt8.exe> <decompile dir>
"""
import re
import struct
import sys

import pefile

BASE = 0x140000000
CONSTRUCTOR = ("0140cd0000.c", 4319, 4424)
RENDER_CONSTANTS = {
    "DAT_14470bad0": (4, 0x14470BAD0),
    "DAT_14470bad4": (4, 0x14470BAD4),
    "DAT_144fd0d94": (4, 0x144FD0D94),
    "DAT_144fd0d78": (4, 0x144FD0D78),
    "DAT_144fd0d70": (4, 0x144FD0D70),
    "DAT_144714a54": (4, 0x144714A54),
    "DAT_144fd0dcc": (4, 0x144FD0DCC),
    "DAT_144fd0d4c": (4, 0x144FD0D4C),
    "DAT_144f38820": (4, 0x144F38820),
    "DAT_1447149d0": (4, 0x1447149D0),
    "DAT_1447148f8": (4, 0x1447148F8),
    "DAT_144f385c4": (4, 0x144F385C4),
    "DAT_144fc4c24": (4, 0x144FC4C24),
    "DAT_144f38d58": (4, 0x144F38D58),
    "DAT_144714988": (8, 0x144714988),
    "DAT_144714998": (8, 0x144714998),
}

# `*(undefined4 *)(param_1 + N)` (N in qwords), `(longlong)param_1 + N` (bytes),
# `param_1[N] = V` (qword index, 4 bytes), `*(undefined8 *)... = V` (8 bytes).
STORE = re.compile(
    r"\*\((undefined[48]) \*\)\((?:\(longlong\)param_1 \+ (0x[0-9a-f]+)|param_1 \+ (0x[0-9a-f]+))\) = (0x[0-9a-f]+);"
    r"|param_1\[(0x[0-9a-f]+)\] = (0x[0-9a-f]+);"
)


def constructor_stores(lines):
    stores = {}
    for line in lines:
        for m in STORE.finditer(line):
            if m.group(1):
                size = 8 if m.group(1) == "undefined8" else 4
                offset = int(m.group(2), 16) if m.group(2) else int(m.group(3), 16) * 8
                value = int(m.group(4), 16)
            else:
                size = 4
                offset = int(m.group(5), 16) * 8
                value = int(m.group(6), 16)
            stores[offset] = value & 0xFFFFFFFF
            if size == 8:
                stores[offset + 4] = (value >> 32) & 0xFFFFFFFF
    return stores


def main(exe, decompile_dir):
    path = f"{decompile_dir}/{CONSTRUCTOR[0]}"
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = f.readlines()[CONSTRUCTOR[1] - 1 : CONSTRUCTOR[2]]
    stores = constructor_stores(lines)
    pe = pefile.PE(exe, fast_load=True)
    data = pe.__data__
    print("# constructor stores (byte offset: u32)")
    for off in sorted(stores):
        if 0xA30 <= off <= 0xBF8:
            print(f"0x{off:03x}: 0x{stores[off]:08x}")
    print("# render constants")
    for name, (size, va) in RENDER_CONSTANTS.items():
        off = pe.get_offset_from_rva(va - BASE)
        if size == 4:
            print(f"{name}: 0x{struct.unpack_from('<I', data, off)[0]:08x}")
        else:
            print(f"{name}: 0x{struct.unpack_from('<Q', data, off)[0]:016x}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
