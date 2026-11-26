#!/usr/bin/env python3
"""The parity board: every Kontakt and UVI Falcon item KONTRA must match, with its status.

    tools/parity-board.py                  generate docs/PARITY.md and docs/PARITY.html (and blame new rows)
    tools/parity-board.py --check          fail when snapshots are malformed, dsp-oracle.tsv cites a
                                           missing/failing test, or flagged-claims.tsv is non-empty
    tools/parity-board.py --ksp-manual KSP_Reference_Manual.txt    refresh snapshots/ksp-manual.tsv
    tools/parity-board.py --uvi-reference [DIR]   refresh snapshots/uvi-reference.tsv from lua.uvi.net
                                                  (or from a directory of its saved pages)
    tools/parity-board.py --census CENSUS.md      refresh snapshots/ni-file-census.tsv from the chunks example

No cargo and no oracle: inputs that need either are the checked-in snapshots under
docs/parity/snapshots/, refreshed by the lane that changes them (the board's header lists the
commands). Each snapshot keeps its rows in byte order, one per line, so two PRs that add rows merge.
The board itself is generated, never committed: committed output made every lane PR conflict. The
site and the public mirror generate it (.github/workflows/mirror.yml in the monorepo). Every status
is derived structurally from the repo: passing v3 oracle manifests, law tables, source registrations,
and corpus censuses. snapshots/overrides.tsv is the only hand-typed layer, and every row gives a reason.

Statuses: tested (KONTRA does it and a real test, probe or oracle measurement verifies it),
done (KONTRA does it with verified law), partial (some of it), unseen (reader exists, 0 corpus
objects observed), missing. "Passing" is tested + done (unseen does not count as passing).
"""
import collections
import hashlib
import html
import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SNAP = ROOT / "docs/parity/snapshots"
ATTRIBUTION = ROOT / "docs/parity/attribution.tsv"  # local caches, gitignored
FLAGGED = ROOT / "docs/parity/flagged-claims.tsv"
OUT_MD = ROOT / "docs/PARITY.md"
OUT_HTML = ROOT / "docs/PARITY.html"
REPO = "https://github.com/DerpcatMusic/KONTRA/blob/main/"
UVI_DOCS = "https://lua.uvi.net/"
STATUSES = ("tested", "done", "partial", "unseen", "missing")
CACHE = pathlib.Path(os.environ.get("XDG_CACHE_HOME", pathlib.Path.home() / ".cache")) / "kontra-parity-blame"

_TEXT_CACHE = {}


def read(rel):
    p = ROOT / rel
    key = p.as_posix()
    if key not in _TEXT_CACHE:
        _TEXT_CACHE[key] = p.read_text(errors="ignore") if p.is_file() else ""
    return _TEXT_CACHE[key]


def tsv(path, header=True):
    lines = [l.rstrip("\n").split("\t") for l in pathlib.Path(path).read_text().splitlines() if l and not l.startswith("#")]
    if not lines:
        return []
    return [dict(zip(lines[0], r)) for r in lines[1:]] if header else lines


def files(*globs):
    return sorted(p for g in globs for p in ROOT.glob(g) if p.is_file() and "target" not in p.parts)


def find_regex(paths, pattern):
    """First repo-relative path in `paths` whose text matches compiled regex `pattern`."""
    for p in paths:
        rel = p.relative_to(ROOT).as_posix()
        if pattern.search(read(rel)):
            return rel
    return None


def tsv_first_col_names(paths):
    """Exact `$NAME` -> first repo-relative TSV path mapping from column 0 of binding TSVs."""
    found = {}
    for p in paths:
        rel = p.relative_to(ROOT).as_posix()
        for line in read(rel).splitlines():
            if not line or line.startswith("#"):
                continue
            cell = line.split("\t", 1)[0].strip()
            if cell.startswith("$"):
                found.setdefault(cell.lstrip("$").upper(), rel)
    return found


def row(rid, area, group, name, status, ev=None, note=""):
    return dict(id=rid, area=area, group=group, name=name, status=status, ev=ev, note=note)


def grade(cells):
    """A parameter's status from its applicable cells [law, display, range, acts].
    `tested` requires every applicable cell; `done` requires verified `law` and `acts`."""
    law, acts = cells[0], cells[-1]
    applicable = [c for c in cells if c is not None]
    if applicable and all(applicable):
        return "tested"
    if law and acts:
        return "done"
    return "partial" if any(applicable) else "missing"


# ------------------------------------------------------------------ shared inputs
def ksp_inputs():
    manual = tsv(SNAP / "ksp-manual.tsv")
    status = {}
    for kind, name, st in tsv(SNAP / "ksp-status.tsv", header=False):
        status[(kind, name.lower() if kind != "par" else name.lstrip("$").upper())] = st
    return manual, status


def load_dsp_oracle():
    """Structured Kontakt DSP oracle manifest (`snapshots/dsp-oracle.tsv`)."""
    path = SNAP / "dsp-oracle.tsv"
    if not path.is_file():
        return {}
    return {r["effect_id"]: r for r in tsv(path)}


def dsp_oracle_errors():
    """Validate every row of `snapshots/dsp-oracle.tsv`: real file, real non-ignored test fn, no failing status."""
    path = SNAP / "dsp-oracle.tsv"
    if not path.is_file():
        return ["dsp-oracle.tsv: missing required manifest"]
    errors = []
    seen = set()
    for r in tsv(path):
        eid = r.get("effect_id", "")
        test_name = r.get("test_name", "")
        max_err = r.get("max_err_db", "")
        st = r.get("status", "")
        commit_sha = r.get("commit", "")
        if eid in seen:
            errors.append(f"dsp-oracle.tsv: duplicate effect_id {eid!r}")
        seen.add(eid)
        if st not in ("tested", "partial", "failing"):
            errors.append(f"dsp-oracle.tsv: {eid}: invalid status {st!r}")
        elif st == "failing":
            errors.append(f"dsp-oracle.tsv: {eid}: test {test_name!r} is marked failing")
        try:
            float(max_err)
        except ValueError:
            errors.append(f"dsp-oracle.tsv: {eid}: max_err_db {max_err!r} is not a number")
        if not commit_sha:
            errors.append(f"dsp-oracle.tsv: {eid}: missing commit")
        if "::" not in test_name:
            errors.append(f"dsp-oracle.tsv: {eid}: test_name {test_name!r} must be '<rel_path>::<fn_name>'")
            continue
        rel_path, fn_name = test_name.split("::", 1)
        target = ROOT / rel_path
        if not target.is_file():
            errors.append(f"dsp-oracle.tsv: {eid}: cited test file missing: {rel_path}")
            continue
        lines = target.read_text(errors="ignore").splitlines()
        fn_rx = re.compile(rf"\bfn\s+{re.escape(fn_name)}\s*\(")
        matches = [i for i, line in enumerate(lines) if fn_rx.search(line)]
        if not matches:
            errors.append(f"dsp-oracle.tsv: {eid}: cited test fn missing: {test_name}")
            continue
        idx = matches[0]
        header = "\n".join(lines[max(0, idx - 5):idx])
        if "#[ignore" in header:
            errors.append(f"dsp-oracle.tsv: {eid}: cited test fn is #[ignore]: {test_name}")
    return errors


# ------------------------------------------------------------------ Kontakt: KSP and parameters
# Maps `kontra_vm::event::*` constants handled in engine src to KSP `$EVENT_PAR_*` bare names.
EVENT_CONST_TO_KSP = {
    "VOLUME": ("EVENT_PAR_VOLUME",),
    "PAN": ("EVENT_PAR_PAN",),
    "TUNE": ("EVENT_PAR_TUNE",),
    "NOTE": ("EVENT_PAR_NOTE",),
    "SOURCE": ("EVENT_PAR_SOURCE",),
    "VELOCITY": ("EVENT_PAR_VELOCITY",),
    "REL_VELOCITY": ("EVENT_PAR_REL_VELOCITY",),
    "ALLOW_GROUP": ("EVENT_PAR_ALLOW_GROUP",),
    "MOD_VALUE": ("EVENT_PAR_MOD_VALUE_ID",),
    "MOD_VALUE_EX": ("EVENT_PAR_MOD_VALUE_EX_ID",),
    "CUSTOM": ("EVENT_PAR_CUSTOM",),
    "CUSTOM_LAST": ("EVENT_PAR_0", "EVENT_PAR_1", "EVENT_PAR_2", "EVENT_PAR_3"),
    "ZONE_SLOT": ("EVENT_PAR_ZONE_SLOT",),
    "PLAY_POS": ("EVENT_PAR_PLAY_POS",),
    "ZONE_ID": ("EVENT_PAR_ZONE_ID",),
    "MIDI_CHANNEL": ("EVENT_PAR_MIDI_CHANNEL",),
    "MIDI_COMMAND": ("EVENT_PAR_MIDI_COMMAND",),
    "MIDI_BYTE_1": ("EVENT_PAR_MIDI_BYTE_1",),
    "MIDI_BYTE_2": ("EVENT_PAR_MIDI_BYTE_2",),
    "POS": ("EVENT_PAR_POS",),
    "TRACK_NR": ("EVENT_PAR_TRACK_NR",),
    "NOTE_LENGTH": ("EVENT_PAR_NOTE_LENGTH",),
    "ID": ("EVENT_PAR_ID",),
}


