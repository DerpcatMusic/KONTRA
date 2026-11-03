#!/usr/bin/env python3
"""Aggregate the KSP symbol surface over the census-loaded instrument set.

Reads $CENSUS_WORK/manifest.jsonl + $CENSUS_WORK/scripts/<i>/slot-*.ksp and uses the
project's own scanner (tools/re/ksp_symbols.py: Resolver, scan_script, classify) so the
statuses are exactly what `ksp_symbols.py doctor` would report.

Writes $CENSUS_WORK/symbols.json:
  {"instruments":[{i,path,library,name,slots,symbols:[{symbol,count,status,code,category,conformance,corpus_rank,owner}]}],
   "by_symbol":[...], "by_library":[...]}
"""
import collections, json, sys
from pathlib import Path
import os

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


#: The repository holding `tools/re/ksp_symbols.py`: this file is `tools/census/`.
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/re"))
import ksp_symbols as K

M = Path(WORK) / "manifest.jsonl"
S = Path(WORK) / "scripts"


def library_of(path):
    p = Path(path)
    parent = p.parent
    if parent.name.lower() == "instruments":
        return parent.parent.name
    # <Lib>/Instruments/<sub>/<sub>/X.nki -> still the library root above Instruments
    for a in p.parents:
        if a.name.lower() == "instruments" and a.parent.name:
            return a.parent.name
    return parent.name or p.stem


def main():
    resolve = K.Resolver(K.load_rows())
    rows = [json.loads(l) for l in M.read_text().splitlines() if l.strip()]
    k, n = (int(sys.argv[1]), int(sys.argv[2])) if len(sys.argv) > 2 else (0, 1)
    rows = [r for j, r in enumerate(rows) if j % n == k]
    instruments, by_symbol = [], {}
    for r in rows:
        if not r["ok"]:
            continue
        hits = collections.Counter()
        for f in sorted((S / str(r["i"])).glob("*.ksp")):
            hits.update(K.scan_script(f.read_text(errors="replace"), resolve))
        lib = library_of(r["path"])
        syms = [K.symbol_report(k, n, resolve(k)) for k, n in hits.items()]
        syms.sort(key=lambda s: (-s["count"], s["symbol"]))
        instruments.append({"i": r["i"], "path": r["path"], "name": r.get("name"),
                            "library": lib, "slots": r["slots"], "symbols": syms})
        for s in syms:
            e = by_symbol.setdefault(s["symbol"], {**{k: s[k] for k in
                             ("symbol", "status", "code", "category", "conformance", "corpus_rank", "owner")},
                             "count": 0, "instruments": 0, "libraries": set(), "examples": []})
            e["count"] += s["count"]
            e["instruments"] += 1
            e["libraries"].add(lib)
            if len(e["examples"]) < 6:
                e["examples"].append(r["path"])
    sym_out = []
    for e in by_symbol.values():
        e = dict(e)
        e["libraries"] = sorted(e["libraries"])
        e["library_count"] = len(e["libraries"])
        sym_out.append(e)
    sym_out.sort(key=lambda e: (-e["instruments"], -e["count"], e["symbol"]))

    libs = collections.defaultdict(lambda: {"instruments": 0, "scriptless": 0, "symbols": set(),
                                            "unsupported": set(), "stub": set(), "approx": set(),
                                            "unverified": set(), "calls": 0})
    for ins in instruments:
        a = libs[ins["library"]]
        a["instruments"] += 1
        if not ins["symbols"]:
            a["scriptless"] += 1
        for s in ins["symbols"]:
            a["symbols"].add(s["symbol"])
            a["calls"] += s["count"]
            if s["code"] in ("KSP-UNSUPPORTED", "KSP-UNKNOWN"):
                a["unsupported"].add(s["symbol"])
            elif s["status"] == "stub":
                a["stub"].add(s["symbol"])
            elif s["conformance"] == "tested":
                a["approx"].add(s["symbol"])
            else:
                a["unverified"].add(s["symbol"])
    lib_out = []
    for name, a in libs.items():
        lib_out.append({"library": name, "instruments": a["instruments"], "scriptless": a["scriptless"],
                        "symbols": len(a["symbols"]), "unsupported": sorted(a["unsupported"]),
                        "stub": sorted(a["stub"]), "approx": len(a["approx"]),
                        "unverified": len(a["unverified"]), "calls": a["calls"]})
    lib_out.sort(key=lambda l: -l["instruments"])

    out = str(Path(WORK) / (f"symbols.{k}.json" if n > 1 else "symbols.json"))
    json.dump({"instruments": instruments, "by_symbol": sym_out, "by_library": lib_out}, open(out, "w"))
    print(f"instruments scanned: {len(instruments)}; symbols seen: {len(sym_out)}; libraries: {len(lib_out)}")
    for s in sym_out[:25]:
        print(f"{s['instruments']:5d} instr {s['count']:8d} calls {s['library_count']:3d} lib  "
              f"{s['status']:<11} {s['symbol']}")


if __name__ == "__main__":
    main()
