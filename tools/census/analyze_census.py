#!/usr/bin/env python3
"""Census-level gap counts: unanswered init actions, refusals, UI assets,
zero-playable-zone and silent instruments, per library.

Reads $CENSUS_WORK/census.json and $CENSUS_WORK/keysweep-results.jsonl.
Writes $CENSUS_WORK/census_summary.json.
"""
import collections, json, re, statistics
import os

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


RECS = json.load(open(os.path.join(WORK, "census.json")))
SWEEP = os.path.join(WORK, "keysweep-results.jsonl")

#: The keys `keysweep.py` renders. A record counts as a measurement of silence
#: only when every one of them answered: `keysweep.py` omits a key whose render
#: failed or timed out, and "no peak" used to read as "peak 0", which turned our
#: own failure into the instrument's silence. The probe's own `error` field says
#: why a key is missing.
KEYS = (36, 43, 48, 55, 60, 64, 67, 72, 84)


def keys_answered(s):
    """How many of the nine probed keys produced a render for this instrument."""
    peaks = s.get("peaks") or {}
    return sum(1 for k in KEYS if str(k) in peaks)



def library_of(path):
    """The product folder: the directory above the nearest `Instruments` ancestor."""
    parts = path.split("/")
    for i in range(len(parts) - 1, 0, -1):
        if parts[i].lower() == "instruments":
            return parts[i - 1]
    return parts[-2] if len(parts) > 1 else "?"


def actions(rec):
    """[(action, count)] from the `init unanswered: [...]` field."""
    raw = rec.get("init_unanswered_raw") or ""
    return [(m[0], int(m[1])) for m in re.findall(r"\(([A-Za-z_0-9]+), (\d+)\)", raw)]


