#!/usr/bin/env python3
"""Native relink of the depaced soothe3 image.

The unpacker restores the real `.text` but leaves the *loader-facing* header
pointing at the protector's own bootstrap:

  * `AddressOfEntryPoint` is the proxy wrapper at RVA 0x7acbea0, not the
    plug-in's `DllMain` at RVA 0x3309a0;
  * data directory 1 (imports) points at RVA 0x7acd76c / 0x64, four protector
    DLLs, while the real 38-descriptor table survives at RVA 0x219a298;
  * the TLS directory's callback array still starts with the proxy's
    bootstrap at 0x187acc190.

This script rewrites exactly those three fields. It does not touch `.text`,
the section table, relocations, or the TLS directory itself: the TLS directory
must stay because it is what makes the loader allocate the CRT TLS index that
the plug-in's magic statics use.

The result loads, runs the plug-in's own `DllMain`, and answers as soothe3:
`GetPluginFactory` returns three classes. It is **not host-instantiable**: the
protector's VM bootstrap is still missing, so `createInstance` faults and every
class reports a zero CID. See `depacing.md` for the evidence.

Usage:

    ./relink.py soothe3-restored.bin -o soothe3-native.bin
    ./relink.py --verify soothe3-native.bin
"""

import argparse
import hashlib
import struct
import sys
from pathlib import Path

# The decoded image this recipe was reviewed against. Anything else is refused
# unless --allow-unknown is given.
RESTORED_SHA256 = "0143f03b7c7f1e41a51896f269a64c3dfc4c804e94ce8cccbeef9c98dbbcfa64"
# Output of this script for the reviewed input, for regression checking.
NATIVE_SHA256 = "5663c0b59561750aeec3c8e9771ca4958f20008597c27276c6ee3cfcfa066501"

ENTRY_RVA = 0x3309A0
IMPORT_RVA = 0x219A298
IMPORT_SIZE = 0x30C

DIR_IMPORT = 1
DIR_TLS = 9


def sha256(data):
    return hashlib.sha256(data).hexdigest()


class PE:
    """Just enough PE32+ to translate RVAs and walk two data directories."""

    def __init__(self, data):
        if data[:2] != b"MZ":
            raise ValueError("not an MZ image")
        self.data = data
        self.pe = struct.unpack_from("<I", data, 0x3C)[0]
        if data[self.pe:self.pe + 4] != b"PE\0\0":
            raise ValueError("not a PE image")
        self.coff = self.pe + 4
        self.count, = struct.unpack_from("<H", data, self.coff + 2)
        self.opt_size, = struct.unpack_from("<H", data, self.coff + 16)
        self.opt = self.coff + 20
        magic, = struct.unpack_from("<H", data, self.opt)
        if magic != 0x20B:
            raise ValueError("expected PE32+, got optional-header magic %#x" % magic)
        self.base, = struct.unpack_from("<Q", data, self.opt + 24)
        self.dirs = self.opt + 112
        self.sections = []
        table = self.opt + self.opt_size
        for i in range(self.count):
            o = table + i * 40
            name = data[o:o + 8].rstrip(b"\0").decode("latin-1")
            vsize, rva, rawsize, rawptr = struct.unpack_from("<IIII", data, o + 8)
            self.sections.append(
                {"name": name, "rva": rva, "vsize": vsize, "raw": rawptr, "rawsize": rawsize}
            )

    @property
    def entry(self):
        return struct.unpack_from("<I", self.data, self.opt + 16)[0]

    def dir(self, index):
        return struct.unpack_from("<II", self.data, self.dirs + index * 8)

    def set_dir(self, index, rva, size):
        struct.pack_into("<II", self.data, self.dirs + index * 8, rva, size)

    def offset(self, rva):
        for s in self.sections:
            if s["rva"] <= rva < s["rva"] + max(s["vsize"], s["rawsize"]):
                if rva - s["rva"] >= s["rawsize"]:
                    raise ValueError("RVA %#x is in the zero-filled tail of %s" % (rva, s["name"]))
                return s["raw"] + (rva - s["rva"])
        raise ValueError("RVA %#x is outside every section" % rva)

    def section_of(self, rva):
        for s in self.sections:
            if s["rva"] <= rva < s["rva"] + max(s["vsize"], s["rawsize"]):
                return s["name"]
        return "?"


def imports(pe, rva=IMPORT_RVA):
    """Walk an import descriptor array and return [(dll, first_thunk_rva)]."""
    out = []
    o = pe.offset(rva)
    for i in range(512):
        oft, stamp, chain, name_rva, ft = struct.unpack_from("<IIIII", pe.data, o + i * 20)
        if not (oft or stamp or chain or name_rva or ft):
            break
        no = pe.offset(name_rva)
        end = pe.data.index(b"\0", no)
        out.append((pe.data[no:end].decode("latin-1"), ft))
    else:
        raise ValueError("import descriptor array at %#x has no terminator" % rva)
    return out


