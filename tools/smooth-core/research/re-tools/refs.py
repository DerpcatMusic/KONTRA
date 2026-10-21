"""Find every reference to a code address in the soothe3 restored image.

Covers the forms that a plain rip-relative scan misses:
  - rel32 call/jmp into the target (direct calls and tail jumps)
  - rip-relative lea/mov (function pointers loaded into registers)
  - abs64 and rva32 entries in data (vtables, dispatch and switch tables)
and, for every abs64 hit inside a vtable, the vtable start, slot index and
RTTI class name (MSVC COL -> TypeDescriptor).

Usage (run with -I, from any directory):
  python3 -I refs.py 0x621d50 [0x62a710 ...] [--image PATH]

Addresses are RVA or VA (>= 0x180000000). Output is plain text.
"""

import re
import struct
import sys

import pefile

DEFAULT_IMAGE = "/tmp/opencode/rea/soothe3-restored.bin"


class Image:
    def __init__(self, path=DEFAULT_IMAGE):
        self.pe = pefile.PE(path, fast_load=True)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.data = self.pe.__data__
        self.sections = [
            (s.Name.rstrip(b"\0").decode(errors="replace"), s.VirtualAddress,
             max(s.Misc_VirtualSize, s.SizeOfRawData), s.PointerToRawData,
             s.Characteristics)
            for s in self.pe.sections
        ]

    def off(self, rva):
        for _, va, size, raw, _ in self.sections:
            if va <= rva < va + size:
                return raw + (rva - va)
        return None

    def q(self, rva):
        return struct.unpack_from("<Q", self.data, self.off(rva))[0]

    def d(self, rva):
        return struct.unpack_from("<I", self.data, self.off(rva))[0]

    def cstr(self, rva):
        o = self.off(rva)
        return bytes(self.data[o:o + 256]).split(b"\0", 1)[0].decode(errors="replace")

    def is_code(self, va):
        return self.base + 0x1000 <= va < self.base + 0x1CFA000

    def code_sections(self):
        for name, va, size, raw, ch in self.sections:
            if ch & 0x20:
                yield name, va, raw, size

    def rtti_name(self, vtable_rva):
        """Class name from the COL stored just before the vtable (x64 layout)."""
        col = self.q(vtable_rva - 8) - self.base
        if self.off(col) is None:
            return None
        td = self.d(col + 12)
        return self.cstr(td + 16)

    def vtable_for_slot(self, rva):
        """Walk back over code pointers to the vtable start containing `rva`."""
        start = rva
        while start - 8 >= 0x1000 and self.is_code(self.q(start - 8)):
            start -= 8
        return start

    def slots(self, vt_rva, limit=64):
        out = []
        i = 0
        while i < limit and self.is_code(self.q(vt_rva + 8 * i)):
            out.append(self.q(vt_rva + 8 * i) - self.base)
            i += 1
        return out


def find_refs(img, targets):
    t_rva = {t - img.base if t >= img.base else t for t in targets}
    hits = []
    for name, va, raw, size in img.code_sections():
        blob = img.data[raw:raw + size]
        # rel32 call/jmp and rip-relative operands (disp32 at p, insn ends at p+4+k)
        for p in range(len(blob) - 4):
            d = struct.unpack_from("<i", blob, p)[0]
            for k in (0, 1, 4):
                tgt = va + p + 4 + k + d
                if tgt in t_rva:
                    kind = "rel32" if k == 0 and p >= 1 and blob[p - 1] in (0xE8, 0xE9) else "rip"
                    hits.append((kind, name, va + p - (1 if kind == "rel32" else 0)))
    for name, va, size, raw, _ in img.sections:
        blob = img.data[raw:raw + size]
        for t in t_rva:
            for fmt, kind, val in (("<Q", "abs64", t + img.base), ("<I", "rva32", t)):
                pat = struct.pack(fmt, val)
                i = blob.find(pat)
                while i != -1:
                    if not (kind == "rva32" and i % 4):
                        hits.append((kind, name, va + i))
                    i = blob.find(pat, i + 1)
    return hits


def main(argv):
    img_path = DEFAULT_IMAGE
    if "--image" in argv:
        i = argv.index("--image")
        img_path = argv[i + 1]
        argv = argv[:i] + argv[i + 2:]
    targets = [int(a, 16) for a in argv]
    img = Image(img_path)
    for t in targets:
        print(f"== refs to {hex(t)}")
        for kind, sec, where in find_refs(img, [t]):
            line = f"  {kind:5s} {sec:6s} at RVA {hex(where)}"
            if kind == "abs64" and sec == ".rdata":
                vt = img.vtable_for_slot(where)
                idx = (where - vt) // 8
                line += (f"  vtable {hex(vt)} slot {idx}"
                         f" class {img.rtti_name(vt)!r}")
            print(line)


if __name__ == "__main__":
    main(sys.argv[1:])
