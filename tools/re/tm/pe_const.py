#!/usr/bin/env python3
"""pe_const.py VA... : read the 4-byte value at each Kontakt8.exe virtual address (float and int views)."""
import struct, sys
import pefile
pe = pefile.PE("/data/dev/workspace/re-binaries/Kontakt8.exe", fast_load=True)
base = pe.OPTIONAL_HEADER.ImageBase
for a in sys.argv[1:]:
    va = int(a, 16)
    off = pe.get_offset_from_rva(va - base)
    raw = pe.__data__[off:off + 4]
    f = struct.unpack("<f", raw)[0]
    i = struct.unpack("<I", raw)[0]
    print(f"{a}\tfloat={f!r}\tbits={i:#010x}")
