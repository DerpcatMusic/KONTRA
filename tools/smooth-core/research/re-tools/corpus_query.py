#!/usr/bin/env python3
"""Query the soothe3 pseudocode corpus (corpus.sqlite from corpus_index.py).

  corpus_query.py [--db F] [--limit N] by-string 'text'   functions that reference a string (substring, case-insensitive)
  corpus_query.py by-const 0.7071 [--tol 1e-4]  functions that load a float constant near VALUE
  corpus_query.py callers 0x180002300          direct callers (and tail jumps) of a function
  corpus_query.py callees 0x180002300          direct callees of a function
  corpus_query.py vtable ClassName             RTTI class (substring) and its vtable slots -> functions
  corpus_query.py fn 0x180002300               print the annotated pseudocode of one function
  corpus_query.py search 'FFT AND scale'       FTS5 full-text search over names, bodies, strings, consts
  corpus_query.py string-at 0x...              the string stored at an RVA/VA

Addresses accept VA (>= 0x180000000) or RVA. Output is plain text; pipe through `head` if needed.
Index location: --db or $SMOOTH_CORPUS_DB (default: RunScratch/smooth/re-corpus/index/corpus.sqlite).
"""
import argparse
import os
import sqlite3
import sys
from pathlib import Path

IMAGE_BASE = 0x180000000
DEFAULT_DB = Path("/data/dev/workspace/RunScratch/smooth/re-corpus/index/corpus.sqlite")
DEFAULT_CORPUS = Path("/data/dev/workspace/RunScratch/smooth/re-corpus/index")


def to_rva(text):
    v = int(text, 0)
    return v - IMAGE_BASE if v >= IMAGE_BASE else v


def fmt_fn(row):
    rva, va, size, section, owner, hint, callers, callees = row
    return (f"fn {va:#x} rva {rva:#x} size {size:<6} {section:<8} {owner:<8} "
            f"callers={callers:<4} callees={callees:<4} {hint or ''}")


FN_COLS = "fn.rva, fn.va, fn.size, fn.section, fn.owner, fn.name_hint, fn.n_callers, fn.n_callees"


def cmd_by_string(db, args):
    pat = f"%{args.text}%"
    rows = db.execute(
        f"SELECT s.rva, s.text, {FN_COLS} FROM strings s JOIN str_ref r ON r.str_rva = s.rva "
        f"JOIN fn ON fn.rva = r.fn WHERE s.text LIKE ? COLLATE NOCASE "
        f"GROUP BY s.rva, fn.rva ORDER BY s.rva, fn.rva LIMIT ?", (pat, args.limit)).fetchall()
    if not rows:
        print("no function references a string matching", repr(args.text))
        strs = db.execute("SELECT rva, enc, text FROM strings WHERE text LIKE ? COLLATE NOCASE "
                          "LIMIT 20", (pat,)).fetchall()
        for rva, enc, text in strs:
            print(f"  (unreferenced string) {rva:#x} {enc} {text[:120]!r}")
        return
    for srva, text, *fn in rows:
        print(f"string {srva:#x} {text[:100]!r}")
        print("   ", fmt_fn(fn))


def cmd_by_const(db, args):
    v = args.value
    rows = db.execute(
        "SELECT c.value, c.width, c.site, c.target_rva, " + FN_COLS + " FROM const_ref c "
        "JOIN fn ON fn.rva = c.fn WHERE ABS(c.value - ?) <= ? "
        "ORDER BY ABS(c.value - ?), fn.rva LIMIT ?", (v, args.tol, v, args.limit)).fetchall()
    if not rows:
        print(f"no float constant within {args.tol} of {v}")
        return
    for val, width, site, trva, *fn in rows:
        print(f"const {val!r} f{width * 8} site {site:#x} data {trva:#x}")
        print("   ", fmt_fn(fn))


def cmd_callers(db, args):
    rva = to_rva(args.addr)
    rows = db.execute(
        "SELECT c.kind, c.site, " + FN_COLS + " FROM call c JOIN fn ON fn.rva = c.caller "
        "WHERE c.callee = ? ORDER BY fn.rva LIMIT ?", (rva, args.limit)).fetchall()
    print(f"callers of {IMAGE_BASE + rva:#x}: {len(rows)}")
    for kind, site, *fn in rows:
        print(f"  {kind:<4} at {site:#x}  ", fmt_fn(fn))