def handled_event_pars():
    """`$EVENT_PAR_*` names structurally matched in engine script/midi_in/midi_object handlers."""
    ev_src = "\n".join(read(p) for p in (
        "crates/engine/engine/src/script.rs",
        "crates/engine/engine/src/midi_in.rs",
        "crates/engine/engine/src/midi_object.rs",
    ))
    used_consts = set(re.findall(r"\bevent::([A-Z0-9_]+)\b", ev_src))
    out = set()
    for c in used_consts:
        out.update(EVENT_CONST_TO_KSP.get(c, ()))
    return out


def kontakt_ksp_and_params(manual, status):
    """The KSP manual's items, split into language items and parameters."""
    tsv_consts = read("crates/script/ksp/data/constants.tsv")
    valued = {l.split("\t")[0].lstrip("$").upper() for l in tsv_consts.splitlines() if l and l[0] != "#"}
    valued |= {n.lstrip("~").upper() for n in re.findall(r'"(~NI_[A-Z_]+)"', read("crates/script/ksp/src/consts.rs"))}
    vendor = {l.strip().lstrip("$%~@!").upper() for l in read("crates/script/ksp/data/vendor_constants.txt").splitlines()
              if l.strip() and l[0] != "#"}
    # Only real Rust KSP tests and measured oracle probes verify KSP items (no `.lua` constant dumps).
    ksp_test_paths = [
        p for p in files("crates/script/ksp/tests/**/*.rs", "crates/app/host/tests/**/*.rs")
        if re.search("ksp", read(p.relative_to(ROOT).as_posix()), re.I)
    ] + [p for p in files("tools/kontakt-oracle/probes/ksp/*.kspp") if p.with_suffix(".measured").is_file()]
    measured_probe_paths = [
        p for p in files("tools/kontakt-oracle/probes/ksp/*.kspp", "tools/ksp-ui/probes/*.ksp")
        if p.with_suffix(".measured").is_file()
    ]
    disp_rows = read("docs/re/kontakt-bindings/engine-par-disp.tsv").splitlines()
    pending = {n for l in disp_rows if l.startswith("# pending:") for n in l[len("# pending:"):].split()}
    disp = {l.split("\t")[0].lstrip("$").upper() for l in disp_rows if l and not l.startswith("#")} - {p.lstrip("$").upper() for p in pending}
    disp_file = "docs/re/kontakt-bindings/engine-par-disp.tsv"
    ui_paths = [p for d in ("crates/app/face/src", "crates/app/ui/src", "crates/engine/script-ui/src", "crates/app/host/src")
                for p in sorted((ROOT / d).rglob("*.rs"))]
    ui_text = "\n".join(read(p.relative_to(ROOT).as_posix()) for p in ui_paths)
    ui_actions = set(re.findall(r"Action::(\w+)", ui_text))
    ui_names = set(re.findall(r'"\$((?:CONTROL_PAR|UI_WF_PROP|UI_WAVEFORM|NI_MOUSE)[A-Z0-9_]*)"', ui_text))
    ev_handled = handled_event_pars()
    src_paths = files("crates/script/ksp/src/**/*.rs", "crates/engine/vm/src/**/*.rs",
                      "crates/engine/engine/src/**/*.rs", "crates/engine/script-ui/src/**/*.rs",
                      "crates/app/face/src/**/*.rs")
    plan = read("crates/engine/plan/src/script.rs")
    ran = set(re.findall(r"pub (\w+):", plan[plan.index("pub struct Callbacks"):].split("\n}", 1)[0]))
    CB = {"init": "init", "note": "note", "release": "release", "controller": "controller", "poly_at": "polyat",
          "ui_control": "uicontrol", "ui_controls": "uicontrols", "ui_update": "uiupdate", "listener": "listener",
          "pgs_changed": "pgschanged", "persistence_changed": "persistencechanged", "async_complete": "asynccomplete",
          "rpn": "rpn", "nrpn": "nrpn", "note_controller": "notecontroller", "midi_in": "midiin"}
    # Exact first-column $ENGINE_PAR_* lookup from binding TSVs (no bag-of-words matching).
    law_files = [p for p in files("docs/re/kontakt-bindings/*.tsv") if p.name != "engine-par-disp.tsv"]
    law_map = tsv_first_col_names(law_files)
    clamp_map = tsv_first_col_names([ROOT / "docs/re/kontakt-bindings/engine_par_clamp.tsv"])

    ksp, pars = [], []
    for item in manual:
        kind, name, sec = item["kind"], item["name"], item["section"]
        key = name.lower().lstrip("$%~@!")
        bare = name.lstrip("$%~@!").upper()
        if kind == "widget":
            continue
        if kind == "par":  # $ENGINE_PAR_*: value law, display text, range, drives sound
            law_ev = law_map.get(bare)
            # acts: the engine plays it; HeldOnly (`Opaque` and `GroupHeld`) reports `held`.
            cells = [law_ev is not None, bare in disp, bare in clamp_map, status.get(("par", bare)) == "carried"]
            ev = (law_ev, name) if law_ev else (disp_file, name) if cells[1] else None
            if not ev and cells[3]:
                ev = ("docs/parity/snapshots/ksp-status.tsv", bare)
            pars.append(row(f"kontakt.par.{bare}", "kontakt.par", "$ENGINE_PAR", name, grade(cells), ev, cells_note(cells)))
            continue
        if kind in ("uipar", "evpar"):  # $CONTROL_PAR_*, $ZONE_PAR_*, $EVENT_PAR_*: measured law, acts
            if bare.startswith("ZONE_PAR"):
                acts = status.get(("par", bare)) == "carried"
                group = "$ZONE_PAR"
            elif kind == "evpar":
                acts = bare in ev_handled
                group = "$EVENT_PAR"
            else:
                acts = bare in ui_names
                group = "$CONTROL_PAR"
            tok_rx = re.compile(rf"\${re.escape(bare)}\b", re.I)
            probe = find_regex(measured_probe_paths, tok_rx)
            cells = [probe is not None, None, None, acts]
            ev = (probe, name) if probe else (find_regex(src_paths, tok_rx), name) if acts else None
            if ev and not ev[0]:
                ev = None
            pars.append(row(f"kontakt.par.{bare}", "kontakt.par", group, name, grade(cells), ev, cells_note(cells)))
            continue
        ev = None
        if kind == "fn":
            st = status.get(("fn", key))
            got = {"vm": "done", "answered": "done", "partial": "partial", "missing": "missing"}.get(st, "missing")
            if got in ("partial", "missing") and "".join(w.capitalize() for w in key.split("_")) in ui_actions:
                got = "done"
            group, rid = "commands", f"kontakt.ksp.cmd.{key}"
            test_rx = re.compile(rf"\b{re.escape(key)}\s*\(", re.I)
            src_rx = re.compile(rf"\b{re.escape(key)}\b", re.I)
        elif kind == "cb":
            got = "missing" if name not in CB else "partial" if name not in ran else "done"
            group, rid = "callbacks", f"kontakt.ksp.cb.{key}"
            test_rx = re.compile(rf"\bon\s+{re.escape(key)}\b", re.I)
            src_rx = re.compile(rf"\b{re.escape(key)}\b", re.I)
        else:
            known_fn = any(("fn", p + key) in status for p in ("", "$", "%"))
            got = "done" if known_fn or bare in ui_names or bare in valued else "partial" if bare in vendor else "missing"
            group = "built-in variables" if kind == "var" else "constants and names"
            rid = f"kontakt.ksp.{'var' if kind == 'var' else 'const'}.{bare}"
            test_rx = re.compile(rf"[$%~@!]{re.escape(bare)}\b", re.I)
            src_rx = re.compile(rf"\b{re.escape(bare)}\b", re.I)
        if got == "done":
            t = find_regex(ksp_test_paths, test_rx)
            if t:
                got, ev = "tested", (t, key)
            else:
                s = find_regex(src_paths, src_rx)
                if s:
                    ev = (s, key)
        ksp.append(row(rid, "kontakt.ksp", group, name, got, ev, sec))
    return ksp, pars


def cells_note(cells):
    names = ("law", "display", "range", "acts")
    return " ".join(f"{n} {'✓' if c else '✗'}" for n, c in zip(names, cells) if c is not None)


def modulation_targets():
    code = read_dir("crates/format/kontakt/src")
    out = []
    for r in tsv(SNAP / "modulation-targets.tsv"):
        p = r["param"]
        if p == "UNREGISTERED":
            continue
        cells = [bool(r["template"]), None, "UNPROVED" not in r["notes"], f'"{p}"' in code]
        ev = (find_in_dir("crates/format/kontakt/src", f'"{p}"'), p) if cells[3] else ("docs/parity/snapshots/modulation-targets.tsv", p)
        out.append(row(f"kontakt.par.MOD_{p}", "kontakt.par", "modulation targets", f"{p} (target {r['target_id']})",
                       grade(cells), ev, cells_note(cells)))
    return out


_dirs = {}


def read_dir(rel):
    if rel not in _dirs:
        _dirs[rel] = {p.relative_to(ROOT).as_posix(): p.read_text(errors="ignore") for p in sorted((ROOT / rel).rglob("*.rs"))}
    return "\n".join(_dirs[rel].values())


def find_in_dir(rel, needle):
    read_dir(rel)
    return next((path for path, text in _dirs[rel].items() if needle in text), None)


# ------------------------------------------------------------------ Kontakt: UI
UI_TYPE = re.compile(r"declare\s+(ui_[a-z_]+)\s+[$%?@!]?(\w+)")


