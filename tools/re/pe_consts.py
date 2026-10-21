"""pe_consts.py EXE VA [VA...]: print the float32 and raw bits stored at each virtual address of
a PE image (e.g. the constants a decompiled `DAT_<va>` / `_DAT_<va>` refers to). Reads the file
bytes only; nothing is executed. Addresses are hex, with or without the `0x` prefix.

    python3 tools/re/pe_consts.py /data/dev/workspace/re-binaries/Kontakt8.exe 144fcfe3c 144714a38
"""
import struct
import sys

import pefile


def main(argv):
    if len(argv) < 3:
        raise SystemExit(__doc__)
    pe = pefile.PE(argv[1], fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    for text in argv[2:]:
        va = int(text, 16)
        raw = pe.get_data(va - base, 4)
        if len(raw) < 4:
            print(f"{va:#x}: not file-backed (bss or past the section end)")
            continue
        (bits,) = struct.unpack("<I", raw)
        (value,) = struct.unpack("<f", raw)
        print(f"{va:#x}: {value!r}  bits={bits:#010x}")


if __name__ == "__main__":
    main(sys.argv)
