#!/usr/bin/env python3
"""Score engines against Kontakt from an identify-kontakt id.json. Usage: probe_report.py ID ID.json OUT.json
Primary sample of a note = best match with NCC >= 0.5 (others kept in the JSON). Reports per engine vs kontakt:
same sample %, same velocity layer %, RR sequence match per (key, vel) group of 4, level difference, articulations."""
import json, statistics, sys
from collections import defaultdict

ID, src, out = sys.argv[1:4]
d = json.load(open(src))
THRESH, SILENT = 0.5, -80.0


def primary(e):
    m = [x for x in e["matches"] if x["ncc"] >= THRESH]
    return m[0] if m else None


def layer(m):  # the velocity ranges of the zones that carry this sample at this key
    return tuple(sorted({tuple(z["vel"]) for z in m["zones"]}))


def artic(m):
    return sorted({z["artic"] or "-" for z in m["zones"]})


notes = d["notes"]
engines = [e for e in notes[0]["engines"] if e != "kontakt"]
res = {"instrument": ID, "name": d["instrument"], "notes": len(notes), "engines": {}}
kont = [n["engines"].get("kontakt") for n in notes]
if not all(kont):
    sys.exit("no kontakt render in this id.json")
kp = [primary(k) for k in kont]
res["kontakt"] = {
    "identified": sum(p is not None for p in kp),
    "silent": sum(k["rms_db"] < SILENT for k in kont),
    "distinct_samples": len({p["asset"] for p in kp if p}),
    "articulations": sorted({a for p in kp if p for a in artic(p)}),
}
groups = defaultdict(list)
for i, n in enumerate(notes):
    groups[(n["key"], n["vel"])].append(i)
for eng in engines:
    ep = [primary(n["engines"][eng]) for n in notes]
    er = [n["engines"][eng]["rms_db"] for n in notes]
    both = [i for i in range(len(notes)) if kp[i] and ep[i]]
    same = [i for i in both if kp[i]["asset"] == ep[i]["asset"]]
    lay = [i for i in both if layer(kp[i]) == layer(ep[i])]
    rr_exact = rr_set = rr_n = 0
    rr_detail = []
    for g, idx in sorted(groups.items()):
        ks = [kp[i]["asset"] if kp[i] else None for i in idx]
        es = [ep[i]["asset"] if ep[i] else None for i in idx]
        if None in ks:
            continue
        rr_n += 1
        rr_exact += ks == es
        rr_set += set(ks) == set(es)
        rr_detail.append({"key": g[0], "vel": g[1], "kontakt_distinct": len(set(ks)), "engine_distinct": len(set(es)), "sequence_equal": ks == es})
    diffs = [er[i] - kont[i]["rms_db"] for i in range(len(notes)) if kont[i]["rms_db"] > SILENT and er[i] > SILENT]
    res["engines"][eng] = {
        "identified": sum(p is not None for p in ep),
        "silent_notes": sum(r < SILENT for r in er),
        "kontakt_audible_engine_silent": sum(er[i] < SILENT and kont[i]["rms_db"] > SILENT for i in range(len(notes))),
        "same_sample_pct": round(100 * len(same) / max(1, len(kp) - kp.count(None)), 1),
        "same_velocity_layer_pct": round(100 * len(lay) / max(1, len(kp) - kp.count(None)), 1),
        "rr_groups": rr_n, "rr_sequence_equal": rr_exact, "rr_same_set": rr_set,
        "level_db_mean": round(statistics.mean(diffs), 2) if diffs else None,
        "level_db_worst": round(max(diffs, key=abs), 2) if diffs else None,
        "articulations": sorted({a for p in ep if p for a in artic(p)}),
        "rr_detail": rr_detail,
        "per_note": [{"i": i, "key": notes[i]["key"], "vel": notes[i]["vel"],
                      "kontakt": kp[i] and {"asset": kp[i]["asset"], "ncc": kp[i]["ncc"], "layer": layer(kp[i]), "rms_db": kont[i]["rms_db"]},
                      "engine": ep[i] and {"asset": ep[i]["asset"], "ncc": ep[i]["ncc"], "layer": layer(ep[i]), "rms_db": er[i]}}
                     for i in range(len(notes)) if not kp[i] or not ep[i] or kp[i]["asset"] != ep[i]["asset"]],
    }
json.dump(res, open(out, "w"), indent=1)
for eng, r in res["engines"].items():
    print(f"{ID:18} {eng:6} sample {r['same_sample_pct']:5.1f}%  layer {r['same_velocity_layer_pct']:5.1f}%  RR {r['rr_sequence_equal']}/{r['rr_groups']}  level {r['level_db_mean']} dB (worst {r['level_db_worst']})  silent-vs-kontakt {r['kontakt_audible_engine_silent']}  artic {','.join(r['articulations'])[:40]}")
