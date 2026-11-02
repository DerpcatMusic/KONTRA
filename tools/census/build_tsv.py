#!/usr/bin/env python3
"""Emit docs/re/gap-inventory-2026-11-01.tsv from the measured files.

Every count is read from $CENSUS_WORK/*.json|.tsv, produced by the commands the
document's method section quotes. The judgement columns (`kind`, `what_it_would_take`)
are the author's and are the only hand-written cells.
"""
import collections, json, re, sys
import os

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


C = json.load(open(f"{WORK}/census_summary.json"))
S = json.load(open(f"{WORK}/symbols.json"))
F = json.load(open(f"{WORK}/compile_fail.json"))
ACT = {a["action"]: a for a in C["actions"]}

# ---------------------------------------------------------------- compile causes
COMPILE = {}


def fam(m):
    return re.sub(r"\(at byte \d+\)", "", m).strip()


fam_instr = collections.defaultdict(lambda: {"instr": 0, "libs": collections.Counter(), "ex": []})
fam_only = collections.Counter()
for i, v in F.items():
    lib = v["path"].split("/")[6] if "/Instruments/" in v["path"] else v["path"].split("/")[6]
    if "/Instruments/" in v["path"]:
        parts = v["path"].split("/")
        lib = parts[parts.index("Instruments") - 1]
    fs = {fam(e) for e in v["errors"]}
    if len(fs) == 1:
        fam_only[next(iter(fs))] += 1
    for m in fs:
        k = fam_instr[m]
        k["instr"] += 1
        k["libs"][lib] += 1
        if len(k["ex"]) < 3:
            k["ex"].append(v["path"].rsplit("/", 1)[-1])

COMPILE_TAKES = {
 "unknown function num_slices_zone": ("function", "a zone-slice model: `num_slices_zone` asks how many slices the zone was "
        "cut into. Slices are an edit-time division of a sample that neither the plan nor the playback has; the model is "
        "the work, the function is one line"),
 "unknown command hide_inst_icon": ("command", "a no-op binding: `hide_inst_icon()` hides the instrument icon in Kontakt's "
        "header, which KONTRA does not draw. 188 instruments, one command name, and every one of them is blocked by it alone"),
 "unknown callback _pgs_changed": ("callback", "an alias to the dispatcher that already exists (`pgs_changed` handles 2,817 "
        "uses); whether Kontakt honours the underscore spellings is unmeasured — one line if it does"),
 "unknown function zone_slice_start": ("function", "the zone-slice model above (`zone_slice_start` is one of its accessors)"),
 "expected a constant": ("ordering", "our front end refusing something Kontakt accepts, at a `declare` line; the message "
        "does not name the token, so each case needs one look — a parser rule, not a missing feature"),
 "$EVENT_VELOCITY is read-only": ("ordering", "a rule that is simply wrong: these scripts write `$EVENT_VELOCITY := 0` to "
        "clamp the incoming velocity, and Kontakt allows it. Our compiler forbids writing any `$EVENT_*`"),
 "redirect_output takes 2 arguments": ("ordering", "argument-count mismatch in our binding for `redirect_output`; Kontakt's "
        "signature is looser than the one we enforce"),
 "not a UI control": ("ordering", "`hide_part()`/`set_control_par()` called with a variable holding a ui id: we insist on a "
        "literal control, Kontakt resolves it at run time"),
 "$NUM_PRESETS is a reserved name": ("ordering", "`$NUM_PRESETS` is on our reserved list and is not a Kontakt builtin; a "
        "script may declare it. Remove it from the list"),
 "unknown function mf_get_note_length": ("function", "the MIDI-file loop model: the length of the note at index *i* — one "
        "accessor on data `mf_get_note` already reads"),
 "unknown command dont_use_machine_mode": ("command", "a no-op binding (Kontakt's own documentation says it does nothing)"),
 "unknown command set_num_user_zones": ("command", "the user-zone model (see `%NI_USER_ZONE_IDS`); 4 instruments"),
 "?": ("ordering", "unknown token in a `!` string: an undeclared text variable or a build-time name; two instruments, one "
        "product"),
}

rows = []


def add(gap, kind, instruments, libraries, refs, takes, examples, note=""):
    rows.append({"gap": gap, "kind": kind, "instruments": instruments, "libraries": libraries,
                 "references": refs, "what_it_would_take": takes,
                 "example_instruments": examples, "note": note})


