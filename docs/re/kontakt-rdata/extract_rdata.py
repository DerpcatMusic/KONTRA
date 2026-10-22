#!/usr/bin/env python3
"""Extract Kontakt 8 .rdata facts from the PE, read-only.

Usage (never run from inside the source dirs):
  python3 -I extract_rdata.py <Kontakt8.exe> <kontakt8 .c dir> <out dir>

Writes vtables.tsv, funcimpl.tsv, vtable_xrefs.tsv, constants.tsv, tables.tsv.
Only reads files. No Ghidra, radare2, Wine or unpacker is started.
"""
import array
import bisect
import collections
import gzip
import hashlib
import os
import re
import struct
import sys

import pefile

IMAGE_SCN_MEM_EXECUTE = 0x20000000
exe_path, src_dir, out_dir = sys.argv[1:4]
os.makedirs(out_dir, exist_ok=True)

pe = pefile.PE(exe_path, fast_load=True)
IB = pe.OPTIONAL_HEADER.ImageBase
SOI = pe.OPTIONAL_HEADER.SizeOfImage
with open(exe_path, "rb") as f:
    RAW = f.read()
print("exe sha256", hashlib.sha256(RAW).hexdigest(), "imagebase", hex(IB), "sizeofimage", hex(SOI))

# ---------- section / VA helpers ----------
secs = []
for s in pe.sections:
    secs.append({
        "name": s.Name.rstrip(b"\0").decode("ascii", "replace"),
        "va": IB + s.VirtualAddress,
        "vsize": s.Misc_VirtualSize,
        "raw_off": s.PointerToRawData,
        "raw_len": min(s.Misc_VirtualSize, s.SizeOfRawData),
        "exec": bool(s.Characteristics & IMAGE_SCN_MEM_EXECUTE),
    })
secs.sort(key=lambda s: s["va"])
SEC_STARTS = [s["va"] for s in secs]


def sec_at(va):
    i = bisect.bisect_right(SEC_STARTS, va) - 1
    if i >= 0 and va < secs[i]["va"] + secs[i]["vsize"]:
        return secs[i]
    return None


def rd(va, n):
    """File-backed bytes at VA, or None (bss, out of range, crossing sections)."""
    s = sec_at(va)
    if s is None:
        return None
    off = va - s["va"]
    if off + n > s["raw_len"]:
        return None
    return RAW[s["raw_off"] + off: s["raw_off"] + off + n]


def u32(va):
    b = rd(va, 4)
    return struct.unpack("<I", b)[0] if b else None


def u64(va):
    b = rd(va, 8)
    return struct.unpack("<Q", b)[0] if b else None


def is_code(v):
    s = sec_at(v)
    return s is not None and s["exec"] and v - s["va"] < s["raw_len"]


def cstr(va, n=256):
    b = rd(va, n) or b""
    return b.split(b"\0", 1)[0].decode("ascii", "replace")


def sec_name(va):
    s = sec_at(va)
    return s["name"] if s else "-"


# ---------- 1. RTTI: COL -> TypeDescriptor, vtable = COL pointer + 8 ----------
vtables = {}  # vt_va -> dict
cd_cache = {}
for s in secs:
    if s["exec"] or s["raw_len"] < 8:
        continue
    n8 = s["raw_len"] // 8
    arr = array.array("Q")
    arr.frombytes(RAW[s["raw_off"]: s["raw_off"] + n8 * 8])
    for i, v in enumerate(arr):
        if not (IB <= v < IB + SOI):
            continue
        col = rd(v, 0x18)
        if col is None:
            continue
        sig, off, cdoff, ptd, pcd, pself = struct.unpack("<6I", col)
        if sig != 1 or pself != v - IB:
            continue
        tname = cstr(IB + ptd + 0x10)
        if not tname.startswith(".?A"):
            continue
        cd = IB + pcd
        if cd not in cd_cache:
            nbase = u32(cd + 8) or 0
            pbca = IB + (u32(cd + 0xC) or 0)
            bases = []
            for k in range(1, min(nbase, 64)):
                bcd = IB + (u32(pbca + 4 * k) or 0)
                btd = IB + (u32(bcd) or 0)
                bases.append(cstr(btd + 0x10)[4:].rstrip("@"))
            cd_cache[cd] = bases
        vt = s["va"] + i * 8 + 8
        vtables[vt] = {
            "class": tname[4:].rstrip("@") if tname[3] in "VU" else tname,
            "col_va": v,
            "this_off": off,
            "bases": ";".join(cd_cache[cd]),
        }
print("rtti vtables", len(vtables))

