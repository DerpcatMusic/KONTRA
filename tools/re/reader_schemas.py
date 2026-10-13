#!/usr/bin/env python3
"""Per-version stream layouts of Kontakt 8.13.1's serializable readers.

    tools/re/fetch_helpers.py docs/re/kontakt-catalog/ser_classes.tsv decompiled.c
    tools/re/reader_schemas.py docs/re/kontakt-catalog/ser_classes.tsv decompiled.c \
        > docs/re/kontakt-catalog/reader_schemas.tsv

Each reader is lowered whole (reader_lower.py: every stored version, validation throw
and data-dependent branch survives) and executed by the reference interpreter
(ir_sim.py) once per candidate stored version. A version is supported when the reader
accepts it and reads from the stream; its layout is the read sequence of a stream the
reader itself accepts. Nothing is matched by pattern, so no branch form is "remaining".

Columns: class, sertype, native_version, stored_version, layout, status, reader_va.
  layout   F f32, B u8, H u16, I u32, Q u64, S string, W wide string, O nested object,
           R raw bytes; `x7` repeats; `{..}` marks a data-dependent group (loop body)
  status   typed          every read is fixed by the stored version and earlier reads
           host-branch    also depends on host state (instrument tables, name registry)
           nested         embeds objects whose class this reader does not name
           virtual:...    a nested object is read through Kontakt's virtual dispatch
           stub           the reader slot is Kontakt's no-op stub: nothing is serialized
"""
import csv
import re
import sys
from collections import OrderedDict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import ir_sim  # noqa: E402
import reader_lower as rl  # noqa: E402

# Arrays name the class of their elements only in the constructor; tags in the stream agree.
ELEMENT_CLASS = {0x3A: 0x25, 0x3B: 0x0D, 0x3C: 0x0C, 0x02: 0x25}
STUB_SLOTS = {"FUN_14050a460", "_guard_check_icall", "FUN_1404f99a0"}


def load(classes_path, decompiled_path):
    rows = list(csv.DictReader(open(classes_path), delimiter="\t"))
    funcs = rl.load_functions([decompiled_path])
    progs = rl.lower_all(funcs, {"FUN_" + r["reader_va"][2:] for r in rows})
    readers = {}
    for r in rows:
        readers.setdefault(int(r["sertype"], 16), "FUN_" + r["reader_va"][2:])
    return rows, progs, readers


def reachable_callx(progs, name, seen=None):
    """Reasons of every `callx` reachable from a program (static, over callees)."""
    seen = seen if seen is not None else set()
    if name in seen or name not in progs:
        return set()
    seen.add(name)
    out = set()
    for op in progs[name]["ops"]:
        m = re.match(r"\(callx (\S+)\)", op)
        if m:
            out.add(m.group(1))
        m = re.match(r"\(call (\w+) ", op)
        if m:
            out |= reachable_callx(progs, m.group(1), seen)
    return out


def render(reads):
    """Compact layout string from read records."""
    seq = []
    for kind, _, n, _ in reads:
        k = kind if kind in "FBHIQSW" else ("O" if kind.startswith("O") else "R")
        seq.append(k)
    out, i = [], 0
    while i < len(seq):
        j = i
        while j < len(seq) and seq[j] == seq[i]:
            j += 1
        out.append(seq[i] if j - i < 4 else f"{seq[i]}x{j - i}")
        i = j
    return "".join(out)


def analyze(rows, progs, readers):
    """-> {reader_name: {versions: {v: info}, reasons, stub}}"""
    out = OrderedDict()
    for r in rows:
        name = "FUN_" + r["reader_va"][2:]
        if name in out:
            continue
        sertype, native = int(r["sertype"], 16), int(r["version"], 16)
        info = {"versions": OrderedDict(), "static": set(), "stub": False}
        out[name] = info
        if name not in progs or name in STUB_SLOTS:
            info["stub"] = True
            continue
        if len(progs[name]["params"]) < 3:  # signature is not read(obj, stream, version, ctx)
            info["stub"] = True
            info["not_reader"] = True
            continue
        info["static"] = reachable_callx(progs, name)
        cls = ELEMENT_CLASS.get(sertype)
        top = min(native + 2, 0x2800)
        for v in range(0, top + 1) if native < 0x400 else [native]:
            st, s0, m0, _ = ir_sim.search(progs, readers, name, v, None, cls=cls, budget=1500, need_reads=True)
            if st != "ok" or not s0.reads:
                continue
            kept = ir_sim.explore(progs, readers, name, v, cls)
            runs = [(s0, m0)] + [(k[1], k[2]) for k in kept]
            values = sorted(set(ir_sim.SYNTH_VALUES) | set(ir_sim.constants(progs, [name])))
            host = any(ir_sim.host_dependent(progs, readers, name, v, values, cls, (k[1], k[3])) for k in kept[:6])
            longest = max(runs, key=lambda r: len(r[0].reads))[0]
            hits = sorted({h for _, m in runs for h in m.undef_hits})
            exprs = sorted({str(e) for _, m in runs for e in m.undef_exprs})
            status = "typed"
            if host:
                status = "host-branch"
            if any(k.startswith("O") and k != "O" for r, _ in runs for k, *_ in r.reads):
                status = "nested"
            if any(x.startswith(("virtual", "stream-vtable", "unread-helper")) for x in info["static"]):
                status = "virtual:" + ",".join(sorted(info["static"]))
            samples, seen = [], set()
            for r, _ in sorted(runs, key=lambda r: len(r[0].out)):
                key = render(r.reads)
                if key not in seen and len(r.out) <= 400 and len(samples) < 3:
                    seen.add(key)
                    samples.append((key, bytes(r.out), "".join(k[0] if k[0] in "FBHIQSWR" else "O" for k, *_ in r.reads)))
            info["versions"][v] = {"layout": render(longest.reads), "status": status, "host": hits,
                                   "exprs": exprs[:6], "paths": len({render(r.reads) for r, _ in runs}),
                                   "samples": samples}
    return out


def main():
    rows, progs, readers = load(sys.argv[1], sys.argv[2])
    res = analyze(rows, progs, readers)
    print("class\tsertype\tnative_version\tstored_version\tlayout\tstatus\treader_va")
    for r in rows:
        name = "FUN_" + r["reader_va"][2:]
        info = res[name]
        head = f"{r['class']}\t{r['sertype']}\t{r['version']}"
        if info["stub"] or not info["versions"]:
            why = ("not-a-reader" if info.get("not_reader") else "stub") if info["stub"] else "no-version-accepted"
            print(f"{head}\t-\t\t{why}\t{r['reader_va']}")
            continue
        for v, i in info["versions"].items():
            print(f"{head}\t0x{v:X}\t{i['layout']}\t{i['status']}\t{r['reader_va']}")


if __name__ == "__main__":
    main()
