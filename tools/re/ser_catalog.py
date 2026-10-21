#!/usr/bin/env python3
"""Catalog Kontakt's serializable classes straight from the executable.

    tools/re/ser_catalog.py VTABLE_METHODS.csv KONTAKT.exe > ser_catalog.tsv

Kontakt's serializable objects (BPar*, BGroup, BProgram, ...) share one vtable
layout, established on BParExternalMod (Kontakt 8.13.1): slot 1 returns the
SerType, slot 2 returns the current write version, slot 11 is the versioned
reader `read(stream, version, ctx)` (FUN_140d09620). The 10-slot FX parameter
family (BParFX*) reads at slot 8 (BParFXDelay FUN_140d00b60). Slots 1/2 are compiled as
`mov eax, imm32; ret`, so they are read from bytes, no decompiler needed.
"""
import csv
import struct
import sys
from collections import defaultdict


def sections(exe: bytes):
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    count = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    base = struct.unpack_from("<Q", exe, pe + 24 + 24)[0]
    out = []
    for i in range(count):
        o = pe + 24 + opt + 40 * i
        vsize, va, rsize, raw = struct.unpack_from("<IIII", exe, o + 8)
        out.append((base + va, max(vsize, rsize), raw))
    return out


def reader(exe: bytes, secs):
    def at(va: int, n: int) -> bytes:
        for start, size, raw in secs:
            if start <= va < start + size:
                off = raw + va - start
                return exe[off:off + n]
        return b""
    return at


def const_return(code: bytes):
    # mov eax, imm32 ; ret       (B8 imm32 C3)
    if len(code) >= 6 and code[0] == 0xB8 and code[5] == 0xC3:
        return struct.unpack_from("<I", code, 1)[0]
    # xor eax, eax ; ret         (33 C0 C3 / 31 C0 C3)
    if code[:3] in (b"\x33\xc0\xc3", b"\x31\xc0\xc3"):
        return 0
    return None


def main():
    methods, exe_path = sys.argv[1], sys.argv[2]
    exe = open(exe_path, "rb").read()
    at = reader(exe, sections(exe))
    slots = defaultdict(dict)  # (type, vtable) -> {slot: entry}
    with open(methods, newline="") as f:
        for row in csv.DictReader(f):
            if row["object_displacement"] != "0":
                continue
            slots[(row["type"], row["vtable_rva"])][int(row["slot"])] = int(row["entry_va"], 16)
    print("class\tsertype\tversion\treader_va\tvtable_rva")
    rows = []
    for (cls, vt), s in slots.items():
        if not {1, 2, 8} <= s.keys():
            continue
        sertype, version = const_return(at(s[1], 8)), const_return(at(s[2], 8))
        if sertype is None or version is None or version == 0 or version > 0xffff:
            continue
        rows.append((sertype, cls, version, s[11] if len(s) >= 12 else s[8], vt))
    for sertype, cls, version, rd, vt in sorted(rows):
        print(f"{cls}\t0x{sertype:02X}\t0x{version:X}\t0x{rd:x}\t{vt}")
    print(f"{len(rows)} serializable classes", file=sys.stderr)


if __name__ == "__main__":
    main()