def main():
    loaded = [r for r in RECS if r.get("loaded")]
    refused = [r for r in RECS if not r.get("loaded")]

    # ---- refusals by cause
    def cause(r):
        t = (r.get("refusal") or "") + (r.get("refusal_head") or "")
        if "no .nicnt access data" in t:
            return "library access: no .nicnt present"
        if "legacy xml" in t.lower():
            return "legacy XML instrument (pre-Kontakt 4 format)"
        if "modulator" in t.lower():
            return "modulator category 3"
        if "program: missing" in t:
            return "program: missing"
        return "other"

    by_cause = collections.defaultdict(lambda: {"instruments": 0, "libraries": collections.Counter()})
    for r in refused:
        c = cause(r)
        by_cause[c]["instruments"] += 1
        by_cause[c]["libraries"][library_of(r["path"])] += 1

    # ---- unanswered actions
    act = collections.defaultdict(lambda: {"calls": 0, "instruments": 0, "libraries": set(), "examples": []})
    for r in loaded:
        for name, n in actions(r):
            e = act[name]
            e["calls"] += n
            e["instruments"] += 1
            e["libraries"].add(library_of(r["path"]))
            if len(e["examples"]) < 5:
                e["examples"].append(r["path"])
    act_out = sorted(({"action": k, "calls": v["calls"], "instruments": v["instruments"],
                       "libraries": sorted(v["libraries"]), "library_count": len(v["libraries"]),
                       "examples": v["examples"]} for k, v in act.items()),
                     key=lambda e: -e["instruments"])

    # ---- instruments with no unanswered action at all
    clean = sum(1 for r in loaded if not actions(r))

    # ---- zero playable / silent
    nz = [r for r in loaded if (r.get("zones_ready") or 0) == 0]
    sweep = [json.loads(l) for l in open(SWEEP) if l.strip()]
    # An instrument is silent only when it answered every key and none of them
    # sounded. A record that stayed quiet at key 60 and then lost renders is
    # *unmeasured*, and is counted separately rather than as silence.
    quiet = [s for s in sweep if s.get("zones_ready", 0) > 0 and not s.get("sounds_at")]
    silent = [s for s in quiet if keys_answered(s) == len(KEYS)]
    unmeasured = [s for s in quiet if keys_answered(s) < len(KEYS)]
    silent_paths = {s["path"] for s in silent}

    # ---- UI
    ui_unres = [r for r in loaded if r.get("ui_unresolved", 0)]
    per_lib_ui = collections.Counter()
    per_lib_ui_instr = collections.Counter()
    for r in loaded:
        if r.get("ui_unresolved", 0):
            per_lib_ui[library_of(r["path"])] += r["ui_unresolved"]
            per_lib_ui_instr[library_of(r["path"])] += 1

    libs = collections.defaultdict(lambda: {"instruments": 0, "loaded": 0, "refused": 0,
                                            "no_zones": 0, "silent": 0, "unanswered": 0,
                                            "ui_instruments": 0, "ui_assets": 0})
    for r in RECS:
        L = libs[library_of(r["path"])]
        L["instruments"] += 1
        if r.get("loaded"):
            L["loaded"] += 1
            if (r.get("zones_ready") or 0) == 0:
                L["no_zones"] += 1
            if actions(r):
                L["unanswered"] += 1
            if r.get("ui_unresolved", 0):
                L["ui_instruments"] += 1
                L["ui_assets"] += r["ui_unresolved"]
            if r["path"] in silent_paths:
                L["silent"] += 1
        else:
            L["refused"] += 1

    out = {
        "totals": {"records": len(RECS), "loaded": len(loaded), "refused": len(refused),
                   "loaded_no_unanswered": clean, "loaded_no_zones": len(nz),
                   "keysweep_instruments": len(sweep), "silent_with_zones": len(silent),
                   "silent_unmeasured": len(unmeasured),
                   "keysweep_errors": sum(1 for s in sweep if s.get("error")),
                   "ui_instruments": len(ui_unres),
                   "ui_unresolved_assets": sum(r.get("ui_unresolved", 0) for r in loaded),
                   "ui_resolved_assets": sum(r.get("ui_resolved", 0) for r in loaded)},
        "actions": act_out,
        "refusals": {k: {"instruments": v["instruments"], "libraries": dict(v["libraries"].most_common())}
                     for k, v in by_cause.items()},
        "no_zones": [{"path": r["path"], "name": r["name"], "library": library_of(r["path"]),
                      "zones_total": r.get("zones_total"), "samples": r.get("samples"),
                      "scripts": r.get("scripts")} for r in nz],
        "silent": [{"path": s["path"], "name": s.get("name"), "library": library_of(s["path"]),
                    "zones_ready": s.get("zones_ready"), "best_peak": s.get("best_peak"),
                    "keys_answered": keys_answered(s), "error": s.get("error")} for s in silent],
        "libraries": sorted(({"library": k, **v} for k, v in libs.items()),
                            key=lambda l: -l["instruments"]),
        "ui_by_library": [{"library": k, "assets": v, "instruments": per_lib_ui_instr[k]}
                          for k, v in per_lib_ui.most_common()],
    }
    json.dump(out, open(os.path.join(WORK, "census_summary.json"), "w"), indent=1)
    t = out["totals"]
    print(json.dumps(t, indent=1))
    print("--- actions")
    for a in act_out[:14]:
        print(f"{a['instruments']:5d} instr {a['calls']:10d} calls {a['library_count']:3d} lib  {a['action']}")
    print("--- refusals")
    for k, v in out["refusals"].items():
        print(v["instruments"], k)
    print("--- zero zones by library")
    print(collections.Counter(r["library"] for r in out["no_zones"]).most_common(10))
    print("--- silent by library")
    print(collections.Counter(r["library"] for r in out["silent"]).most_common(10))
    print(f"--- silence measurement: {len(silent)} instruments answered all {len(KEYS)} keys and never sounded; "
          f"{len(unmeasured)} stayed quiet but answered fewer, so they are not counted as silent")


if __name__ == "__main__":
    main()