def kontakt_ui(manual):
    """Each control type x each $CONTROL_PAR it reads or writes: what the manual says applies, plus every
    pair a probe exercises. tested: an oracle probe KONTRA replays (oracle_probes.rs) measures it; done: a
    KSP UI probe page Kontakt was measured on uses it; partial: a probe page uses it, unmeasured."""
    pairs = {}  # (type, PAR) -> (status, evidence path)
    rank = {s: i for i, s in enumerate(STATUSES)}
    probes = [(p, "tested") for p in files("tools/kontakt-oracle/probes/ksp/*.kspp") if p.with_suffix(".measured").is_file()]
    probes += [(p, "done" if p.with_suffix(".measured").is_file() else "partial") for p in files("tools/ksp-ui/probes/*.ksp")]
    for path, st in probes:
        text = path.read_text(errors="ignore")
        types = {n.lower(): t for t, n in UI_TYPE.findall(text)}
        ids = {a.lower(): types.get(b.lower()) for a, b in re.findall(r"\$(\w+)\s*:=\s*get_ui_id\(\s*[$%?@!]?(\w+)", text)}
        for target, par in re.findall(r"(?:get|set)_control_par(?:_str|_arr|_real)?(?:_arr)?\(\s*([^,]+?)\s*,\s*\$(CONTROL_PAR_\w+)", text):
            m = re.match(r"get_ui_id\(\s*[$%?@!]?(\w+)", target)
            t = types.get(m[1].lower()) if m else ids.get(target.lstrip("$%?@!").lower())
            if not t:
                continue
            key = (t, par)
            if key not in pairs or rank[st] < rank[pairs[key][0]]:
                pairs[key] = (st, path.relative_to(ROOT).as_posix())
    for item in manual:
        if item["kind"] == "uipar" and item.get("applies"):
            for t in item["applies"].split(","):
                pairs.setdefault((t, item["name"].lstrip("$")), ("missing", None))
    out = [row(f"kontakt.ui.{t}.{p}", "kontakt.ui", t, f"{t} × ${p}", st, (ev, p) if ev else None)
           for (t, p), (st, ev) in sorted(pairs.items())]
    corpus = tsv(SNAP / "ui-corpus.tsv")
    return out, corpus


# ------------------------------------------------------------------ Kontakt: format (ni-file)
def kontakt_format():
    """Every serialized class in `ser_classes.tsv` UNION every class observed in `ni-file-census.tsv`:
    tested = every corpus object read in full; partial = some bytes unaccounted for;
    unseen = reader exists but 0 objects in corpus (does not count as passing); missing = no reader."""
    census = collections.defaultdict(lambda: [0, 0, 0, 0])
    census_sertype = {}
    totals = {}
    for r in tsv(SNAP / "ni-file-census.tsv"):
        if r["sertype"] == "total":
            totals = r
            continue
        cls_name = r["class"]
        census_sertype.setdefault(cls_name, r["sertype"])
        c = census[cls_name]
        for i, k in enumerate(("objects", "full", "bytes", "unaccounted")):
            c[i] += int(r[k])
    cov = {}
    for line in read("docs/re/kontakt-catalog/parser_coverage.md").splitlines():
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(c) == 6 and c[0].startswith("0x"):
            cov[c[1]] = c
    out = []
    seen_names = set()
    for r in tsv(ROOT / "docs/re/kontakt-catalog/ser_classes.tsv"):
        full = r["class"]
        name = re.sub(r"^(class|struct) ", "", full)
        seen_names.add(name)
        c = census.get(name)
        if c and c[0]:
            st = "tested" if c[0] == c[1] else "partial"
            note = f"{c[1]}/{c[0]} objects full, {c[3]} of {c[2]} bytes unaccounted"
            ev = ("docs/parity/snapshots/ni-file-census.tsv", name)
        else:
            cv = cov.get(full, ["", "", "", "", "", ""])
            stub = "no stream reader" in cv[5] or cv[3] in ("stub", "not-a-reader", "")
            st = "missing" if stub and not cv[4] else "unseen"
            note = "0 corpus objects (unseen)" if st == "unseen" else "not in the corpus, no reader"
            ev = ("docs/re/kontakt-catalog/parser_coverage.md", name) if st == "unseen" else None
        out.append(row(f"kontakt.format.{r['sertype']}.{re.sub(r'[^A-Za-z0-9]+', '_', name).strip('_')}", "kontakt.format",
                       "serialized classes", f"{r['sertype']} {name}", st, ev, note))
    # Union with all classes present in ni-file-census.tsv not listed in ser_classes.tsv
    # (exposes top-level/container classes like Bank, Program, ZoneList, FNTableImpl, etc.).
    for name in sorted(census.keys()):
        if name in seen_names:
            continue
        c = census[name]
        if not c[0]:
            continue
        sertype = census_sertype[name]
        st = "tested" if c[0] == c[1] else "partial"
        note = f"{c[1]}/{c[0]} objects full, {c[3]} of {c[2]} bytes unaccounted"
        ev = ("docs/parity/snapshots/ni-file-census.tsv", name)
        out.append(row(f"kontakt.format.{sertype}.{re.sub(r'[^A-Za-z0-9]+', '_', name).strip('_')}", "kontakt.format",
                       "serialized classes", f"{sertype} {name}", st, ev, note))
    after = re.findall(r"^\| After \|.*\| (\d+) \|\s*$", read("docs/re/NI_FILE_PARITY.md"), re.M)
    return out, totals, (int(after[-1]) if after else None)


# ------------------------------------------------------------------ DSP processing types
TYPES = [  # processing type by the module's own name
    ("convolution", r"convol|irc|ireverb|sampled ?reverb"),
    ("reverb", r"reverb|raum|plate|verb|diffus|bloom|opal"),
    ("delay", r"delay|echo|replika|psyche|bbd delay"),
    ("time-stretch / slicing", r"masher|slicer|reverse grain|stretch|granul|shifter|harmoniz|gater|freq shift"),
    ("modulation FX", r"chorus|flang|phas|flair|choral|vibrato|rotat|wow|ring|freak|tune|ensemble|thorus|autopan|tremolo"),
    ("EQ", r"\beq\b|eq\b|g-eq|tilt|tone stack|shelf|band shelf|passive"),
    ("filter", r"filter|wah|svf|ladder|lowpass|one pole|lpg|formant|comb|resonat|talk"),
    ("dynamics", r"comp|limit|transient|gainer|gate|expander|maxim|mastering"),
    ("distortion", r"distort|satur|skream|lo-?fi|bite|dirt|fuzz|cat\b|stortion|chainsaw|tape|supercharg|drive|cabinet|twang|"
                   r"jump|van51|ac box|hot solo|bass|crunch|clip|crush|redux|destruct|guitar|amp|preamp|core cell|kolor|exciter|shaper"),
]


def ptype(name):
    n = name.lower()
    return next((t for t, rx in TYPES if re.search(rx, n)), "utility")


def key_of(name):
    return re.sub(r"[^a-z0-9]", "", name.lower().replace("(legacy)", ""))


# MODULES names whose binding-table effect name differs after key_of.
BINDING = {"Reverb": "galoisreverb", "Replika": "rdelay", "Stereo Modeller": "stereo", "Supercharger": "superchargergt",
           "Hilbert Limiter": "transparentlimiter"}