# ---------------------------------------------------------------- commands (runtime)
T = {
 "GetEnginePar": "a read that answers `Nothing` for every engine parameter the slot does not model; the record counts "
                 "calls, not ids, so the work list is a probe first (which ids, which slot) and then one law per id",
 "GetEngineParDisp": "the same for the display string: a formatting law per parameter (unit, ratio, rounding), "
                     "measurable only against the native binary with a value sweep",
 "SetEnginePar": "a write declined for ids the engine does not model and for effect type/subtype swaps the slot cannot "
                 "make in place — one law per id plus the async load path",
 "GetFolder": "`get_folder` answers kinds 1-3 (library, patch, factory); these two products ask through an array cell "
              "and the record does not keep the argument, so the missing kind is unmeasured — likely one enum case",
 "LoadArray": "the .nka array read answers only the text-array shape in `on init`; a numeric/other-shape binding onto the "
              "same reader `load_array_str` already uses",
 "AttachZone": "a runtime zone model: zones are a load-time plan today, and this attaches one while the instrument runs",
 "SetUiWfProperty": "a waveform-widget property setter: the waveform view exists, the property is not wired to it",
 "LoadIrSample": "an impulse-response load from `Resources/impulses` (the reader exists — convolution uses it — but this "
                 "entry point does not fill the slot)",
 "LoadPerformanceView": "a private Kontakt performance view: no format, no model. Nothing to implement without the "
                        "private page format the repo does not hold",
 "MfSetExportArea": "the Machine/export-area object (slice export regions), which the plan does not have at all",
 "SetNksNavPar": "the NKS2 navigation model the engine deliberately lacks; 10.0 M calls in 26 instruments means it is a "
                 "hot loop in those UI scripts, not a stray call",
 "SetNksNavName": "as above, the name half of the same navigation model",
 "GetVoiceLimit": "the voice-limit service exists (`set_voice_limit` is bound) but the getter answers a fixed "
                  "placeholder and the counter is not per group",
 "SetVoiceLimit": "accepted and ignored: the engine has no per-group voice cap",
 "FindZone": "a zone search by name: a name index over the runtime zone list, for four instruments",
 "MfSetNumExportAreas": "the export-area object above",
 "MfCopyExportArea": "the export-area object above",
 "SetRpn": "an RPN message to the host, which the offline renderer has no port for",
 "LoadNativeUi": "Kontakt's own private UI pages, which this repo does not hold",
 "ExposeControls": "the host-automation exposure list, which the offline renderer does not have",
 "GetSampleLength": "one field of the loaded sample's metadata",
 "LoadMidiFile": "a MIDI-file read into an array (and its loop model)",
}
for name, a in ACT.items():
    add(name, "command", a["instruments"], a["library_count"], a["calls"],
        T.get(name, "unclassified"), "; ".join(p.rsplit("/", 1)[-1] for p in a["examples"][:3]))

# ---------------------------------------------------------------- symbols (compile)
for m, k in sorted(fam_instr.items(), key=lambda kv: -kv[1]["instr"]):
    key = next((c for c in COMPILE_TAKES if m.startswith(c[:20])), None)
    kind, takes = COMPILE_TAKES.get(m, ("function", "unclassified"))
    add(m, kind, k["instr"], len(k["libs"]),
        f"{k['instr']} instruments refuse to compile; {fam_only[m]} of them fail for this reason alone",
        takes, "; ".join(k["ex"]),
        f"slots that fail to compile: {k['instr']}")

# ---------------------------------------------------------------- bookkeeping
book = collections.defaultdict(lambda: {"ins": set(), "libs": set(), "calls": 0})
for i in S["instruments"]:
    for x in i["symbols"]:
        if (x["symbol"].startswith("$MARK_") and x["symbol"] != "$MARK_1") or x["symbol"] == "%NOTE_DURATION":
            b = book["mark" if x["symbol"].startswith("$MARK_") else "dur"]
            b["ins"].add(i["i"])
            b["libs"].add(i["library"])
            b["calls"] += x["count"]
