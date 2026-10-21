#!/usr/bin/env python3
"""Build the searchable soothe3 pseudocode corpus index (sqlite + FTS5).

Inputs
  --binary   restored PE (same bytes the decompile corpus was built from)
  --corpus   output dir of corpus_build.py (manifest.tsv + fn/*.c)
  --sweep    UNPACER sweep index (index-default.sqlite): sites, vtables, vtable_slots

Writes  OUT/corpus.sqlite  and  OUT/annotated/<rva:08x>.c  (pseudocode with a header:
rva/va/size/section, name hint (vtable class + slot), callers/callees, string refs,
float constants).  Name hints replace `fcn.<va>` inside annotated bodies when a
vtable slot gives the function a class name.

Tables
  fn(rva, va, end_rva, size, section, owner, name_hint, status, ...)
  fn_fts  FTS5(name, body, strings, callees, classes, consts)
  call(caller, callee, site, kind)          kind: call | tail
  str_ref(fn, str_rva, site)                RIP-relative reference to a string
  strings(rva, enc, text)                   ASCII and UTF-16LE runs in non-exec sections
  const_ref(fn, site, target_rva, width, value)   float constants loaded from data
  vtable(vtable_rva, class_name, slots)
  vtable_slot(vtable_rva, slot, fn)
"""
import argparse
import bisect
import os
import re
import sqlite3
import struct
from pathlib import Path

IMAGE_BASE = 0x180000000
IMAGE_SCN_MEM_EXECUTE = 0x20000000
SCHEMA = """
DROP TABLE IF EXISTS fn; DROP TABLE IF EXISTS fn_fts; DROP TABLE IF EXISTS call;
DROP TABLE IF EXISTS str_ref; DROP TABLE IF EXISTS strings; DROP TABLE IF EXISTS const_ref;
DROP TABLE IF EXISTS vtable; DROP TABLE IF EXISTS vtable_slot; DROP TABLE IF EXISTS meta;
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE fn(rva INTEGER PRIMARY KEY, va INTEGER, end_rva INTEGER, size INTEGER,
  section TEXT, owner TEXT, name_hint TEXT, status TEXT, n_callers INTEGER, n_callees INTEGER,
  body_path TEXT);
CREATE VIRTUAL TABLE fn_fts USING fts5(name, body, strings, callees, classes, consts,
  tokenize = "unicode61 tokenchars '_:.@'");
CREATE TABLE call(caller INTEGER, callee INTEGER, site INTEGER, kind TEXT);
CREATE INDEX call_callee ON call(callee);
CREATE INDEX call_caller ON call(caller);
CREATE TABLE str_ref(fn INTEGER, str_rva INTEGER, site INTEGER);
CREATE INDEX str_ref_s ON str_ref(str_rva);
CREATE INDEX str_ref_f ON str_ref(fn);
CREATE TABLE strings(rva INTEGER PRIMARY KEY, enc TEXT, text TEXT);
CREATE INDEX strings_text ON strings(text);
CREATE TABLE const_ref(fn INTEGER, site INTEGER, target_rva INTEGER, width INTEGER, value REAL);
CREATE INDEX const_ref_v ON const_ref(value);
CREATE INDEX const_ref_f ON const_ref(fn);
CREATE TABLE vtable(vtable_rva INTEGER PRIMARY KEY, class_name TEXT, rtti TEXT, slots INTEGER);
CREATE TABLE vtable_slot(vtable_rva INTEGER, slot INTEGER, fn INTEGER);
CREATE INDEX vtable_slot_f ON vtable_slot(fn);
"""


def pe_sections(data):
    e = struct.unpack_from("<I", data, 0x3C)[0]
    nsec = struct.unpack_from("<H", data, e + 6)[0]
    optsz = struct.unpack_from("<H", data, e + 20)[0]
    so = e + 24 + optsz
    secs = []
    for i in range(nsec):
        off = so + i * 40
        name = data[off:off + 8].rstrip(b"\0").decode("latin-1")
        vsize, vaddr, rsize, rptr = struct.unpack_from("<IIII", data, off + 8)
        chars = struct.unpack_from("<I", data, off + 36)[0]
        secs.append({"name": name, "rva": vaddr, "vsize": vsize, "rptr": rptr,
                     "rsize": rsize, "exec": bool(chars & IMAGE_SCN_MEM_EXECUTE)})
    return secs


def section_of(secs, rva):
    for s in secs:
        if s["rva"] <= rva < s["rva"] + max(s["vsize"], s["rsize"]):
            return s["name"]
    return "?"


def rva_to_off(secs, rva, length=1):
    for s in secs:
        if s["rva"] <= rva and rva + length <= s["rva"] + s["rsize"]:
            return s["rptr"] + (rva - s["rva"])
    return None


