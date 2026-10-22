#!/usr/bin/env python3
"""Find MSVC vtable addresses for named classes in a PE by walking RTTI.

    rtti_vtables.py Kontakt8.exe BEffectJump [BEffectVan51 ...]    -> class, vtable VA(s)
    rtti_vtables.py Kontakt8.exe --slots 0x144fc0330 40          -> the first N vtable slots

Static parse only (pefile): TypeDescriptor name `.?AV<name>@@` -> CompleteObjectLocator
(x64, RVA-based) -> the qword after the COL pointer in a vtable.
"""
import struct
import sys

import pefile


def vtables(pe, data, name):
    base = pe.OPTIONAL_HEADER.ImageBase
    tname = b".?AV" + name.encode() + b"@@"
    out = []
    i = data.find(tname)
    while i >= 0:
        td_rva = pe.get_rva_from_offset(i - 0x10)  # TypeDescriptor: vfptr, spare, name
        for j in _find_all(data, struct.pack("<I", td_rva)):
            col_off = j - 12  # COL: signature, offset, cdOffset, pTypeDescriptor
            if col_off < 0 or struct.unpack_from("<I", data, col_off)[0] not in (0, 1):
                continue
            target = struct.pack("<Q", base + pe.get_rva_from_offset(col_off))
            for k in _find_all(data, target):
                out.append(base + pe.get_rva_from_offset(k) + 8)
        i = data.find(tname, i + 1)
    return out


def _find_all(data, pat):
    i = data.find(pat)
    while i >= 0:
        yield i
        i = data.find(pat, i + 1)


def main():
    pe = pefile.PE(sys.argv[1], fast_load=True)
    data = pe.__data__
    if sys.argv[2] == "--slots":
        va, n = int(sys.argv[3], 16), int(sys.argv[4])
        for k in range(n):
            off = pe.get_offset_from_rva(va - pe.OPTIONAL_HEADER.ImageBase + 8 * k)
            print(k, hex(struct.unpack_from("<Q", data, off)[0]))
        return
    for name in sys.argv[2:]:
        print(name, [hex(v) for v in vtables(pe, data, name)])


if __name__ == "__main__":
    main()