def cmd_callees(db, args):
    rva = to_rva(args.addr)
    rows = db.execute(
        "SELECT c.kind, c.site, c.callee, " + FN_COLS.replace("fn.", "f2.") +
        " FROM call c JOIN fn f2 ON f2.rva = c.callee WHERE c.caller = ? "
        "ORDER BY c.site LIMIT ?", (rva, args.limit)).fetchall()
    print(f"callees of {IMAGE_BASE + rva:#x}: {len(rows)}")
    for kind, site, _, *fn in rows:
        print(f"  {kind:<4} at {site:#x}  ", fmt_fn(fn))


def cmd_vtable(db, args):
    classes = db.execute("SELECT vtable_rva, class_name, slots FROM vtable "
                         "WHERE class_name LIKE ? COLLATE NOCASE ORDER BY slots DESC LIMIT 20",
                         (f"%{args.name}%",)).fetchall()
    if not classes:
        print("no RTTI class matches", repr(args.name))
    for vrva, cls, slots in classes:
        print(f"vtable {vrva:#x}  {cls}  slots={slots}")
        for slot, fn, va, size, hint in db.execute(
                "SELECT s.slot, fn.rva, fn.va, fn.size, fn.name_hint FROM vtable_slot s "
                "JOIN fn ON fn.rva = s.fn WHERE s.vtable_rva = ? ORDER BY s.slot", (vrva,)):
            if args.slot is None or slot == args.slot:
                print(f"  [{slot:>3}] {va:#x} size={size}")


def cmd_fn(db, args):
    rva = to_rva(args.addr)
    row = db.execute("SELECT body_path FROM fn WHERE rva = ?", (rva,)).fetchone()
    if not row:
        sys.exit(f"no function at {args.addr}")
    corpus = Path(os.environ.get("SMOOTH_CORPUS_DIR", DEFAULT_CORPUS))
    print((corpus / row[0]).read_text())


def cmd_search(db, args):
    rows = db.execute(
        "SELECT fn.rva, fn.va, fn.size, fn.section, fn.owner, fn.name_hint, fn.n_callers, fn.n_callees, "
        "snippet(fn_fts, 1, '[', ']', ' ... ', 12) FROM fn_fts JOIN fn ON fn.rva = fn_fts.rowid "
        "WHERE fn_fts MATCH ? ORDER BY rank LIMIT ?", (args.query, args.limit)).fetchall()
    for rva, va, size, section, owner, hint, nc, ne, snip in rows:
        print(fmt_fn((rva, va, size, section, owner, hint, nc, ne)))
        print("    " + snip.replace("\n", " ")[:220])


def cmd_string_at(db, args):
    rva = to_rva(args.addr)
    row = db.execute("SELECT enc, text FROM strings WHERE rva = ?", (rva,)).fetchone()
    print(f"{rva + IMAGE_BASE:#x}: {row[0]} {row[1]!r}" if row else "no string starts there")


def main():
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--db", default=os.environ.get("SMOOTH_CORPUS_DB", str(DEFAULT_DB)))
    common.add_argument("--limit", type=int, default=50)
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    def add(name, **kw):
        return sub.add_parser(name, parents=[common], **kw)

    p = add("by-string"); p.add_argument("text"); p.set_defaults(fn=cmd_by_string)
    p = add("by-const"); p.add_argument("value", type=float)
    p.add_argument("--tol", type=float, default=1e-4); p.set_defaults(fn=cmd_by_const)
    p = add("callers"); p.add_argument("addr"); p.set_defaults(fn=cmd_callers)
    p = add("callees"); p.add_argument("addr"); p.set_defaults(fn=cmd_callees)
    p = add("vtable"); p.add_argument("name")
    p.add_argument("--slot", type=int); p.set_defaults(fn=cmd_vtable)
    p = add("fn"); p.add_argument("addr"); p.set_defaults(fn=cmd_fn)
    p = add("search"); p.add_argument("query"); p.set_defaults(fn=cmd_search)
    p = add("string-at"); p.add_argument("addr"); p.set_defaults(fn=cmd_string_at)
    args = ap.parse_args()
    db = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    args.fn(db, args)


if __name__ == "__main__":
    main()