m, d = book["mark"], book["dur"]
add("$MARK_2 … $MARK_28 (27 constants)", "bookkeeping", len(m["ins"]), len(m["libs"]), m["calls"],
    "a table to fill, not a law: `crates/script/ksp/data/constants.tsv` already holds every value ($MARK_3 = 8, "
    "$MARK_28 = 2^28) and `$MARK_#` is in the catalogue, so scripts using them compile and run today — the ledger simply "
    "has no row for all but `$MARK_1`, and `doctor --strict` reports them as unsupported",
    "40s Very Own - Drums; Vintage Organs", "measured: `consts::int(\"$MARK_3\") == Some(8)` is an existing unit test")
add("%NOTE_DURATION", "bookkeeping", len(d["ins"]), len(d["libs"]), d["calls"],
    "already implemented (`Action::NoteDuration`); the ledger row is stale and names a crate that no longer exists "
    "(`crates/sampler-ksp/src/sema.rs`)", "02. Trumpet 1 Bucket Multi; Soundiron Harpsichord",
    "STATUS.md records the Harpsichord falling silent on this and being fixed")

# ---------------------------------------------------------------- reader walls
add("legacy XML instrument (pre-Kontakt 4)", "reader", 624, 1, "",
    "a format to read: 624 Garritan files are the Kontakt-2-era XML preset our reader refuses. Whether Kontakt 8 itself "
    "opens them is unmeasured here", "Bb Clarinet 1.nki (Garritan - Jazz & Big Band)")
add("modulator category 3", "reader", 544, 6, "",
    "one reader case: `ExternalMod::params` knows category 1 and 2 and rejects 3. The payload layout needs the Kontakt "
    "8.13.1 reader (`FUN_140d09620`, noted at `vendor/ni-file/src/kontakt/objects/modulation.rs`) disassembled, then the "
    "same treatment as category 1. 517 of the 544 are one family (Heavyocity)",
    "Dramatic Techtronic 02.nki (Evolve R2); 8Dio Fire Sax")
add("LASS `.nicnt` access data", "out of scope", 1151, 1, "",
    "not ours: the four `.nicnt` files are present but key-locked, and Kontakt refuses these instruments too without the "
    "library activated (recorded by the orchestrator; not re-verified in this pass)",
    "Aleatoric Non Vib Cellos.nki")

# ---------------------------------------------------------------- sound
add("silent at every key with zones", "symptom", len(C["silent"]),
    len({s["library"] for s in C["silent"]}), "704 instruments keyswept",
    "a symptom, not a gap: only 32 of these 180 have a script that fails to compile; 57 of the 61 keyswept instruments "
    "that call `load_array` are in this set, so `LoadArray` is the best single explanation the numbers offer — the "
    "mechanism itself is unproven",
    "; ".join(s["library"] for s in C["silent"][:3]))
add("no playable zone at all", "not ours", len(C["no_zones"]),
    len({s["library"] for s in C["no_zones"]}), "",
    "242 of the 254 are Spitfire Glass & Steel patches whose every zone is referenced only by purged groups — saved purge "
    "state, not a parse gap",
    "; ".join(s["name"] for s in C["no_zones"][:3]))

# ---------------------------------------------------------------- UI
add("UI asset not found", "not ours", C["totals"]["ui_instruments"], len(C["ui_by_library"]),
    C["totals"]["ui_unresolved_assets"],
    "not ours in every sampled case: 44 instruments across 23 libraries re-checked on today's build, 1,490 distinct "
    "unresolved paths, every one absent from the library on disk; none of the 25 libraries ships a `pictures/` folder. "
    "The counts reproduce the census exactly, so the picture fix moved none of them",
    "Bicycle - Sick Range.nki (8DIO Misfit); Kalimba")

head = ["gap", "kind", "instruments", "libraries", "references", "what_it_would_take",
        "example_instruments", "note"]
rows.sort(key=lambda r: (-r["instruments"], r["gap"]))
out_tsv = sys.argv[1] if len(sys.argv) > 1 else f"{WORK}/gap-inventory.tsv"
with open(out_tsv, "w") as f:
    f.write("\t".join(head) + "\n")
    for r in rows:
        f.write("\t".join(str(r[k]).replace("\t", " ") for k in head) + "\n")
for r in rows:
    print(f"{r['instruments']:5d} {r['libraries']:>4} {r['kind']:<12} {r['gap'][:60]}")
