#!/usr/bin/env python3
"""Merge $CENSUS_WORK/symbols.<k>.json shards into one ranked file, filtered to real
gaps (ledger Unsupported / Stub / no binding), keeping the full surface for reference."""
import collections, json
import os

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


N = 8
inst, modes = [], {}
for k in range(N):
    d = json.load(open(f"{WORK}/symbols.{k}.json"))
    inst.extend(d["instruments"])
inst.sort(key=lambda i: i["i"])

# per-symbol join, with instruments that use it (rebuilt from per-instrument rows)
sym = {}
for i in inst:
    for s in i["symbols"]:
        e = sym.setdefault(s["symbol"], {**{x: s[x] for x in ("symbol", "status", "code", "category",
                                                             "conformance", "corpus_rank", "owner")},
                                         "count": 0, "instruments": 0,
                                         "libraries": collections.Counter(), "examples": []})
        e["count"] += s["count"]
        e["instruments"] += 1
        e["libraries"][i["library"]] += 1
        if len(e["examples"]) < 8:
            e["examples"].append({"library": i["library"], "name": i["name"], "path": i["path"]})

for e in sym.values():
    e["library_count"] = len(e["libraries"])
    e["libraries"] = dict(e["libraries"].most_common())

buckets = collections.defaultdict(list)
for e in sym.values():
    if e["code"] in ("KSP-UNSUPPORTED", "KSP-UNKNOWN"):
        buckets["unsupported"].append(e)
    elif e["status"] == "stub":
        buckets["stub"].append(e)
    elif e["conformance"] == "tested":
        buckets["approximate"].append(e)
    else:
        buckets["unverified"].append(e)
for b in buckets:
    buckets[b].sort(key=lambda e: (-e["instruments"], -e["count"], e["symbol"]))

libs = collections.defaultdict(lambda: {"instruments": 0, "scriptless": 0, "calls": 0,
                                        "unsupported": collections.Counter(), "stub": collections.Counter(),
                                        "approximate": set(), "unverified": set()})
for i in inst:
    L = libs[i["library"]]
    L["instruments"] += 1
    if not i["symbols"]:
        L["scriptless"] += 1
    for s in i["symbols"]:
        L["calls"] += s["count"]
        if s["code"] in ("KSP-UNSUPPORTED", "KSP-UNKNOWN"):
            L["unsupported"][s["symbol"]] += 1
        elif s["status"] == "stub":
            L["stub"][s["symbol"]] += 1
        elif s["conformance"] == "tested":
            L["approximate"].add(s["symbol"])
        else:
            L["unverified"].add(s["symbol"])

lib_out = []
for name, L in libs.items():
    lib_out.append({"library": name, "instruments": L["instruments"], "scriptless": L["scriptless"],
                    "calls": L["calls"],
                    "unsupported_calls": sum(L["unsupported"].values()),
                    "stub_calls": sum(L["stub"].values()),
                    "unsupported": dict(L["unsupported"].most_common()),
                    "stub": dict(L["stub"].most_common()),
                    "approximate": len(L["approximate"]), "unverified": len(L["unverified"])})
lib_out.sort(key=lambda l: (-l["instruments"], l["library"]))

json.dump({"instruments": inst, "buckets": buckets, "by_library": lib_out,
           "counts": {b: len(v) for b, v in buckets.items()}}, open(f"{WORK}/symbols.json", "w"))
print("instruments", len(inst), "symbols", len(sym), {b: len(v) for b, v in buckets.items()})
for b in ("unsupported", "stub"):
    print(f"--- {b}")
    for e in buckets[b][:20]:
        print(f"{e['instruments']:5d} instr {e['count']:7d} calls {e['library_count']:3d} lib  {e['symbol']}")
