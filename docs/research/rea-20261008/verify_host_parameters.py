#!/usr/bin/env python3
"""Execute unchanged plugin parameter enumeration code with authored vectors.

Requires installed pefile and unicorn. No Wine, builds, or prefix writes.
The ParameterInfo virtual accessor is authored; plugin construction is untested.
"""
import hashlib
import json
import struct
from pathlib import Path

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64
from unicorn.x86_const import (UC_X86_REG_RAX, UC_X86_REG_RCX,
                              UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_RSP,
                              UC_X86_REG_RIP)


TARGETS = (
    ("kontakt", "/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.vst3plugin",
     "8ed90c4b9dd2bb2c5cc45b64dea144f6cca245c706457de4aa72c643a8acc09d",
     0x180d6acf0, 0x180d6ad10),
    ("uvi_workstation", "/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/UVIWorkstation.vst3/Contents/x86_64-win/UVIWorkstation.vst3",
     "5215dd15ac95038483378925bc55380f48f991aed597ad6c20a0debfe64018b5",
     0x1809397e0, 0x180939800),
)


def main():
    results = []
    for name, path, digest, count_va, info_va in TARGETS:
        raw = Path(path).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == digest, "reference image changed"
        pe = pefile.PE(data=raw, fast_load=True)
        base = pe.OPTIONAL_HEADER.ImageBase
        u = Uc(UC_ARCH_X86, UC_MODE_64)
        u.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 4095) & ~4095)
        u.mem_write(base, pe.get_memory_mapped_image())
        u.mem_map(0x100000, 0x300000)
        stop, accessor = 0x100000, 0x100100
        controller, container, vector = 0x201030, 0x210000, 0x220000
        parameter, vtable, record, output = 0x230000, 0x231000, 0x240000, 0x250020
        stack = 0x3fff08
        u.mem_write(accessor, b"\x48\xb8" + struct.pack("<Q", record) + b"\xc3")
        u.mem_write(parameter, struct.pack("<Q", vtable))
        u.mem_write(vtable + 0x70, struct.pack("<Q", accessor))
        expected = struct.pack("<I", 0xf1234567) + bytes(i % 251 for i in range(788))
        u.mem_write(record, expected)
        u.mem_write(vector, struct.pack("<Q", parameter) * 2050)
        case_count = 0

        def call(va, index=0):
            u.mem_write(stack, struct.pack("<Q", stop))
            for reg, value in ((UC_X86_REG_RCX, controller),
                               (UC_X86_REG_RDX, index & 0xffffffff),
                               (UC_X86_REG_R8, output), (UC_X86_REG_RSP, stack)):
                u.reg_write(reg, value)
            u.emu_start(va, stop, count=10000)
            assert u.reg_read(UC_X86_REG_RIP) == stop, "native path did not return"
            return u.reg_read(UC_X86_REG_RAX) & 0xffffffff

        for count in (None, 0, 1, 2047, 2048, 2049, 2050):
            u.mem_write(controller + 0x20, struct.pack("<Q", 0 if count is None else container))
            if count is not None:
                u.mem_write(container, struct.pack("<QQ", vector, vector + count * 8))
            assert call(count_va) == (count or 0)
            case_count += 1
            for index in sorted({-1, 0, 1, 2047, 2048, 2049, 2050, count or 0, (count or 0) - 1}):
                sentinel = b"\xa5" * (32 + len(expected) + 32)
                u.mem_write(output - 32, sentinel)
                valid = count is not None and 0 <= index < count
                assert call(info_va, index) == (0 if valid else 1), (name, count, index)
                actual = bytes(u.mem_read(output - 32, len(sentinel)))
                wanted = b"\xa5" * 32 + expected + b"\xa5" * 32 if valid else sentinel
                assert actual == wanted, (name, count, index, "ParameterInfo copy or guards differ")
                case_count += 1
            if count:
                u.mem_write(vector, bytes(8))
                assert call(info_va, 0) == 1, "null parameter must fail"
                u.mem_write(vector, struct.pack("<Q", parameter))
                case_count += 1
        results.append({"target": name, "sha256": digest, "cases": case_count,
                        "count_va": hex(count_va), "info_va": hex(info_va),
                        "parameter_info_bytes": len(expected),
                        "authored_vector_sizes": [None, 0, 1, 2047, 2048, 2049, 2050],
                        "count_and_index_helpers_substituted": False,
                        "parameter_info_accessor": "authored virtual method",
                        "live_capacity_or_initialization_verified": False})
        pe.close()
    print(json.dumps({"passed": True, "results": results}, indent=2))


if __name__ == "__main__":
    main()