def demangle_rtti(name):
    """'.?AVexception@std@@' -> 'std::exception'  (best effort; templates left raw)."""
    m = re.match(r"^\.\?A[VUW](.*)$", name)
    if not m:
        return name
    body = m.group(1)
    if body.endswith("@@"):
        body = body[:-2]
    # templates, lambdas and local classes carry mangled operators: keep them raw
    if any(c in body for c in "?$<>"):
        return body
    parts = [p for p in body.split("@") if p]
    return "::".join(reversed(parts)) if parts else name


def scan_strings(data, secs):
    """ASCII (>=4) and UTF-16LE (>=4) runs in non-executable sections, keyed by RVA."""
    out = {}
    ascii_re = re.compile(rb"[\x20-\x7e\t]{4,}")
    utf16_re = re.compile(rb"(?:[\x20-\x7e]\x00){4,}")
    for s in secs:
        if s["exec"] or s["rptr"] == 0 or s["rsize"] == 0:
            continue
        blob = data[s["rptr"]:s["rptr"] + s["rsize"]]
        for m in ascii_re.finditer(blob):
            # keep NUL-terminated runs only: avoids fragments inside larger blobs
            end = m.end()
            if end < len(blob) and blob[end] != 0:
                continue
            out[s["rva"] + m.start()] = ("ascii", m.group().decode("latin-1"))
        for m in utf16_re.finditer(blob):
            text = m.group().decode("latin-1")[::2]
            rva = s["rva"] + m.start()
            if rva not in out:
                out[rva] = ("utf16", text)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--sweep", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    corpus = Path(args.corpus)
    out = Path(args.out)
    (out / "annotated").mkdir(parents=True, exist_ok=True)
    data = Path(args.binary).read_bytes()
    secs = pe_sections(data)

    funcs = {}
    starts = []
    with open(corpus / "manifest.tsv") as f:
        next(f)
        for line in f:
            rva, end, size, section = line.rstrip("\n").split("\t")
            rva, end = int(rva, 16), int(end, 16)
            funcs[rva] = {"end": end, "size": int(size), "section": section}
            starts.append(rva)
    starts.sort()

    def containing(rva):
        i = bisect.bisect_right(starts, rva) - 1
        if i < 0:
            return None
        s = starts[i]
        return s if rva < funcs[s]["end"] else None

    strings = scan_strings(data, secs)
    str_starts = sorted(strings)

    def string_at(rva):
        return strings.get(rva)

    db_path = out / "corpus.sqlite"
    if db_path.exists():
        db_path.unlink()
    db = sqlite3.connect(db_path)
    db.executescript(SCHEMA)

    db.executemany("INSERT INTO strings VALUES (?,?,?)",
                   ((r, enc, text) for r, (enc, text) in strings.items()))

    # --- vtables (RTTI names) -------------------------------------------------
    sweep = sqlite3.connect(f"file:{args.sweep}?mode=ro", uri=True)
    vt_rows = []
    vt_names = {}
    for vrva, type_name, slots in sweep.execute(
            "SELECT vtable_rva, type_name, slots FROM vtables"):
        cls = demangle_rtti(type_name)
        vt_names[vrva] = cls
        vt_rows.append((vrva, cls, type_name, slots))
    db.executemany("INSERT INTO vtable VALUES (?,?,?,?)", vt_rows)

    slot_rows = []
    fn_class = {}  # fn rva -> [(class, slot)]
    for vrva, slot, target in sweep.execute(
            "SELECT vtable_rva, slot, target_rva FROM vtable_slots"):
        fn = target if target in funcs else containing(target)
        if fn is None:
            continue
        slot_rows.append((vrva, slot, fn))
        fn_class.setdefault(fn, []).append((vt_names.get(vrva, "?"), slot))
    db.executemany("INSERT INTO vtable_slot VALUES (?,?,?)", slot_rows)

    # --- call edges and RIP-relative references from the sweep -----------------
    calls, call_count, callee_map = [], {}, {}
    str_refs, const_rows = [], []
    seen_site = set()
    for site_rva, kind, text, target_rva in sweep.execute(
            "SELECT site_rva, kind, text, target_rva FROM sites "
            "WHERE kind IN ('direct-call','direct-jump','riprel')"):
        caller = containing(site_rva)
        if caller is None or site_rva in seen_site:
            continue
        seen_site.add(site_rva)
        if kind == "riprel":
            if target_rva is None:
                continue
            s = string_at(target_rva)
            if s is not None:
                str_refs.append((caller, target_rva, site_rva))
            # float constants: 4-byte (dword) and 8-byte (qword) loads from data
            if target_rva is not None and not secs_exec(secs, target_rva):
                # SSE scalar loads only: movss/addss/... (4 bytes) and movsd/addsd/... (8 bytes).
                # Integer or pointer loads (mov/lea/movsxd with qword ptr) are not float constants.
                mnem = text.split(" ", 1)[0]
                width = 4 if mnem.endswith("ss") else 8 if mnem.endswith("sd") else 0
                if width:
                    off = rva_to_off(secs, target_rva, width)
                    if off is not None:
                        raw = data[off:off + width]
                        val = struct.unpack("<f" if width == 4 else "<d", raw)[0]
                        # skip NaN, infinities and denormals (bit patterns that are not real constants)
                        if val == val and (val == 0 or 1e-30 < abs(val) < 1e30):
                            const_rows.append((caller, site_rva, target_rva, width, val))
            continue
        if target_rva is None or target_rva not in funcs:
            continue
        if kind == "direct-call":
            k = "call"
        elif target_rva != caller:
            k = "tail"  # direct jump to another function's entry
        else:
            continue
        calls.append((caller, target_rva, site_rva, k))
        call_count[target_rva] = call_count.get(target_rva, 0) + 1
        callee_map.setdefault(caller, set()).add(target_rva)
    db.executemany("INSERT INTO call VALUES (?,?,?,?)", calls)
    db.executemany("INSERT INTO str_ref VALUES (?,?,?)", str_refs)
    db.executemany("INSERT INTO const_ref VALUES (?,?,?,?,?)", const_rows)

    # --- per-function rows + FTS -----------------------------------------------
    str_by_fn = {}
    for fn, s_rva, _ in str_refs:
        str_by_fn.setdefault(fn, set()).add(strings[s_rva][1])
    const_by_fn = {}
    for fn, _, _, w, v in const_rows:
        const_by_fn.setdefault(fn, set()).add(f"{v:.9g}")

    fn_rows, fts_rows = [], []
    for rva in starts:
        meta = funcs[rva]
        body_file = corpus / "fn" / f"{rva:08x}.c"
        body = body_file.read_text() if body_file.exists() else ""
        status = "ok" if body else "missing"
        classes = sorted({f"{c}::v{s}" for c, s in fn_class.get(rva, [])})
        class_names = sorted({c for c, _ in fn_class.get(rva, [])})
        callees = sorted(callee_map.get(rva, set()))
        callee_names = " ".join(f"fcn.{IMAGE_BASE + c:x}" for c in callees)
        owner = ("IPP" if meta["section"] == "IPPCODE" else
                 "juce" if any(c.lower().startswith("juce") for c in class_names) else
                 "plugin?")
        hint = classes[0].replace("::v", "::slot") if classes else ""
        fn_rows.append((rva, IMAGE_BASE + rva, meta["end"], meta["size"], meta["section"],
                        owner, hint, status, call_count.get(rva, 0), len(callees),
                        f"annotated/{rva:08x}.c"))
        strs = " | ".join(sorted(str_by_fn.get(rva, set())))
        consts = " ".join(sorted(const_by_fn.get(rva, set())))
        fts_rows.append((rva, f"fcn.{IMAGE_BASE + rva:x} {hint}", body, strs, callee_names,
                         " ".join(classes), consts))
        # annotated copy: header + body with fn names replaced by class hints
        header = [
            f"// fn {IMAGE_BASE + rva:#x} (rva {rva:#x}) size {meta['size']} section {meta['section']} owner {owner}",
            f"// name-hint: {hint or '-'}   vtable: {', '.join(classes) or '-'}",
            f"// callers: {call_count.get(rva, 0)}   callees: {len(callees)}",
        ]
        if strs:
            header.append("// strings: " + " | ".join(sorted(str_by_fn.get(rva, set())))[:800])
        if consts:
            header.append("// float consts: " + consts)
        ann = body
        for c_rva in callees:
            if c_rva in fn_class:
                c_hint = sorted(fn_class[c_rva])[0]
                ann = ann.replace(f"fcn.{IMAGE_BASE + c_rva:x}",
                                  f"{c_hint[0]}::slot{c_hint[1]}_fcn{IMAGE_BASE + c_rva:x}")
        (out / "annotated" / f"{rva:08x}.c").write_text("\n".join(header) + "\n" + ann)

    db.executemany("INSERT INTO fn VALUES (?,?,?,?,?,?,?,?,?,?,?)", fn_rows)
    # rowid = function RVA, so FTS hits join straight to fn(rva)
    db.executemany("INSERT INTO fn_fts(rowid, name, body, strings, callees, classes, consts) "
                   "VALUES (?,?,?,?,?,?,?)", fts_rows)
    db.executemany("INSERT INTO meta VALUES (?,?)", [
        ("binary_sha256_note", "see RUN.md; index built from restored PE"),
        ("functions", str(len(starts))),
        ("calls", str(len(calls))),
        ("str_refs", str(len(str_refs))),
        ("const_refs", str(len(const_rows))),
        ("strings", str(len(strings))),
        ("vtables", str(len(vt_rows))),
    ])
    db.commit()
    db.close()
    print(f"functions={len(starts)} calls={len(calls)} str_refs={len(str_refs)} "
          f"const_refs={len(const_rows)} strings={len(strings)} vtables={len(vt_rows)}")


def secs_exec(secs, rva):  # True when rva lies in an executable section
    for s in secs:
        if s["rva"] <= rva < s["rva"] + max(s["vsize"], s["rsize"]):
            return s["exec"]
    return False


if __name__ == "__main__":
    main()