# Explicit structural registration checks for Kontakt modulator classes:
# cls -> ((ir_file, ir_symbol), (engine_file, engine_symbol))
MODULATOR_CHECKS = {
    "BParLFO": (
        ("crates/format/kontakt/src/timed.rs", "Modulator::Lfo("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::Lfo("),
    ),
    "BParArp": (
        ("crates/format/kontakt/src/timed.rs", "Modulator::StepMod("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::Shaped("),
    ),
    "BParEnv_AHDSR": (
        ("crates/format/kontakt/src/timed.rs", "Modulator::Ahdsr("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::Envelope("),
    ),
    "BParEnv_DBD": (
        ("crates/format/kontakt/src/timed.rs", "Modulator::Dbd("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::Envelope("),
    ),
    "BParEnv_FM7": (
        ("crates/format/kontakt/src/timed.rs", "Modulator::Flex("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::Envelope("),
    ),
    "BParEnvF": (
        ("crates/format/kontakt/src/timed.rs", "Modulator::Follower("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::EnvelopeFollower"),
    ),
    "BParGlide": (
        ("crates/format/kontakt/src/translate.rs", "Modulator::Glide("),
        ("crates/engine/engine/src/glide.rs", "pub(crate) struct Slide"),
    ),
    "BParExternalMod": (
        ("crates/format/kontakt/src/external.rs", "pub(crate) fn external("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::Controller("),
    ),
    "BParInternalMod": (
        ("crates/format/kontakt/src/timed.rs", "pub(crate) fn timed("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::Lfo("),
    ),
    "BZoneLevelEnv": (
        ("crates/format/kontakt/src/laws.rs", "pub(crate) fn zone_envelope("),
        ("crates/engine/engine/src/voice_gain.rs", "ir::Source::ZoneEnvelope("),
    ),
}


def oracle_row(rid, group, label, slug, ir_ok, oracle):
    """Grade one `.measured` row strictly from `snapshots/dsp-oracle.tsv`."""
    hit = oracle.get(slug) if ir_ok else None
    if not hit or hit["status"] not in ("tested", "partial"):
        return row(rid, "kontakt.dsp", group, label, "missing", None, "")
    rel_path, fn_name = hit["test_name"].split("::", 1)
    note = f"max {hit['max_err_db']} dB ({fn_name})"
    if hit["status"] == "partial":
        note = "partial: " + note
    return row(rid, "kontakt.dsp", group, label, hit["status"], (rel_path, fn_name), note)


def kontakt_dsp(status):
    """Every effect module in `format/kontakt/src/effects.rs` MODULES and every modulator class,
    three rows each: in IR, driven live, and output vs oracle (strictly from `dsp-oracle.tsv`)."""
    src = read("crates/format/kontakt/src/effects.rs")
    table = src[src.index("const MODULES"):src.index("];", src.index("const MODULES"))]
    oracle = load_dsp_oracle()
    own = {}
    for f in files("docs/re/kontakt-bindings/fx_*.tsv"):
        for r in tsv(f):
            if r["class"] != "BParFX":
                own.setdefault(key_of(r["effect"].removeprefix("Effects/")), set()).add(r["par"].lstrip("$").upper())
    played = {n for (k, n), st in status.items() if k == "par" and st == "carried"}
    out = []
    for m in re.finditer(r'(fx|model)\((0x[0-9a-f]+|&crate::(\w+)::(\w+)), "([^"]+)", [^\n]*?(Read|Unread|\|s\|)', table):
        kind, name = m[1], m[5]
        words = [w for w in re.findall(r"[a-z0-9]+", name.lower()) if w not in ("legacy",)]
        slug = "_".join(words)
        t = ptype(name)
        ir_ok = m[6] != "Unread"
        ev = ("crates/format/kontakt/src/effects.rs", name)
        out.append(row(f"kontakt.dsp.{slug}.ir", "kontakt.dsp", t, f"{name}: in IR", "done" if ir_ok else "missing", ev if ir_ok else None))
        pars = own.get(BINDING.get(name, key_of(name)), set())
        live = pars <= played if pars else kind == "model"
        out.append(row(f"kontakt.dsp.{slug}.driven", "kontakt.dsp", t, f"{name}: driven live",
                       "done" if live else "partial" if ir_ok else "missing", ev if ir_ok else None,
                       "" if live or not ir_ok else "held: " + " ".join(sorted(pars - played)) if pars else "load-time only"))
        out.append(oracle_row(f"kontakt.dsp.{slug}.measured", t, f"{name}: output vs oracle", slug, ir_ok, oracle))
    for r in tsv(ROOT / "docs/re/kontakt-catalog/ser_classes.tsv"):
        cls = re.sub(r"^(class|struct) ", "", r["class"])
        if not re.fullmatch(r"BPar(LFO|Env_\w+|EnvF|Glide|Arp|ExternalMod|InternalMod)|BZoneLevelEnv", cls):
            continue
        token = cls.split("_")[-1] if "_" in cls else cls.removeprefix("BPar").removeprefix("B")
        slug = token.lower()
        t = "envelope" if "Env" in cls else "LFO" if "LFO" in cls else "modulation source"
        check = MODULATOR_CHECKS.get(cls)
        if check:
            (ir_file, ir_sym), (drv_file, drv_sym) = check
            ir_ok = ir_sym in read(ir_file)
            drv_ok = ir_ok and drv_sym in read(drv_file)
            ir_ev = (ir_file, ir_sym) if ir_ok else None
            drv_ev = (drv_file, drv_sym) if drv_ok else ir_ev
        else:
            ir_ok = drv_ok = False
            ir_ev = drv_ev = None
        out.append(row(f"kontakt.dsp.{slug}.ir", "kontakt.dsp", t, f"{cls}: in IR", "done" if ir_ok else "missing", ir_ev))
        out.append(row(f"kontakt.dsp.{slug}.driven", "kontakt.dsp", t, f"{cls}: driven",
                       "done" if drv_ok else "partial" if ir_ok else "missing", drv_ev))
        out.append(oracle_row(f"kontakt.dsp.{slug}.measured", t, f"{cls}: output vs oracle", slug, ir_ok, oracle))
    return out


# ------------------------------------------------------------------ UVI Falcon
def uvi():
    """UVI Falcon from its public script reference (snapshots/uvi-reference.tsv, cited per row)."""
    ref = tsv(SNAP / "uvi-reference.tsv")
    lua_src = "\n".join(read(p) for p in ("crates/script/lua/src/prelude.lua", "crates/script/lua/src/widgets.lua")) \
        + read_dir("crates/script/lua/src")
    lua_test_paths = files("crates/script/lua/tests/**/*", "crates/format/uvi/src/program/tests.rs")
    probe = {}
    for l in read("crates/script/lua/tests/uvi_ui/probe.expected").splitlines():
        if ": " in l:
            k, v = l.rsplit(": ", 1)
            probe[k] = v
    uvi_code = "\n".join(t for p, t in sorted(_read_tree("crates/format/uvi/src").items()) if not p.endswith("tests.rs"))
    uvi_test = read("crates/format/uvi/src/program/tests.rs") + "\n".join(
        t[t.find("#[cfg(test)]"):] for t in _read_tree("crates/format/uvi/src").values() if "#[cfg(test)]" in t)
    features = {r["name"]: r["coverage"] for r in tsv(SNAP / "uvi-elements.tsv")}
    out = []
    for r in ref:
        kind, group, owner, name = r["kind"], r["group"], r["owner"], r["name"]
        cite = r["source"]
        if kind in ("function", "member"):
            full = f"{owner}.{name}" if owner else name
            in_probe = probe.get(full)
            defined = re.search(rf"(function\s+(?:[\w.:]*[.:])?{re.escape(name)}\b|\b{re.escape(name)}\s*=|\"{re.escape(name)}\")", lua_src)
            call_rx = re.compile(rf"(?:[.:]\s*{re.escape(name)}\b|\b{re.escape(name)}\s*\()")
            t = find_regex(lua_test_paths, call_rx)
            if in_probe == "ok":
                st, ev = "tested", ("crates/script/lua/tests/uvi_ui/probe.expected", f"{full}: ok")
            elif in_probe:
                st, ev = "partial", ("crates/script/lua/tests/uvi_ui/probe.expected", full)
            elif defined and t:
                st, ev = "tested", (t, name)
            elif defined:
                st, ev = "done", (_lua_file(name), name)
            else:
                st, ev = "missing", None
            area = "uvi.ui" if r["group"] == "UI" else "uvi.api"
            out.append(row(f"{area}.{full}", area, owner or group, full, st, ev, cite))
        elif kind == "element":
            t = ptype(r["display"]) if group == "FX" else group.lower()
            lit = f'"{name}"' in uvi_code or f"<{name}" in uvi_code
            cov = features.get(name, "")
            drv = lit and any(w in cov for w in ("mapped", "runtime-consumed"))
            tested = f"<{name}" in uvi_test or f'"{name}"' in uvi_test
            ev = (_uvi_file(name), name) if lit else None
            label = r["display"]
            out.append(row(f"uvi.dsp.{name}.ir", "uvi.dsp", t, f"{label}: in IR", "tested" if lit and tested else "done" if lit else "missing", ev, cite))
            out.append(row(f"uvi.dsp.{name}.driven", "uvi.dsp", t, f"{label}: driven", "done" if drv else "partial" if lit else "missing",
                           ("docs/parity/snapshots/uvi-elements.tsv", name) if drv else None, cov))
            out.append(row(f"uvi.dsp.{name}.measured", "uvi.dsp", t, f"{label}: output vs oracle", "missing", None,
                           "no UVI oracle in the repo yet"))
        elif kind == "param":
            lit = f'"{name}"' in uvi_code
            el_driven = f'"{owner}"' in uvi_code and any(w in features.get(owner, "") for w in ("mapped", "runtime-consumed"))
            cells = [lit, None, lit and _ranged(uvi_code, name), lit and el_driven]
            ev = (_uvi_file(f'"{name}"'), name) if lit else None
            out.append(row(f"uvi.par.{owner}.{name}", "uvi.par", group, f"{owner}.{name}", grade(cells), ev, cells_note(cells)))
    return out


_trees = {}


def _read_tree(rel):
    if rel not in _trees:
        _trees[rel] = {p.relative_to(ROOT).as_posix(): p.read_text(errors="ignore") for p in sorted((ROOT / rel).rglob("*.rs"))}
    return _trees[rel]


def _uvi_file(needle):
    return next((p for p, t in _read_tree("crates/format/uvi/src").items() if needle.strip('"') in t), "crates/format/uvi/src")


def _lua_file(name):
    for p in ("crates/script/lua/src/prelude.lua", "crates/script/lua/src/widgets.lua"):
        if re.search(rf"\b{re.escape(name)}\b", read(p)):
            return p
    return next((p for p, t in _read_tree("crates/script/lua/src").items() if name in t), "crates/script/lua/src")


def _ranged(code, name):
    return re.search(rf'"{re.escape(name)}"[^\n]{{0,120}}(clamp|max\(|min\()', code) is not None


# ------------------------------------------------------------------ overrides, attribution
def apply_overrides(rows):
    by = {r["id"]: r for r in rows}
    for o in tsv(SNAP / "overrides.tsv"):
        if not o.get("reason"):
            sys.exit(f"overrides.tsv: {o['id']} has no reason")
        if o["id"] in by:
            by[o["id"]]["status"] = o["status"]
            by[o["id"]]["note"] = f"override: {o['reason']}"


def git(*args):
    return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True).stdout


def blame(path, needle):
    """The commit that last changed the first line of `path` naming `needle` (cached by the file's content)."""
    p = ROOT / path
    if not p.is_file():
        return None
    data = p.read_bytes()
    key = hashlib.sha1(data).hexdigest()
    cache = CACHE / f"{key}.json"
    if cache.is_file():
        shas = json.loads(cache.read_text())
    else:
        shas = re.findall(r"^([0-9a-f]{40}) \d+ \d+", git("blame", "-w", "--line-porcelain", "--", str(p)), re.M)
        CACHE.mkdir(parents=True, exist_ok=True)
        cache.write_text(json.dumps(shas))
    needle = needle.lower()
    for i, line in enumerate(data.decode(errors="ignore").splitlines()):
        if needle in line.lower() and i < len(shas):
            return shas[i]
    return shas[0] if shas else None


_commits = {}


def commit(sha):
    """(PR, date) of a commit. Commits from before KONTRA moved into the monorepo touch top-level paths:
    their numbers are the public KONTRA repo's PRs (`K123`); later ones are monorepo PRs (`123`)."""
    if sha not in _commits:
        out = git("show", "--name-only", "--format=%s%x1f%cs%x1f", sha).split("\x1f")
        _commits[sha] = out if len(out) == 3 else ["", "", ""]
    subject, date, paths = _commits[sha]
    pr = re.search(r"\(#(\d+)\)\s*$", subject)
    mono = any(l.startswith("plugins/") for l in paths.split("\n"))
    return (("" if mono else "K") + pr[1] if pr else ""), date


def claims():
    """`Parity: <row-id> ...` trailers: row id -> PR numbers."""
    out = collections.defaultdict(set)
    for entry in git("log", "--format=%s%x1f%b%x1e", "--", ".").split("\x1e"):
        if "\x1f" not in entry:
            continue
        subject, body = entry.strip("\n").split("\x1f", 1)
        pr = re.search(r"\(#(\d+)\)\s*$", subject)
        for ids in re.findall(r"^Parity:\s*(.+)$", body, re.M):
            for rid in ids.split():
                out[rid].add(pr[1] if pr else "")
    return out


RETRACTED = SNAP / "retracted-claims.tsv"
PASSING = set()  # row ids passing in the last build()


def retracted():
    """`snapshots/retracted-claims.tsv`: `Parity:` trailers the honest grading no longer supports, each
    with its reason. Git history cannot be edited, so a retraction is the only way to drop a claim."""
    return {r["id"]: r["reason"] for r in tsv(RETRACTED)} if RETRACTED.is_file() else {}


def flagged_claims(rows):
    """Every `Parity:` claim in git history whose row does not currently pass (`tested` or `done`)
    and is not retracted with a reason."""
    claim = claims()
    passing = {r["id"] for r in rows if r["status"] in ("tested", "done")}
    gone = retracted()
    flagged = [
        (rid, ",".join(sorted(p for p in prs if p)))
        for rid, prs in claim.items()
        if rid not in passing and rid not in gone
    ]
    flagged.sort()
    return flagged, claim


def attribute(rows, refresh):
    """Fill each passing row's PR(s) and date from the attribution cache; with `refresh`, blame the rows
    it lacks or holds unmerged, add trailer claims, and rewrite it."""
    path = ATTRIBUTION
    known = {r["id"]: r for r in tsv(path)} if path.is_file() else {}
    if refresh:
        flagged, claim = flagged_claims(rows)
        new = {}
        for r in rows:
            if r["status"] not in ("tested", "done"):
                continue
            old = known.get(r["id"])
            prs = set(old["pr"].split(",")) - {""} if old else set()
            date = old["date"] if old else ""
            if not prs and r["ev"] and r["ev"][0]:
                sha = blame(*r["ev"])
                if sha and not sha.startswith("0000000"):
                    pr, date = commit(sha)
                    prs |= {pr} - {""}
            prs |= claim.get(r["id"], set()) - {""}
            new[r["id"]] = dict(id=r["id"], pr=",".join(sorted(prs, key=lambda p: (p[0] == "K", int(p.lstrip("K"))))), date=date)
        known = new
        path.write_text("id\tpr\tdate\n" + "".join(f"{v['id']}\t{v['pr']}\t{v['date']}\n" for v in sorted(new.values(), key=lambda v: v["id"])))
        FLAGGED.write_text("id\tpr\n" + "".join(f"{a}\t{b}\n" for a, b in flagged))
    else:
        flagged, _ = flagged_claims(rows)  # live from git history, never a stale local file
    for r in rows:
        a = known.get(r["id"]) if r["status"] in ("tested", "done") else None
        r["pr"], r["date"] = (a["pr"], a["date"]) if a else ("", "")
    return flagged


# ------------------------------------------------------------------ the board
AREAS = [  # area, title, format
    ("kontakt.ksp", "Scripting (KSP): commands, callbacks and variables", "Kontakt"),
    ("kontakt.par", "Script parameters: engine, control and zone", "Kontakt"),
    ("kontakt.ui", "Instrument panels: each control type and property", "Kontakt"),
    ("kontakt.dsp", "Effects and modulators", "Kontakt"),
    ("kontakt.format", "File loading: every saved object type", "Kontakt"),
    ("uvi.api", "Scripting (Lua API)", "UVI"),
    ("uvi.ui", "Instrument panels: widgets", "UVI"),
    ("uvi.dsp", "Effects and elements", "UVI"),
    ("uvi.par", "Parameters", "UVI"),
]


def counts(rows):
    c = collections.Counter(r["status"] for r in rows)
    n = len(rows)
    pct = lambda k: f"{100 * k / n:.1f}" if n else "0.0"
    return [n, *(c[s] for s in STATUSES), pct(c["tested"]), pct(c["tested"] + c["done"])]


def summary(rows):
    """[(label, counts)]: per area, DSP by processing type, then per format and overall."""
    out = []
    for area, title, _ in AREAS:
        rs = [r for r in rows if r["area"] == area]
        out.append((title, counts(rs)))
        if area.endswith(".dsp"):
            for t in sorted({r["group"] for r in rs}):
                out.append((f"  {area.split('.')[0].replace('kontakt', 'Kontakt').replace('uvi', 'UVI')} DSP: {t}",
                            counts([r for r in rs if r["group"] == t])))
    for fmt in ("Kontakt", "UVI"):
        out.append((f"**{fmt} overall**", counts([r for r in rows if r["area"].startswith(fmt.lower())])))
    out.append(("**All**", counts(rows)))
    return out + corpus_census()


def corpus_census():
    """Per-format corpus rows from snapshots/corpus-census.tsv: both `playable` (loads, samples, no
    fault, UI, sound) and `clean` (`playable` + 0 audible diagnostics, 0 unhandled actions, 0 opaque pars)."""
    path = SNAP / "corpus-census.tsv"
    if not path.is_file():
        return []
    c = {(r["format"], r["check"]): r for r in tsv(path)}
    out = []
    for fmt in ("Kontakt", "UVI"):
        play_key = "playable" if (fmt, "playable") in c else "green"
        if (fmt, play_key) in c:
            n = int(c[fmt, play_key]["instruments"])
            playable = int(c[fmt, play_key]["pass"])
            loads = int(c[fmt, "loads"]["pass"])
            pct = f"{100 * playable / n:.1f}" if n else "0.0"
            out.append((f"**{fmt} corpus** (playable)", [n, playable, 0, loads - playable, 0, n - loads, pct, pct]))
        if (fmt, "clean") in c:
            n = int(c[fmt, "clean"]["instruments"])
            clean = int(c[fmt, "clean"]["pass"])
            loads = int(c[fmt, "loads"]["pass"])
            pct = f"{100 * clean / n:.1f}" if n else "0.0"
            out.append((f"**{fmt} corpus** (clean: 0 audible/script drops)", [n, clean, 0, loads - clean, 0, n - loads, pct, pct]))
    return out


def corpus_line():
    path = SNAP / "corpus-census.tsv"
    if not path.is_file():
        return []
    rs = tsv(path)
    return [f"{fmt} corpus checks: " + ", ".join(
        f"{r['check']} {r['pass']}" + ("" if r["check"].startswith("load_s") else f"/{r['instruments']}")
        for r in rs if r["format"] == fmt) + " (snapshots/corpus-census.tsv)." for fmt in dict.fromkeys(r["format"] for r in rs)]


SNAPSHOTS = [
    ("ksp-manual.tsv", "the KSP manual's items", "tools/parity-board.py --ksp-manual KSP_Reference_Manual.txt (pdftotext -layout of the manual)"),
    ("ksp-status.tsv", "what KONTRA's VM and engine answer", "build-slot cargo run -p kontra-host --example ksp_status \\| LC_ALL=C sort -u > docs/parity/snapshots/ksp-status.tsv"),
    ("dsp-oracle.tsv", "passing v3 Rust DSP oracle tests", "checked by tools/parity-board.py --check against real non-ignored Rust tests"),
    ("ni-file-census.tsv", "the ni-file corpus census", "build-slot cargo run --release -p kontra-format-kontakt --example chunks -- <Kontakt library root> > census.md; tools/parity-board.py --census census.md"),
    ("ui-corpus.tsv", "UI areas that differ from Kontakt per corpus instrument", "tools/ksp-ui/align.py per instrument (README 'Library instruments'); one row per instrument: an opaque id and its count"),
    ("corpus-census.tsv", "per check, how many installed instruments pass (load, samples, script, UI, sound, playable, clean) and load time", "KONTRA_CENSUS_JOBS=4 tools/parity/census.sh <out> --snapshot (counts only, no library content)"),
    ("uvi-reference.tsv", "UVI Falcon's public script reference", f"tools/parity-board.py --uvi-reference ({UVI_DOCS})"),
    ("uvi-elements.tsv", "which UVI elements the loader maps or runs", "the UVI catalog lane (was tools/coverage/data/uvi-features.tsv)"),
    ("modulation-targets.tsv", "Kontakt's modulation targets", "docs/specs/modulation-laws.md lane"),
    ("overrides.tsv", "hand-set statuses, each with a reason", "by hand, with a reason"),
]


def esc(s):
    return str(s).replace("|", "\\|")


KONTRA_PR = "https://github.com/DerpcatMusic/KONTRA/pull/"


def pr_md(prs):
    """Public KONTRA PRs link; monorepo PR numbers are private, shown unlinked."""
    return ", ".join(f"[KONTRA#{p[1:]}]({KONTRA_PR}{p[1:]})" if p.startswith("K") else f"#{p}" for p in prs.split(",") if p)


def ev_link(ev):
    return f"[{ev[0].rsplit('/', 1)[-1]}]({REPO}{ev[0]})" if ev and ev[0] else ""


def render_md(rows, sums, fmt_extra, flagged):
    tot, latest, mism = fmt_extra
    out = ["# KONTRA parity board", "",
           "Every Kontakt and UVI Falcon item KONTRA must match, one checkable row each. Generated by "
           "`tools/parity-board.py` from the snapshots below and the repo; never edited or committed. A PR changes "
           "snapshots (rows in byte order) and evidence, and the site regenerates this board.", "",
           "**tested**: KONTRA does it and a real test, probe or oracle measurement verifies it. **done**: KONTRA does it "
           "(parameters require both verified `law` and `acts`). **partial**: some of it. **unseen**: reader exists, 0 corpus "
           "objects observed (not counted in passing). **missing**: not yet. Passing = tested + done. Parameter cells: law "
           "(set → internal value), display (internal → text), range (clamp), acts (drives sound or the UI).", "",
           "PR: the squash merge that last changed each row's evidence line (`Parity: <row-id>` trailers add claims). "
           "KONTRA#N is a PR of the public KONTRA repository from before it moved into the monorepo; #N is a monorepo PR.", "",
           "## Summary", "",
           "| Criterion | Items | tested | done | partial | unseen | missing | % tested | % passing |",
           "|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    out += [f"| {label} | " + " | ".join(map(str, c)) + " |" for label, c in sums]
    out += ["", f"ni-file corpus bytes: {tot.get('bytes', '?')} read, {tot.get('unaccounted', '?')} unaccounted for "
                f"(census snapshot); {latest if latest is not None else '?'} unaccounted after the latest batch in "
                f"docs/re/NI_FILE_PARITY.md.",
            f"UI corpus: {mism[0]} mismatched areas across {mism[1]} instruments (snapshots/ui-corpus.tsv).", *corpus_line(), "",
            "## Snapshots", "", "| File | What | Refresh |", "|---|---|---|"]
    out += [f"| `docs/parity/snapshots/{f}` | {w} | `{c}` |" for f, w, c in SNAPSHOTS]
    if flagged:
        out += ["", "## Claims that do not pass", "", "| Row | PR |", "|---|---|"]
        out += [f"| `{a}` | #{b} |" for a, b in flagged]
    for area, title, _ in AREAS:
        rs = [r for r in rows if r["area"] == area]
        out += ["", f"## {title}", ""]
        for g in sorted({r["group"] for r in rs}):
            grs = [r for r in rs if r["group"] == g]
            c = counts(grs)
            out += [f"### {g} ({c[1]} tested, {c[2]} done, {c[3]} partial, {c[4]} unseen, {c[5]} missing)", "",
                    "| Status | Row | Item | Evidence | PR | Changed | Note |", "|---|---|---|---|---|---|---|"]
            for r in grs:
                out.append(f"| {r['status']} | `{r['id']}` | {esc(r['name'])} | {ev_link(r['ev'])} | {pr_md(r['pr'])} | {r['date']} | {esc(r['note'])} |")
    return "\n".join(out) + "\n"


def render_html(rows, sums):
    data = [[r["id"], r["area"], r["group"], r["name"], r["status"], r["ev"][0] if r["ev"] and r["ev"][0] else "",
             r["pr"], r["date"], r["note"]] for r in rows]
    areas = [list(a) for a in AREAS]
    summ = [[label.replace("**", "").strip(), c, label.startswith("  ")] for label, c in sums]
    page = HTML.replace("__FRAGMENT__", FRAGMENT).replace("__DATA__", json.dumps(data, ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/"))
    page = page.replace("__AREAS__", json.dumps(areas)).replace("__SUMMARY__", json.dumps(summ)).replace("__REPO__", REPO)
    return page


FRAGMENT = """<!-- board:start --><div class="pboard"><style>
.pboard{--bg:#fafaf8;--fg:#1b1b1b;--mute:#6b6b6b;--line:#e2e2dc;--card:#fff;--tested:#2f7d4f;--done:#7aa95c;--partial:#d39b2a;--unseen:#7a8b99;--missing:#c4c4bc;--badge:#eef1f5;--badge-fg:#1f4f8a}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]) .pboard{--bg:#161616;--fg:#ececec;--mute:#9a9a9a;--line:#2c2c2c;--card:#1e1e1e;--missing:#4a4a4a;--badge:#1f2a38;--badge-fg:#8cb8f0}}
:root[data-theme="dark"] .pboard{--bg:#161616;--fg:#ececec;--mute:#9a9a9a;--line:#2c2c2c;--card:#1e1e1e;--missing:#4a4a4a;--badge:#1f2a38;--badge-fg:#8cb8f0}
.pboard,.pboard *{box-sizing:border-box}.pboard{color:var(--fg);font:15px/1.45 system-ui,sans-serif}
.pboard .heads{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:10px;margin:0 0 20px}
.pboard .head{border:1px solid var(--line);border-radius:8px;background:var(--card);padding:12px 14px}
.pboard .head b{display:block;font-size:28px;line-height:1.1;font-variant-numeric:tabular-nums}.pboard .head span{color:var(--mute);font-size:13px}
.pboard .bar{display:flex;height:8px;border-radius:4px;overflow:hidden;background:var(--missing);margin-top:8px}.pboard .bar span{display:block;height:100%}
.pboard .controls{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px;position:sticky;top:0;background:var(--bg);padding:8px 0;z-index:1}
.pboard .controls input,.pboard .controls select{font:inherit;padding:6px 8px;border:1px solid var(--line);border-radius:6px;background:var(--card);color:var(--fg)}
.pboard .controls input{flex:1;min-width:160px}.pboard h2{font-size:20px;margin:24px 0 8px}
.pboard details{border:1px solid var(--line);border-radius:8px;margin-bottom:8px;background:var(--card)}
.pboard summary{cursor:pointer;padding:10px 12px;display:grid;grid-template-columns:1fr minmax(80px,200px) auto;gap:12px;align-items:center}
.pboard summary .t{font-weight:600}.pboard summary small{color:var(--mute);font-variant-numeric:tabular-nums;white-space:nowrap}
.pboard .wrap{overflow-x:auto}.pboard table{border-collapse:collapse;width:100%;font-size:13px}
.pboard td,.pboard th{padding:6px 10px;border-top:1px solid var(--line);text-align:left;vertical-align:top}.pboard th{color:var(--mute);font-weight:500}
.pboard td.cat{color:var(--mute);white-space:nowrap}.pboard a{color:inherit}
.pboard .st{display:inline-block;white-space:nowrap;border-radius:999px;padding:1px 8px;font-size:12px;font-weight:600;color:#fff}
.pboard .st.tested{background:var(--tested)}.pboard .st.done{background:var(--done)}.pboard .st.partial{background:var(--partial)}.pboard .st.unseen{background:var(--unseen)}.pboard .st.missing{background:var(--missing);color:var(--fg)}
.pboard .pr{display:inline-block;margin:0 4px 2px 0;padding:1px 7px;border-radius:6px;background:var(--badge);color:var(--badge-fg);font:12px ui-monospace,monospace;text-decoration:none;white-space:nowrap}
.pboard .pr:hover{text-decoration:underline}.pboard .when{color:var(--mute);font-size:12px;white-space:nowrap}
.pboard .more{padding:8px 12px}.pboard .more button{font:inherit;padding:4px 10px}.pboard .none{padding:10px 12px;color:var(--mute)}
@media (max-width:600px){.pboard summary{grid-template-columns:1fr auto}.pboard summary .bar{display:none}.pboard td.cat,.pboard th.cat{display:none}}
</style>
<div class="heads" id="heads"></div>
<div class="controls"><input id="q" type="search" placeholder="Search features" aria-label="Search features"><select id="fmt" aria-label="Sampler"><option value="">Kontakt and UVI</option><option>Kontakt</option><option>UVI</option></select><select id="st" aria-label="Status"><option value="">Any status</option><option value="tested">Done and verified</option><option value="done">Done</option><option value="partial">Partial</option><option value="unseen">Unseen</option><option value="missing">Not done</option></select></div>
<div id="areas"></div>
<script>
const ROWS=__DATA__,AREAS=__AREAS__,SUM=__SUMMARY__,REPO="__REPO__";
const C={tested:"var(--tested)",done:"var(--done)",partial:"var(--partial)",unseen:"var(--unseen)"},L={tested:"Done ✓",done:"Done",partial:"Partial",unseen:"Unseen",missing:"Not done"};
const T={tested:"Done, and a test, probe or measurement against the original checks it",done:"Done with verified law",partial:"Some of it works",unseen:"Reader exists, 0 corpus objects observed",missing:"Not implemented yet"};
const e=s=>String(s).replace(/[&<>"]/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"})[c]);
const bar=c=>`<div class="bar" aria-hidden="true">${["tested","done","partial","unseen"].map((s,i)=>`<span style="width:${100*c[i+1]/(c[0]||1)}%;background:${C[s]}"></span>`).join("")}</div>`;
document.getElementById("heads").innerHTML=SUM.filter(([l])=>/overall|corpus/i.test(l)).map(([l,c])=>{const corpus=/corpus/i.test(l);
return `<div class="head"><b>${c[7]}%</b><span>${e(corpus?l:l.replace("overall","features done"))}: ${c[1]+c[2]} of ${c[0]}</span>${bar(c)}</div>`}).join("");
const prs=p=>p?p.split(",").map(n=>n[0]==="K"?`<a class="pr" href="https://github.com/DerpcatMusic/KONTRA/pull/${e(n.slice(1))}">KONTRA#${e(n.slice(1))}</a>`:`<a class="pr" href="https://github.com/DerpcatMusic/KONTRA/search?q=%23${e(n)}&type=commits" title="Commits from change #${e(n)}">#${e(n)}</a>`).join(""):"";
const PAGE=200;
function match(r,q,st,fmt,a){return r[1]===a&&(!st||r[4]===st)&&(!q||(r[0]+" "+r[2]+" "+r[3]+" "+r[8]).toLowerCase().includes(q))}
function draw(){const q=document.getElementById("q").value.toLowerCase(),st=document.getElementById("st").value,fmt=document.getElementById("fmt").value;let html="",last="";
for(const [a,t,f] of AREAS){if(fmt&&f!==fmt)continue;const all=ROWS.filter(r=>r[1]===a),rs=all.filter(r=>match(r,q,st,fmt,a));if((q||st)&&!rs.length)continue;
if(f!==last){html+=`<h2>${e(f)}</h2>`;last=f}const c=[all.length,...["tested","done","partial","unseen","missing"].map(s=>all.filter(r=>r[4]===s).length)];
html+=`<details data-a="${a}"${q||st?" open":""}><summary><span class="t">${e(t)}</span>${bar(c)}<small>${c[1]+c[2]} of ${c[0]} done${q||st?` · ${rs.length} shown`:""}</small></summary><div class="wrap"><table><thead><tr><th>Feature</th><th class="cat">Category</th><th>Status</th><th>Changes</th><th>Proof</th><th>Note</th></tr></thead><tbody data-n="0"></tbody></table></div><div class="more"></div></details>`}
document.getElementById("areas").innerHTML=html||'<p class="none">No features match.</p>';
document.querySelectorAll("#areas details").forEach(d=>{const fill=()=>{const a=d.dataset.a,tb=d.querySelector("tbody");const rs=ROWS.filter(r=>match(r,q,st,fmt,a));
const n=+tb.dataset.n,m=Math.min(rs.length,n+PAGE);tb.insertAdjacentHTML("beforeend",rs.slice(n,m).map(r=>`<tr id="${e(r[0])}"><td title="${e(r[0])}">${e(r[3])}</td><td class="cat">${e(r[2])}</td><td><span class="st ${r[4]}" title="${T[r[4]]}">${L[r[4]]}</span></td><td>${prs(r[6])}${r[7]?`<span class="when">${e(r[7])}</span>`:""}</td><td>${r[5]?`<a href="${REPO}${e(r[5])}">${e(r[5].split("/").pop())}</a>`:""}</td><td>${e(r[8])}</td></tr>`).join(""));
tb.dataset.n=m;const more=d.querySelector(".more");more.innerHTML=m<rs.length?`<button>Show ${Math.min(PAGE,rs.length-m)} more of ${rs.length-m}</button>`:"";const b=more.querySelector("button");if(b)b.onclick=fill};
if(d.open)fill();else d.addEventListener("toggle",()=>{if(d.open&&d.querySelector("tbody").dataset.n==="0")fill()},{once:true})})}
for(const id of ["q","st","fmt"])document.getElementById(id).addEventListener(id==="q"?"input":"change",draw);draw();
</script><noscript><p>The table needs JavaScript; <a href="https://github.com/DerpcatMusic/KONTRA/blob/main/docs/PARITY.md">the same list as Markdown</a>.</p></noscript></div><!-- board:end -->"""

HTML = """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>KONTRA Parity Board</title>
<meta name="description" content="Every Kontakt and UVI Falcon function KONTRA must match, each with its status.">
<style>:root{--page:#fafaf8;--ink:#1b1b1b;--soft:#6b6b6b}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--page:#161616;--ink:#ececec;--soft:#9a9a9a}}
:root[data-theme="dark"]{--page:#161616;--ink:#ececec;--soft:#9a9a9a}
body{margin:0;background:var(--page);color:var(--ink);font:15px/1.45 system-ui,sans-serif}
main{max-width:1100px;margin:0 auto;padding:24px 16px 64px}h1{margin:0 0 4px;font-size:26px}p.lede{color:var(--soft);margin:0 0 20px}</style>
</head><body><main id="main">
<h1>KONTRA parity board</h1>
<p class="lede">Every function of Kontakt and of UVI Falcon that KONTRA must match, one checkable row each, with what proves it and the PR that brought it. Generated from the repository by tools/parity-board.py.</p>
__FRAGMENT__
</main></body></html>
"""


# ------------------------------------------------------------------ snapshot refreshers
SECTIONS = ["General Commands", "Array Commands", "Engine Parameter Commands", "Event Commands", "Group Commands",
            "Keyboard Commands", "Load/Save Commands", "MIDI Object Commands", "Music Information Retrieval",
            "Time-Related Commands", "User Interface Commands", "Zone Commands", "Built-in Variables and Constants",
            "Control Parameters", "Engine Parameters", "Zone Parameters", "Callbacks", "Variables",
            "Arithmetic Commands & Operators", "Control Statements", "User-defined Functions", "User Interface Widgets",
            "Multi Script", "Advanced Concepts"]


def refresh_ksp_manual(path):
    """The manual's items: a command heading (`name()`), a callback (`on name`), a `$`/`%` name that opens a
    line or bullet, and each UI widget; a control parameter carries the widgets its description names."""
    manual = pathlib.Path(path).read_text(errors="ignore").splitlines()
    whole = "\n".join(manual)
    spelled = {"$ENGINE_PAR_SF100_OVEDRIVE": "$ENGINE_PAR_SF100_OVERDRIVE"}
    kontakt = set(read("crates/script/ksp/data/kontakt-8.13.1-names.txt").split())
    at = {}
    for i, line in enumerate(manual):
        m = re.match(r"^\d+\. (.+?)\s*$", line)
        if m and m[1] in SECTIONS and i > 300:
            at[m[1]] = i
    starts = sorted((i, t) for t, i in at.items())
    section = lambda i: max(((s, t) for s, t in starts if s <= i), default=(0, "Front"))[1]
    declared = {m[1].lower() for m in re.finditer(
        r"(?:declare|decalre)\s+(?:(?:const|polyphonic|global|local|read|pers|instpers|ui_\w+)\s+)*[$%~@!?]?([A-Za-z_]\w*)", whole)}
    items = {}
    fn, var, cb = re.compile(r"^([a-z_][a-z_0-9]*)\(\)\s*$"), re.compile(r"^[\s•\-\*]*([$%~@!][A-Za-z_][A-Za-z_0-9]*)"), re.compile(r"^\s*on ([a-z_]+)")
    sig = re.compile(r"^ ?(?:<\w+> := )?([a-z_][a-z_0-9]*)\(<")
    last_par = None
    for i, line in enumerate(manual):
        sec = section(i)
        if sec == "Front":
            continue
        if sec == "User Interface Widgets" and re.fullmatch(r"ui_[a-z_]+", line.strip()):
            items.setdefault(("widget", line.strip()), [sec, ""])
        if (m := fn.match(line)) or ((sec.endswith("Commands") or sec == "Advanced Concepts") and (m := sig.match(line))):
            items.setdefault(("fn", m[1]), [sec, ""])
        elif m := var.match(line):
            name = m[1]
            tail = re.match(r"^([A-Z_0-9]+)(?:\s|$)", manual[i + 1] if i + 1 < len(manual) else "")
            if tail and name.isupper() and name + tail[1] in whole:
                name += tail[1]
            name = spelled.get(name, name)
            bare = name.lstrip("$%~@!")
            if name.endswith(("_", "#")) or (len(bare) >= 4 and bare not in kontakt):
                continue
            up = name.upper()
            kind = ("par" if "ENGINE_PAR" in up else "evpar" if "EVENT_PAR" in up else
                    "uipar" if up.startswith(("$CONTROL_PAR", "$ZONE_PAR")) else
                    "var" if up.startswith(("$NI_", "$EVENT_", "$KEY_", "$MARK_", "$NUM_", "$CURRENT_", "%")) else "sym")
            if sec in ("Variables", "Built-in Variables and Constants", "Engine Parameters", "Control Parameters",
                       "Zone Parameters", "Event Commands", "General Commands", "Callbacks") or kind in ("par", "evpar", "uipar"):
                if kind in ("sym", "var") and bare.lower() in declared and bare.upper() not in kontakt:
                    continue
                k = (kind, bare.lower() if kind == "var" else name)
                items.setdefault(k, [sec, ""])
                last_par = k if kind == "uipar" and sec == "Control Parameters" else None
        elif m := cb.match(line):
            if sec == "Callbacks":
                items.setdefault(("cb", m[1]), [sec, ""])
        elif last_par and sec == "Control Parameters":
            ws = set(re.findall(r"\bui_[a-z_]+", line)) - {"ui_control", "ui_controls", "ui_update", "ui_id"}
            if ws:
                items[last_par][1] = ",".join(sorted(set(filter(None, items[last_par][1].split(","))) | ws))
    out = ["kind\tname\tsection\tapplies"] + sorted(f"{k}\t{n}\t{s}\t{a}" for (k, n), (s, a) in items.items())
    (SNAP / "ksp-manual.tsv").write_text("# The KSP Reference Manual's items (names only), from tools/parity-board.py --ksp-manual\n"
                                         + "\n".join(out) + "\n")
    print(f"{len(items)} items -> {SNAP / 'ksp-manual.tsv'}")


def refresh_uvi_reference(src):
    """UVI's public script reference: functions per group, class members, elements and their parameters."""
    def page(name):
        if src:
            return (pathlib.Path(src) / name).read_text(errors="ignore")
        return subprocess.run(["curl", "-sSL", "-A", "Mozilla/5.0", UVI_DOCS + name], capture_output=True, text=True).stdout
    member = re.compile(r'<td class="memItemRight"[^>]*>(?:<a [^>]*>)?([A-Za-z_]\w*)')
    out = ["kind\tgroup\towner\tname\tdisplay\tsource"]
    groups = {"group___async": "Async", "group___context": "Context", "group___conversions": "Conversions",
              "group___event_callbacks": "Event callbacks", "group___events": "Events", "group___m_i_d_i": "MIDI",
              "group___time": "Time", "group___voice": "Voice", "group___u_i": "UI"}
    for g, label in groups.items():
        seen = set()
        for n in member.findall(page(g + ".html")):
            if n not in seen and not n[0].isupper():
                seen.add(n)
                out.append(f"function\t{label}\t\t{n}\t{n}\t{UVI_DOCS}{g}.html")
    crawl = page("doxygen_crawl.html")
    classes = sorted(set(re.findall(r'href="(class_[a-z_0-9]+)\.html"', crawl)))
    base = set(member.findall(page("class_element.html")))
    widget_base = set(member.findall(page("class_widget.html")))
    for c in classes:
        text = page(c + ".html")
        title = re.search(r"<title>[^:]*:\s*(\w+)", text)
        owner = title[1] if title else c
        group = "UI" if re.search(r"Widget|group___u_i", text) else "API"
        inherited = (base if owner != "Element" else set()) | (widget_base if owner != "Widget" else set())
        seen = set()
        for n in member.findall(text):
            if n in seen or (n in inherited and owner not in ("Element", "Widget")):
                continue
            seen.add(n)
            out.append(f"member\t{group}\t{owner}\t{n}\t{n}\t{UVI_DOCS}{c}.html")
    el = page("_elements.html")
    body = el[el.find('<h1><a class="anchor" id="ElemFX"'):]
    cat = owner = None
    for m in re.finditer(r'<h1><a class="anchor" id="\w+"></a>\s*([^<]+)</h1>|<h2><a class="anchor" id="(\w+)"></a>\s*([^<]+)</h2>|'
                         r"<tr>\s*<td>([^<]*)</td><td>([^<]*)</td>", body):
        if m[1]:
            cat = m[1].strip()
        elif m[2]:
            owner = m[2]
            out.append(f"element\t{cat}\t\t{owner}\t{html.unescape(m[3].strip())}\t{UVI_DOCS}_elements.html#{owner}")
        elif owner and m[4].strip():
            out.append(f"param\t{cat}\t{owner}\t{m[4].strip()}\t{html.unescape(m[5].strip())}\t{UVI_DOCS}_elements.html#{owner}")
    out[1:] = sorted(set(out[1:]))
    (SNAP / "uvi-reference.tsv").write_text(f"# UVI script reference ({UVI_DOCS}), names only, from tools/parity-board.py --uvi-reference\n"
                                            + "\n".join(out) + "\n")
    print(f"{len(out) - 1} items -> {SNAP / 'uvi-reference.tsv'}")


def refresh_census(path):
    """The chunks example's per-class table (its markdown) as a TSV, with the whole-corpus totals."""
    text = pathlib.Path(path).read_text()
    out = ["sertype\tversion\tclass\tobjects\tfull\tbytes\tunaccounted"]
    tot = re.search(r"(\d+) objects: (\d+) full, (\d+) partial, (\d+) unrecognised\. Bytes (\d+), unaccounted for (\d+)", text)
    if tot:
        out.append(f"total\t\t\t{tot[1]}\t{tot[2]}\t{tot[5]}\t{tot[6]}")
    for line in text.splitlines():
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(c) == 10 and c[0].startswith("0x"):
            out.append("\t".join([c[0], c[1], c[2], c[3], c[4], c[7], c[8]]))
    out[1:] = sorted(out[1:])
    (SNAP / "ni-file-census.tsv").write_text("\n".join(out) + "\n")
    print(f"{len(out) - 1} rows -> {SNAP / 'ni-file-census.tsv'}")


# ------------------------------------------------------------------ main
def snapshot_errors():
    """Each snapshot: every row as wide as its header, rows in byte order (`LC_ALL=C sort`) and unique,
    plus `dsp-oracle.tsv` test-existence and passing-status validation."""
    errors = []
    for p in sorted(SNAP.glob("*.tsv")):
        lines = [l for l in p.read_text().splitlines() if l and not l.startswith("#")]
        body = lines if p.name == "ksp-status.tsv" else lines[1:]  # ksp-status has no header
        width = 3 if p.name == "ksp-status.tsv" else len(lines[0].split("\t"))
        errors += [f"{p.name}: {l!r} has {len(l.split(chr(9)))} columns, not {width}" for l in body if len(l.split("\t")) != width]
        if body != sorted(set(body)):
            errors.append(f"{p.name}: rows are not sorted and unique (keep the header, LC_ALL=C sort -u the rows)")
    if (SNAP / "dsp-oracle.tsv").is_file():
        errors.extend(dsp_oracle_errors())
    return errors


def build(refresh):
    manual, status = ksp_inputs()
    ksp, pars = kontakt_ksp_and_params(manual, status)
    ui, corpus = kontakt_ui(manual)
    fmt, totals, latest = kontakt_format()
    rows = ksp + pars + modulation_targets() + ui + kontakt_dsp(status) + fmt + uvi()
    seen = set()
    for r in rows:
        while r["id"] in seen:
            r["id"] += "_"
        seen.add(r["id"])
    apply_overrides(rows)
    flagged = attribute(rows, refresh)
    PASSING.clear()
    PASSING.update(r["id"] for r in rows if r["status"] in ("tested", "done"))
    mism = (sum(int(r["areas"]) for r in corpus), len(corpus))
    sums = summary(rows)
    return render_md(rows, sums, (totals, latest, mism), flagged), render_html(rows, sums), sums, flagged


def main():
    args = sys.argv[1:]
    if args[:1] == ["--ksp-manual"]:
        return refresh_ksp_manual(args[1])
    if args[:1] == ["--uvi-reference"]:
        return refresh_uvi_reference(args[1] if len(args) > 1 else None)
    if args[:1] == ["--census"]:
        return refresh_census(args[1])
    if args[:1] == ["--check"]:
        errors = snapshot_errors()
        if not (SNAP / "dsp-oracle.tsv").is_file():
            errors.append("dsp-oracle.tsv: missing required manifest")
        if errors:
            sys.exit("\n".join(errors))
        _, _, _, flagged = build(refresh=False)
        if flagged:
            details = ", ".join(f"{rid} (#{pr})" if pr else rid for rid, pr in flagged)
            sys.exit(f"flagged-claims.tsv is non-empty ({len(flagged)} failing claims): {details}")
        stale = sorted(set(retracted()) & PASSING)
        if stale:
            sys.exit(f"retracted-claims.tsv: these rows pass again, delete their retraction: {', '.join(stale)}")
        print("parity snapshots are well-formed, dsp-oracle.tsv verified, and the board generates with 0 flagged claims")
        return
    md, page, sums, flagged = build(refresh=True)
    OUT_MD.write_text(md)
    OUT_HTML.write_text(page)
    for label, c in sums:
        print(f"{label:48} {c[0]:6} tested {c[1]:5} done {c[2]:5} partial {c[3]:5} unseen {c[4]:4} missing {c[5]:5}  {c[6]}% / {c[7]}%")
    if flagged:
        print(f"WARNING: {len(flagged)} flagged claims written to {FLAGGED.relative_to(ROOT)}", file=sys.stderr)


if __name__ == "__main__":
    main()
