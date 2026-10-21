#!/usr/bin/env python3
"""MSVC x64 RTTI: for each class name, print the vtable VA(s) and the vtable's first N slot targets.

    rtti_vt.py NAME [NAME...]   (NAME is the decorated class name without ?A[UV] prefix, e.g. CPSOLAAnalysisRT)
"""
import os
import struct
import sys
import pefile

EXE = os.environ.get("KONTAKT_EXE", "/data/dev/workspace/re-binaries/Kontakt8.exe")
pe = pefile.PE(EXE, fast_load=True)
data = pe.__data__
base = pe.OPTIONAL_HEADER.ImageBase


def find_all(pat):
    out, i = [], data.find(pat)
    while i != -1:
        out.append(i)
        i = data.find(pat, i + 1)
    return out


def vtables_for(name):
    """Yield (td_va, col_va, vtable_va) for every class-name occurrence matching name."""
    for prefix in (b".?AV", b".?AU"):
        for off in find_all(prefix + name.encode() + b"@@"):
            td_off = off - 0x10
            td_rva = pe.get_rva_from_offset(td_off)
            for co in find_all(struct.pack("<I", td_rva)):
                col_off = co - 0xC
                if col_off < 0 or struct.unpack("<I", data[col_off:col_off + 4])[0] not in (0, 1):
                    continue
                col_rva = pe.get_rva_from_offset(col_off)
                for vo in find_all(struct.pack("<Q", base + col_rva)):
                    yield base + td_rva, base + col_rva, base + pe.get_rva_from_offset(vo) + 8


def slots(vt_va, n=16):
    out = []
    for i in range(n):
        raw = pe.get_data(vt_va - base + 8 * i, 8) if False else None
        off = pe.get_offset_from_rva(vt_va - base + 8 * i)
        ptr = struct.unpack("<Q", data[off:off + 8])[0]
        out.append(ptr - base if base <= ptr < base + 0x10000000 else ptr)
    return out


for name in sys.argv[1:]:
    for td, col, vt in vtables_for(name):
        print(f"{name}\tTD={td:#x}\tCOL={col:#x}\tvtable={vt:#x}\tslots=" + ",".join(f"{s:#x}" for s in slots(vt)))