# slots: consecutive pointers into executable sections
slot_map = {}
for vt in vtables:
    t = []
    for k in range(4096):
        b = rd(vt + 8 * k, 8)
        if not b:
            break
        p = struct.unpack("<Q", b)[0]
        if not is_code(p):
            break
        t.append(p)
    slot_map[vt] = t
print("slots total", sum(len(v) for v in slot_map.values()))

# ---------- 2. decompile export: function headers, DAT/LAB/switchdata refs ----------
HDR = re.compile(rb"^// ==== ([0-9a-f]+) (\S+) size=(\d+)", re.M)
REF = re.compile(rb"(DAT|LAB|switchdataD)_(14[0-9a-f]{7})")
INIT = re.compile(rb"DAT_(14[0-9a-f]{7}) = (0x[0-9a-fA-F]+|-?[0-9]*\.?[0-9]+(?:[eE][-+]?[0-9]+)?f?);")  # literal stores in decompiled code
ARR_LAB = re.compile(rb"&LAB_(14[0-9a-f]{7})\)?(\[| \+ )")  # indexed use only: (&LAB_x)[i] or &LAB_x + off
funcs = {}
dat_refs = collections.Counter()
init_lit = {}
init_n = collections.Counter()
lab_refs = collections.Counter()
sw_refs = collections.Counter()
for fn in sorted(os.listdir(src_dir)):
    if not fn.endswith(".c"):
        continue
    with open(os.path.join(src_dir, fn), "rb") as f:
        data = f.read()
    for m in INIT.finditer(data):
        a = int(m.group(1), 16)
        init_n[a] += 1
        init_lit.setdefault(a, m.group(2).decode())
    for m in HDR.finditer(data):
        funcs[int(m.group(1), 16)] = (m.group(2).decode("ascii", "replace"), int(m.group(3)))
    for m in REF.finditer(data):
        addr = int(m.group(2), 16)
        kind = m.group(1)
        if kind == b"DAT":
            dat_refs[addr] += 1
        elif kind == b"LAB":
            lab_refs[addr] += 1
        else:
            sw_refs[addr] += 1
arr_lab = set()
for fn in sorted(os.listdir(src_dir)):
    if fn.endswith(".c"):
        with open(os.path.join(src_dir, fn), "rb") as f:
            arr_lab.update(int(m.group(1), 16) for m in ARR_LAB.finditer(f.read()))
func_starts = sorted(funcs)
print("decompiled functions", len(funcs), "DAT refs", len(dat_refs), "LAB", len(lab_refs), "switchdata", len(sw_refs))


def fn_containing(va):
    i = bisect.bisect_right(func_starts, va) - 1
    if i >= 0:
        a = func_starts[i]
        if va < a + funcs[a][1]:
            return a
    return None


# ---------- 3. xrefs: lea rip-relative into a vtable (ctors install vtables this way) ----------
text = [s for s in secs if s["exec"]]
vt_set = set(vtables)
xrefs = []  # (site, vt)
LEA = re.compile(rb"[\x48-\x4f]\x8d[\x05\x0d\x15\x1d\x25\x2d\x35\x3d]", re.S)
for s in text:
    blob = RAW[s["raw_off"]: s["raw_off"] + s["raw_len"]]
    for m in LEA.finditer(blob):
        st = m.start()
        disp = struct.unpack_from("<i", blob, st + 3)[0]
        tgt = s["va"] + st + 7 + disp
        if tgt in vt_set:
            xrefs.append((s["va"] + st, tgt))
print("lea->vtable xrefs", len(xrefs))

refs_of_vt = collections.defaultdict(list)
for site, vt in xrefs:
    fa = fn_containing(site)
    refs_of_vt[vt].append(fa if fa is not None else site)

# ---------- write vtables.tsv / funcimpl.tsv / vtable_xrefs.tsv ----------
def fmt(x):
    return "" if x is None else "%x" % x


with gzip.open(os.path.join(out_dir, "vtables.tsv.gz"), "wt") as o:
    o.write("class\tvtable_va\tslot\ttarget_va\ttarget_section\tdecompiled_size\tcol_va\tthis_offset\tbases\n")
    for vt in sorted(vtables, key=lambda v: (vtables[v]["class"], v)):
        meta = vtables[vt]
        slots = slot_map[vt] or [None]
        for k, t in enumerate(slots):
            if t is None:
                tgt, sec, sz = "", "", ""
            else:
                tgt, sec = "%x" % t, sec_name(t)
                sz = funcs.get(t, ("", ""))[1] if t in funcs else "-"
            o.write("%s\t%x\t%s\t%s\t%s\t%s\t%x\t%d\t%s\n" % (
                meta["class"], vt, "" if t is None else k, tgt, sec, sz,
                meta["col_va"], meta["this_off"], meta["bases"]))