def tls(pe):
    rva, _ = pe.dir(DIR_TLS)
    o = pe.offset(rva)
    start, end, index, callbacks = struct.unpack_from("<QQQQ", pe.data, o)
    entries = []
    if callbacks:
        co = pe.offset(callbacks - pe.base)
        for i in range(64):
            value, = struct.unpack_from("<Q", pe.data, co + i * 8)
            if not value:
                break
            entries.append((co + i * 8, value))
    return {"rva": rva, "start": start, "end": end, "index": index,
            "callbacks": callbacks, "entries": entries}


def relink(data, allow_unknown=False):
    pe = PE(bytearray(data))
    digest = sha256(data)
    if digest != RESTORED_SHA256 and not allow_unknown:
        raise SystemExit(
            "input is not the reviewed decoded image\n"
            "  expected sha256 %s\n  got      sha256 %s\n"
            "pass --allow-unknown to relink a different build" % (RESTORED_SHA256, digest)
        )

    before = {
        "entry": pe.entry,
        "import_dir": pe.dir(DIR_IMPORT),
        "callbacks": [v for _, v in tls(pe)["entries"]],
    }

    struct.pack_into("<I", pe.data, pe.opt + 16, ENTRY_RVA)
    pe.set_dir(DIR_IMPORT, IMPORT_RVA, IMPORT_SIZE)

    # Keep the TLS directory, drop the proxy bootstrap from the callback array.
    # The remaining entries stay as they are; the array is terminated by 0.
    for offset, value in tls(pe)["entries"]:
        if value - pe.base == 0x7ACC190:
            struct.pack_into("<Q", pe.data, offset, 0)
            break

    return bytes(pe.data), pe, before


def report(pe, out_sha, before=None):
    entry = pe.entry
    import_rva, import_size = pe.dir(DIR_IMPORT)
    table = imports(pe, import_rva)
    t = tls(pe)
    lines = []
    lines.append("runtime_binding: proxy-vm")
    if before:
        lines.append("")
        lines.append("rewritten")
        lines.append("  entry        %#x -> %#x  (%s)" %
                     (before["entry"], entry, pe.section_of(entry)))
        lines.append("  import dir   %#x/%#x -> %#x/%#x" %
                     (before["import_dir"][0], before["import_dir"][1],
                      import_rva, import_size))
        lines.append("  tls callback %s -> %s" %
                     (", ".join(hex(v) for v in before["callbacks"]) or "none",
                      ", ".join(hex(v) for _, v in t["entries"]) or "none"))
    lines.append("")
    lines.append("header")
    lines.append("  image base   %#x" % pe.base)
    lines.append("  entry        %#x  (%s)" % (entry, pe.section_of(entry)))
    lines.append("  imports      %#x/%#x  %d descriptors" %
                 (import_rva, import_size, len(table)))
    for dll, _ in table[:40]:
        lines.append("      %s" % dll)
    if len(table) > 40:
        lines.append("      ... %d more" % (len(table) - 40))
    lines.append("  tls dir      %#x  index %#x  callbacks %#x" %
                 (t["rva"], t["index"], t["callbacks"]))
    lines.append("      %s" % (", ".join(hex(v) for _, v in t["entries"]) or "none"))
    lines.append("")
    lines.append("output sha256  %s" % out_sha)
    if out_sha == NATIVE_SHA256:
        lines.append("matches the reviewed relink")
    lines.append("")
    lines.append("known limits, not fixable from the header")
    lines.append("  * createInstance faults: the protector's VM bootstrap never ran")
    lines.append("  * every class reports cid 00000000:...: the CID table is VM-built")
    lines.append("  * static locks at 0x2779d78 / 0x2779db0 stay zero-initialised because")
    lines.append("    their initialisers at 0x2ffbc0 / 0x2ffbf0 are unreferenced")
    lines.append("  see depacing.md; do not install this image as a plugin")
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Relink the depaced soothe3 image for native loading")
    parser.add_argument("input", type=Path, help="decoded image (soothe3-restored.bin)")
    parser.add_argument("-o", "--output", type=Path, help="where to write the relinked image")
    parser.add_argument("--verify", action="store_true", help="only inspect, write nothing")
    parser.add_argument("--allow-unknown", action="store_true",
                        help="relink even if the input hash is not the reviewed build")
    args = parser.parse_args(argv)

    data = args.input.read_bytes()
    if args.verify:
        pe = PE(bytearray(data))
        print(report(pe, sha256(data)))
        return 0

    if not args.output:
        parser.error("-o/--output is required unless --verify is used")
    out, pe, before = relink(data, args.allow_unknown)
    args.output.write_bytes(out)
    print(report(pe, sha256(out), before))
    return 0


if __name__ == "__main__":
    sys.exit(main())