FI = [vt for vt in vtables if "_Func_impl" in vtables[vt]["class"]]
with open(os.path.join(out_dir, "funcimpl.tsv"), "w") as o:
    o.write("class\tvtable_va\tslot\ttarget_va\ttarget_section\tdecompiled_size\trefs_from_fn\n")
    for vt in sorted(FI, key=lambda v: (vtables[v]["class"], v)):
        refs = sorted(set(refs_of_vt.get(vt, [])))
        refs_s = ";".join("%x" % r for r in refs[:16])
        slots = slot_map[vt] or [None]
        for k, t in enumerate(slots):
            if t is None:
                continue
            sz = funcs[t][1] if t in funcs else "-"
            o.write("%s\t%x\t%d\t%x\t%s\t%s\t%s\n" % (
                vtables[vt]["class"], vt, k, t, sec_name(t), sz, refs_s))
print("funcimpl vtables", len(FI))

with open(os.path.join(out_dir, "vtable_xrefs.tsv"), "w") as o:
    o.write("vtable_va\tclass\tsite_or_fn_va\tfn_name\tfn_size\n")
    for site, vt in sorted(xrefs, key=lambda x: (x[1], x[0])):
        fa = fn_containing(site)
        name, size = funcs[fa] if fa is not None else ("", "")
        o.write("%x\t%s\t%x\t%s\t%s\n" % (vt, vtables[vt]["class"], fa if fa is not None else site, name, size))

# ---------- 4. constants at DAT_ addresses ----------
with open(os.path.join(out_dir, "constants.tsv"), "w") as o:
    o.write("addr\tsection\tu32\tu32_dec\tf32\tu64\tf64\trefs_in_c\tc_literal_stores\tc_first_literal_store\n")
    for a in sorted(dat_refs):
        b4 = rd(a, 4)
        b8 = rd(a, 8)
        sec = sec_name(a) if sec_at(a) else "out-of-image"
        if b4 is None:
            sec = sec + "(bss-or-unbacked)" if sec_at(a) else sec
            o.write("%x\t%s\t\t\t\t\t\t%d\t%d\t%s\n" % (a, sec, dat_refs[a], init_n[a], init_lit.get(a, "")))
            continue
        u = struct.unpack("<I", b4)[0]
        f = struct.unpack("<f", b4)[0]
        q = struct.unpack("<Q", b8)[0] if b8 else None
        d = struct.unpack("<d", b8)[0] if b8 else None
        o.write("%x\t%s\t%08x\t%d\t%.9g\t%s\t%s\t%d\t%d\t%s\n" % (
            a, sec, u, u, f, "%016x" % q if q is not None else "",
            "%r" % d if d is not None else "", dat_refs[a], init_n[a], init_lit.get(a, "")))
print("constants", len(dat_refs))

# ---------- 5. tables: named bases, LAB_ and switchdataD_ jump tables ----------
NAMED = {
    "T1": 0x144F61B10, "T2": 0x144F61B40, "T3": 0x144F61B70, "T4": 0x14A6B2600,
    "DAT_144737708": 0x144737708, "DAT_1447148b4": 0x1447148B4, "DAT_1447148a0": 0x1447148A0,
    "LAB_141aaaffc": 0x141AAAFFC,
}
TBL_WORDS = 64
tables = [(n, a) for n, a in NAMED.items()]
for a in sorted(arr_lab):
    tables.append(("LAB_%x" % a, a))
for a in sorted(sw_refs):
    tables.append(("switchdataD_%x" % a, a))
seen = set()
with open(os.path.join(out_dir, "tables.tsv"), "w") as o:
    o.write("table\tbase_va\tidx\tva\tu32\ts32\tf32\ttarget_if_image_base\ttarget_if_table_base\tsection\n")
    for name, base in tables:
        if base in seen:
            continue
        seen.add(base)
        for k in range(TBL_WORDS):
            va = base + 4 * k
            b = rd(va, 4)
            if b is None:
                break
            u = struct.unpack("<I", b)[0]
            sv = struct.unpack("<i", b)[0]
            f = struct.unpack("<f", b)[0]
            o.write("%s\t%x\t%d\t%x\t%08x\t%d\t%.9g\t%x\t%x\t%s\n" % (
                name, base, k, va, u, sv, f, IB + sv, base + sv, sec_name(va)))
print("tables", len(seen), "LAB arrays", len(arr_lab))
