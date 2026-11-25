#!/usr/bin/env python3
"""The parity board: every Kontakt and UVI Falcon item KONTRA must match, with its status.

    tools/parity-board.py                  regenerate docs/PARITY.md and docs/PARITY.html (and blame new rows)
    tools/parity-board.py --check          fail when the checked-in board is out of date with its inputs
    tools/parity-board.py --ksp-manual KSP_Reference_Manual.txt    refresh snapshots/ksp-manual.tsv
    tools/parity-board.py --uvi-reference [DIR]   refresh snapshots/uvi-reference.tsv from lua.uvi.net
                                                  (or from a directory of its saved pages)
    tools/parity-board.py --census CENSUS.md      refresh snapshots/ni-file-census.tsv from the chunks example

No cargo and no oracle: inputs that need either are the checked-in snapshots under
docs/parity/snapshots/, refreshed by the lane that changes them (the board's header lists the
commands). Every status is derived from the repo: tests, probes, law tables, the census.
snapshots/overrides.tsv is the only hand-typed layer, and every row of it gives a reason.

Statuses: tested (KONTRA does it and a test, probe or oracle measurement checks it), done (KONTRA
does it), partial (some of it), missing. "Passing" is tested + done.

Each passing row names the PR that brought its evidence: the squash-merge `(#NNN)` of the commit
that last changed the evidence line (git blame, cached by file content), or a `Parity: <row-id>...`
trailer in a commit message. A claim on a row that does not pass is listed, not trusted.
Attributions are kept in snapshots/attribution.tsv so --check needs no git history.
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
OUT_MD = ROOT / "docs/PARITY.md"
OUT_HTML = ROOT / "docs/PARITY.html"
REPO = "https://github.com/DerpcatMusic/KONTRA/blob/main/"
UVI_DOCS = "https://lua.uvi.net/"
STATUSES = ("tested", "done", "partial", "missing")
CACHE = pathlib.Path(os.environ.get("XDG_CACHE_HOME", pathlib.Path.home() / ".cache")) / "kontra-parity-blame"


def read(rel):
    return (ROOT / rel).read_text(errors="ignore")


def tsv(path, header=True):
    lines = [l.rstrip("\n").split("\t") for l in pathlib.Path(path).read_text().splitlines() if l and not l.startswith("#")]
    return [dict(zip(lines[0], r)) for r in lines[1:]] if header else lines


def files(*globs):
    return sorted(p for g in globs for p in ROOT.glob(g) if p.is_file() and "target" not in p.parts)


class Index:
    """Lower-cased words per file, to find the first file that names a word."""

    def __init__(self, paths):
        self.words = [(p.relative_to(ROOT).as_posix(), set(re.findall(r"[a-z_0-9]+", p.read_text(errors="ignore").lower())))
                      for p in paths]

    def find(self, *words):
        for path, ws in self.words:
            if any(w.lower() in ws for w in words):
                return path
        return None


class Titles:
    """A measurement is of a module when its file name or its first line names it."""

    def __init__(self, paths):
        self.heads = [(p.relative_to(ROOT).as_posix(), (p.stem + " " + p.read_text(errors="ignore").lstrip().split("\n", 1)[0]).lower())
                      for p in paths]

    def find(self, word):
        word = word.lower()
        return next((path for path, head in self.heads if re.search(rf"(?<![a-z]){re.escape(word)}", head)), None)


def row(rid, area, group, name, status, ev=None, note=""):
    return dict(id=rid, area=area, group=group, name=name, status=status, ev=ev, note=note)


def grade(cells):
    """A parameter's status from its applicable cells (True/False; None is not applicable)."""
    cells = [c for c in cells if c is not None]
    if all(cells):
        return "tested"
    if cells[-1]:  # the last cell is the one that acts: it drives sound or the UI
        return "done"
    return "partial" if any(cells) else "missing"


# ------------------------------------------------------------------ shared inputs
def ksp_inputs():
    manual = tsv(SNAP / "ksp-manual.tsv")
    status = {}
    for kind, name, st in tsv(SNAP / "ksp-status.tsv", header=False):
        status[(kind, name.lower() if kind != "par" else name.lstrip("$").upper())] = st
    return manual, status


# ------------------------------------------------------------------ Kontakt: KSP and parameters
def kontakt_ksp_and_params(manual, status):
    """The KSP manual's items (was tools/ksp-manual-coverage.py), split: language items, then parameters."""
    tsv_consts = read("crates/script/ksp/data/constants.tsv")
    valued = {l.split("\t")[0].lstrip("$").upper() for l in tsv_consts.splitlines() if l and l[0] != "#"}
    valued |= {n.lstrip("~").upper() for n in re.findall(r'"(~NI_[A-Z_]+)"', read("crates/script/ksp/src/consts.rs"))}
    vendor = {l.strip().lstrip("$%~@!").upper() for l in read("crates/script/ksp/data/vendor_constants.txt").splitlines()
              if l.strip() and l[0] != "#"}
    tests = Index([p for p in files("crates/script/ksp/tests/**/*", "crates/app/host/tests/**/*", "tools/ksp-conformance/**/*",
                                    "tools/ksp-ui/**/*") if p.suffix in (".rs", ".ksp", ".lua", ".py") and
                   re.search("ksp", p.read_text(errors="ignore"), re.I)]
                  + [p for p in files("tools/kontakt-oracle/probes/ksp/*.kspp") if p.with_suffix(".measured").is_file()])
    disp_rows = read("docs/re/kontakt-bindings/engine-par-disp.tsv").splitlines()
    pending = {n for l in disp_rows if l.startswith("# pending:") for n in l[len("# pending:"):].split()}
    disp = {l.split("\t")[0].lstrip("$").upper() for l in disp_rows if l and not l.startswith("#")} - {p.lstrip("$").upper() for p in pending}
    disp_file = "docs/re/kontakt-bindings/engine-par-disp.tsv"
    ui_paths = [p for d in ("crates/app/face/src", "crates/app/ui/src", "crates/engine/script-ui/src", "crates/app/host/src")
                for p in sorted((ROOT / d).rglob("*.rs"))]
    ui_text = "\n".join(p.read_text(errors="ignore") for p in ui_paths)
    ui_actions = set(re.findall(r"Action::(\w+)", ui_text))
    ui_names = set(re.findall(r'"\$((?:CONTROL_PAR|UI_WF_PROP|UI_WAVEFORM|NI_MOUSE)[A-Z0-9_]*)"', ui_text))
    src = Index(files("crates/script/ksp/src/**/*.rs", "crates/engine/vm/src/**/*.rs",
                      "crates/engine/engine/src/**/*.rs", "crates/engine/script-ui/src/**/*.rs", "crates/app/face/src/**/*.rs"))
    plan = read("crates/engine/plan/src/script.rs")
    ran = set(re.findall(r"pub (\w+):", plan[plan.index("pub struct Callbacks"):].split("\n}", 1)[0]))
    CB = {"init": "init", "note": "note", "release": "release", "controller": "controller", "poly_at": "polyat",
          "ui_control": "uicontrol", "ui_controls": "uicontrols", "ui_update": "uiupdate", "listener": "listener",
          "pgs_changed": "pgschanged", "persistence_changed": "persistencechanged", "async_complete": "asynccomplete",
          "rpn": "rpn", "nrpn": "nrpn", "note_controller": "notecontroller", "midi_in": "midiin"}
    # Law tables: a $ENGINE_PAR whose set -> stored value Kontakt's oracle measured.
    law_files = [p for p in files("docs/re/kontakt-bindings/*.tsv") if p.name != "engine-par-disp.tsv"]
    law = Index(law_files)
    clamp = Index([ROOT / "docs/re/kontakt-bindings/engine_par_clamp.tsv"])
    measured_probes = Index([p for p in files("tools/kontakt-oracle/probes/ksp/*.kspp", "tools/ksp-ui/probes/*.ksp")
                             if p.with_suffix(".measured").is_file()])

    ksp, pars = [], []
    for item in manual:
        kind, name, sec = item["kind"], item["name"], item["section"]
        key = name.lower().lstrip("$%~@!")
        bare = name.lstrip("$%~@!").upper()
        if kind == "widget":
            continue
        if kind == "par":  # $ENGINE_PAR_*: value law, display text, range, drives sound
            law_ev = law.find("engine_par_" + key.removeprefix("engine_par_"), key)
            cells = [law_ev is not None, bare in disp, clamp.find(key) is not None, ("par", bare) in status]
            ev = (law_ev, name) if law_ev else (disp_file, name) if cells[1] else None
            if not ev and cells[3]:  # carried: the engine answers it (ksp_status)
                ev = (src.find(key), name) if src.find(key) else ("docs/parity/snapshots/ksp-status.tsv", bare)
            pars.append(row(f"kontakt.par.{bare}", "kontakt.par", "$ENGINE_PAR", name, grade(cells), ev, cells_note(cells)))
            continue
        if kind in ("uipar", "evpar"):  # $CONTROL_PAR_*, $ZONE_PAR_*, $EVENT_PAR_*: measured law, acts
            if bare.startswith("ZONE_PAR"):
                acts = ("par", bare) in status
            else:
                acts = bare in ui_names or bare in valued or any(("fn", p + key) in status for p in ("", "$", "%"))
            probe = measured_probes.find(key)
            tested = tests.find(key)
            cells = [probe is not None or tested is not None, None, None, acts]
            group = "$ZONE_PAR" if bare.startswith("ZONE_PAR") else "$EVENT_PAR" if kind == "evpar" else "$CONTROL_PAR"
            ev = (probe or tested, name) if (probe or tested) else (src.find(key), name) if acts and src.find(key) else None
            pars.append(row(f"kontakt.par.{bare}", "kontakt.par", group, name, grade(cells), ev, cells_note(cells)))
            continue
        ev = None
        if kind == "fn":
            st = status.get(("fn", key))
            got = {"vm": "done", "answered": "done", "partial": "partial", "missing": "partial"}.get(st, "missing")
            if got == "partial" and "".join(w.capitalize() for w in key.split("_")) in ui_actions:
                got = "done"
            group, rid = "commands", f"kontakt.ksp.cmd.{key}"
        elif kind == "cb":
            got = "missing" if name not in CB else "partial" if name not in ran else "done"
            group, rid = "callbacks", f"kontakt.ksp.cb.{key}"
        else:
            known_fn = any(("fn", p + key) in status for p in ("", "$", "%"))
            got = "done" if known_fn or bare in ui_names or bare in valued else "partial" if bare in vendor else "missing"
            group = "built-in variables" if kind == "var" else "constants and names"
            rid = f"kontakt.ksp.{'var' if kind == 'var' else 'const'}.{bare}"
        if got == "done":
            t = tests.find(key)
            if t:
                got, ev = "tested", (t, key)
            elif src.find(key):
                ev = (src.find(key), key)
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
        ev = ("crates/format/kontakt/src", p) if cells[3] else ("docs/parity/snapshots/modulation-targets.tsv", p)
        ev = (find_in_dir("crates/format/kontakt/src", f'"{p}"'), p) if cells[3] else ev
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
    """Every serialized class Kontakt 8.13.1 registers, by the corpus census: tested = every object of it on
    the corpus is read in full; partial = some bytes unaccounted for; done = a reader exists but the corpus
    holds none; missing = no stream reader."""
    census = collections.defaultdict(lambda: [0, 0, 0, 0])
    totals = {}
    for r in tsv(SNAP / "ni-file-census.tsv"):
        if r["sertype"] == "total":
            totals = r
            continue
        c = census[r["class"]]
        for i, k in enumerate(("objects", "full", "bytes", "unaccounted")):
            c[i] += int(r[k])
    cov = {}
    for line in read("docs/re/kontakt-catalog/parser_coverage.md").splitlines():
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(c) == 6 and c[0].startswith("0x"):
            cov[c[1]] = c
    out = []
    for r in tsv(ROOT / "docs/re/kontakt-catalog/ser_classes.tsv"):
        full = r["class"]
        name = re.sub(r"^(class|struct) ", "", full)
        c = census.get(name)
        if c and c[0]:
            st = "tested" if c[0] == c[1] else "partial"
            note = f"{c[1]}/{c[0]} objects full, {c[3]} of {c[2]} bytes unaccounted"
            ev = ("docs/parity/snapshots/ni-file-census.tsv", name)
        else:
            cv = cov.get(full, ["", "", "", "", "", ""])
            stub = "no stream reader" in cv[5] or cv[3] in ("stub", "not-a-reader", "")
            st = "missing" if stub and not cv[4] else "done"
            note = "not in the corpus" + ("" if st == "done" else ", no reader")
            ev = ("docs/re/kontakt-catalog/parser_coverage.md", name) if st == "done" else None
        out.append(row(f"kontakt.format.{r['sertype']}.{re.sub(r'[^A-Za-z0-9]+', '_', name).strip('_')}", "kontakt.format",
                       "serialized classes", f"{r['sertype']} {name}", st, ev, note))
    # The latest whole-corpus totals NI_FILE_PARITY.md records (each batch's "After" row).
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


def kontakt_dsp(status):
    """Every effect module Kontakt's files can hold (the MODULES table of crates/format/kontakt/src/effects.rs) and
    every modulator class, three rows each: in the IR (decoded to an ir::EffectKind / translated), driven at run
    time (script and automation moves rebuild it live), and its output measured against the oracle (a gate or
    response measurement under docs/re/engine/fx-measured or docs/re/kontakt-bindings names it)."""
    src = read("crates/format/kontakt/src/effects.rs")
    table = src[src.index("const MODULES"):src.index("];", src.index("const MODULES"))]
    measured = Titles(files("docs/re/engine/fx-measured/*.md", "docs/re/kontakt-bindings/*-responses*.tsv",
                            "docs/re/kontakt-bindings/limiter-dynamics.tsv", "docs/re/kontakt-bindings/saturation-curves.tsv",
                            "docs/re/kontakt-bindings/filter-laws.tsv"))
    out = []
    for m in re.finditer(r'(fx|model)\((0x[0-9a-f]+|&crate::(\w+)::(\w+)), "([^"]+)", [^\n]*?(Read|Unread|\|s\|)', table):
        kind, name = m[1], m[5]
        words = [w for w in re.findall(r"[a-z0-9]+", name.lower()) if w not in ("legacy",)]
        slug = "_".join(words)
        t = ptype(name)
        ir_ok = m[6] != "Unread"
        ev = ("crates/format/kontakt/src/effects.rs", name)
        out.append(row(f"kontakt.dsp.{slug}.ir", "kontakt.dsp", t, f"{name}: in IR", "done" if ir_ok else "missing", ev if ir_ok else None))
        live = kind == "model"
        out.append(row(f"kontakt.dsp.{slug}.driven", "kontakt.dsp", t, f"{name}: driven live",
                       "done" if live else "partial" if ir_ok else "missing", ev if ir_ok else None,
                       "" if live else "load-time only" if ir_ok else ""))
        hit = measured.find(words[0]) if ir_ok and words else None
        out.append(row(f"kontakt.dsp.{slug}.measured", "kontakt.dsp", t, f"{name}: output vs oracle",
                       "tested" if hit else "missing", (hit, words[0]) if hit else None,
                       "threshold in the cited measurement" if hit else ""))
    code = read_dir("crates/format/kontakt/src")
    voice = read_dir("crates/engine/voice/src") + read_dir("crates/engine/engine/src")
    oracle = Titles(files("docs/re/engine/fx-measured/*.md", "tools/kontakt-oracle/probes/*.lua"))
    for r in tsv(ROOT / "docs/re/kontakt-catalog/ser_classes.tsv"):
        cls = re.sub(r"^(class|struct) ", "", r["class"])
        if not re.fullmatch(r"BPar(LFO|Env_\w+|EnvF|Glide|Arp|ExternalMod|InternalMod)|BZoneLevelEnv", cls):
            continue
        token = cls.split("_")[-1] if "_" in cls else cls.removeprefix("BPar").removeprefix("B")
        t = "envelope" if "Env" in cls else "LFO" if "LFO" in cls else "modulation source"
        ir_ok = re.search(rf"\b{token}\b", code, re.I) is not None
        drv = ir_ok and re.search(rf"\b{token}\b", voice, re.I) is not None
        hit = oracle.find(token)
        ev = (find_in_dir("crates/format/kontakt/src", token) or "crates/format/kontakt/src", token)
        out.append(row(f"kontakt.dsp.{token.lower()}.ir", "kontakt.dsp", t, f"{cls}: in IR", "done" if ir_ok else "missing", ev if ir_ok else None))
        out.append(row(f"kontakt.dsp.{token.lower()}.driven", "kontakt.dsp", t, f"{cls}: driven",
                       "done" if drv else "partial" if ir_ok else "missing", ev if ir_ok else None))
        out.append(row(f"kontakt.dsp.{token.lower()}.measured", "kontakt.dsp", t, f"{cls}: output vs oracle",
                       "tested" if hit and ir_ok else "missing", (hit, token) if hit and ir_ok else None))
    return out


# ------------------------------------------------------------------ UVI Falcon
def uvi():
    """UVI Falcon from its public script reference (snapshots/uvi-reference.tsv, cited per row): script API
    functions and class members, UI widgets and their members (the probe crates/script/lua/tests/uvi_ui
    checks each), elements by processing type (in IR / driven / measured), and every element parameter."""
    ref = tsv(SNAP / "uvi-reference.tsv")
    lua_src = "\n".join(read(p) for p in ("crates/script/lua/src/prelude.lua", "crates/script/lua/src/widgets.lua")) \
        + read_dir("crates/script/lua/src")
    lua_tests = Index(files("crates/script/lua/tests/**/*", "crates/format/uvi/src/program/tests.rs"))
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
            t = lua_tests.find(name)
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
            # value law: KONTRA reads the attribute; range: it is clamped or ranged where read; acts: its element is driven.
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
        subject, body = entry.strip("\n").split("\x1f", 1)  # not strip(): Python counts \x1f as whitespace
        pr = re.search(r"\(#(\d+)\)\s*$", subject)
        for ids in re.findall(r"^Parity:\s*(.+)$", body, re.M):
            for rid in ids.split():
                out[rid].add(pr[1] if pr else "")
    return out


def attribute(rows, refresh):
    """Fill each passing row's PR(s) and date from snapshots/attribution.tsv; with `refresh`, blame the rows
    it lacks or holds unmerged (no PR number yet), add trailer claims, and rewrite it."""
    path = SNAP / "attribution.tsv"
    known = {r["id"]: r for r in tsv(path)} if path.is_file() else {}
    flagged = []
    if refresh:
        claim = claims()
        passing = {r["id"] for r in rows if r["status"] in ("tested", "done")}
        for rid, prs in claim.items():
            if rid not in passing:
                flagged.append((rid, ",".join(sorted(p for p in prs if p))))
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
        (SNAP / "flagged-claims.tsv").write_text("id\tpr\n" + "".join(f"{a}\t{b}\n" for a, b in sorted(flagged)))
    else:
        f = SNAP / "flagged-claims.tsv"
        flagged = [tuple(l) for l in tsv(f, header=False)[1:]] if f.is_file() else []
    for r in rows:
        a = known.get(r["id"]) if r["status"] in ("tested", "done") else None
        r["pr"], r["date"] = (a["pr"], a["date"]) if a else ("", "")
    return flagged


# ------------------------------------------------------------------ the board
AREAS = [  # area, title, format
    ("kontakt.ksp", "Kontakt KSP: commands, callbacks, variables", "Kontakt"),
    ("kontakt.par", "Kontakt parameters", "Kontakt"),
    ("kontakt.ui", "Kontakt UI: control type × control parameter", "Kontakt"),
    ("kontakt.dsp", "Kontakt DSP: effects and modulators", "Kontakt"),
    ("kontakt.format", "Kontakt format (ni-file)", "Kontakt"),
    ("uvi.api", "UVI Falcon script API", "UVI"),
    ("uvi.ui", "UVI Falcon UI widgets", "UVI"),
    ("uvi.dsp", "UVI Falcon DSP: elements", "UVI"),
    ("uvi.par", "UVI Falcon parameters", "UVI"),
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
    return out


SNAPSHOTS = [
    ("ksp-manual.tsv", "the KSP manual's items", "tools/parity-board.py --ksp-manual KSP_Reference_Manual.txt (pdftotext -layout of the manual)"),
    ("ksp-status.tsv", "what KONTRA's VM and engine answer", "build-slot cargo run -p kontra-host --example ksp_status > docs/parity/snapshots/ksp-status.tsv"),
    ("ni-file-census.tsv", "the ni-file corpus census", "build-slot cargo run --release -p kontra-format-kontakt --example chunks -- <Kontakt library root> > census.md; tools/parity-board.py --census census.md"),
    ("ui-corpus.tsv", "UI areas that differ from Kontakt per corpus instrument", "tools/ksp-ui/align.py per instrument (README 'Library instruments'); one row per instrument: an opaque id and its count"),
    ("uvi-reference.tsv", "UVI Falcon's public script reference", f"tools/parity-board.py --uvi-reference ({UVI_DOCS})"),
    ("uvi-elements.tsv", "which UVI elements the loader maps or runs", "the UVI catalog lane (was tools/coverage/data/uvi-features.tsv)"),
    ("modulation-targets.tsv", "Kontakt's modulation targets", "docs/specs/modulation-laws.md lane"),
    ("attribution.tsv", "PR and date per passing row", "tools/parity-board.py (blames rows it lacks)"),
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
           "`tools/parity-board.py`; do not edit by hand (`tools/parity-board.py --check` fails when it is stale).", "",
           "**tested**: KONTRA does it and a test, probe or oracle measurement checks it. **done**: KONTRA does it. "
           "**partial**: some of it. **missing**: not yet. Passing = tested + done. Parameter cells: law (set → internal "
           "value), display (internal → text), range (clamp), acts (drives sound or the UI).", "",
           "PR: the squash merge that last changed each row's evidence line (`Parity: <row-id>` trailers add claims). "
           "KONTRA#N is a PR of the public KONTRA repository from before it moved into the monorepo; #N is a monorepo PR.", "",
           "## Summary", "",
           "| Criterion | Items | tested | done | partial | missing | % tested | % passing |",
           "|---|---:|---:|---:|---:|---:|---:|---:|"]
    out += [f"| {label} | " + " | ".join(map(str, c)) + " |" for label, c in sums]
    out += ["", f"ni-file corpus bytes: {tot.get('bytes', '?')} read, {tot.get('unaccounted', '?')} unaccounted for "
                f"(census snapshot); {latest if latest is not None else '?'} unaccounted after the latest batch in "
                f"docs/re/NI_FILE_PARITY.md.",
            f"UI corpus: {mism[0]} mismatched areas across {mism[1]} instruments (snapshots/ui-corpus.tsv).", "",
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
            out += [f"### {g} ({c[1]} tested, {c[2]} done, {c[3]} partial, {c[4]} missing)", "",
                    "| Status | Row | Item | Evidence | PR | Changed | Note |", "|---|---|---|---|---|---|---|"]
            for r in grs:
                out.append(f"| {r['status']} | `{r['id']}` | {esc(r['name'])} | {ev_link(r['ev'])} | {pr_md(r['pr'])} | {r['date']} | {esc(r['note'])} |")
    return "\n".join(out) + "\n"


def render_html(rows, sums):
    data = [[r["id"], r["area"], r["group"], r["name"], r["status"], r["ev"][0] if r["ev"] and r["ev"][0] else "",
             r["pr"], r["date"], r["note"]] for r in rows]
    areas = [[a, t] for a, t, _ in AREAS]
    summ = [[label.replace("**", "").strip(), c, label.startswith("  ")] for label, c in sums]
    page = HTML.replace("__FRAGMENT__", FRAGMENT).replace("__DATA__", json.dumps(data, ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/"))
    page = page.replace("__AREAS__", json.dumps(areas)).replace("__SUMMARY__", json.dumps(summ)).replace("__REPO__", REPO)
    return page


FRAGMENT = """<!-- board:start --><div class="pboard"><style>
.pboard{--bg:#fafaf8;--fg:#1b1b1b;--mute:#6b6b6b;--line:#e2e2dc;--card:#fff;--tested:#2f7d4f;--done:#7aa95c;--partial:#d39b2a;--missing:#c4c4bc}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]) .pboard{--bg:#161616;--fg:#ececec;--mute:#9a9a9a;--line:#2c2c2c;--card:#1e1e1e;--missing:#4a4a4a}}
:root[data-theme="dark"] .pboard{--bg:#161616;--fg:#ececec;--mute:#9a9a9a;--line:#2c2c2c;--card:#1e1e1e;--missing:#4a4a4a}
.pboard,.pboard *{box-sizing:border-box}.pboard{color:var(--fg);font:15px/1.45 system-ui,sans-serif}
.pboard .sum{display:grid;gap:6px;margin-bottom:24px}.pboard .srow{display:grid;grid-template-columns:minmax(120px,280px) 1fr 70px;gap:10px;align-items:center}
.pboard .srow.sub .lab{padding-left:16px;color:var(--mute);font-size:13px}.pboard .srow.tot .lab{font-weight:600}
.pboard .bar{display:flex;height:14px;border-radius:3px;overflow:hidden;background:var(--missing)}.pboard .bar span{display:block;height:100%}
.pboard .pct{text-align:right;font-variant-numeric:tabular-nums}.pboard .legend{display:flex;gap:14px;flex-wrap:wrap;font-size:13px;color:var(--mute);margin:8px 0 20px}
.pboard .dot{display:inline-block;width:10px;height:10px;border-radius:2px;margin-right:4px;vertical-align:-1px}
.pboard .controls{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px;position:sticky;top:0;background:var(--bg);padding:8px 0;z-index:1}
.pboard .controls input,.pboard .controls select{font:inherit;padding:6px 8px;border:1px solid var(--line);border-radius:4px;background:var(--card);color:var(--fg)}
.pboard .controls input{flex:1;min-width:160px}.pboard details{border:1px solid var(--line);border-radius:6px;margin-bottom:8px;background:var(--card)}
.pboard summary{cursor:pointer;padding:10px 12px;font-weight:600}.pboard summary small{font-weight:400;color:var(--mute)}
.pboard .wrap{overflow-x:auto}.pboard table{border-collapse:collapse;width:100%;font-size:13px}.pboard td,.pboard th{padding:4px 8px;border-top:1px solid var(--line);text-align:left;vertical-align:top}
.pboard th{color:var(--mute);font-weight:500}.pboard td.id{font-family:ui-monospace,monospace;font-size:12px;word-break:break-all}.pboard a{color:inherit}
.pboard .st{white-space:nowrap}.pboard .more{padding:8px 12px}.pboard .more button{font:inherit;padding:4px 10px}
</style>
<div class="legend"><span><i class="dot" style="background:var(--tested)"></i>tested: done and checked by a test, probe or oracle measurement</span><span><i class="dot" style="background:var(--done)"></i>done</span><span><i class="dot" style="background:var(--partial)"></i>partial</span><span><i class="dot" style="background:var(--missing)"></i>missing</span></div>
<div class="sum" id="sum"></div>
<div class="controls"><input id="q" type="search" placeholder="Filter rows (id, name, note)" aria-label="Filter rows"><select id="st" aria-label="Status"><option value="">All statuses</option><option>tested</option><option>done</option><option>partial</option><option>missing</option></select></div>
<div id="areas"></div>
<script>
const ROWS=__DATA__,AREAS=__AREAS__,SUM=__SUMMARY__,REPO="__REPO__";
const C={tested:"var(--tested)",done:"var(--done)",partial:"var(--partial)",missing:"var(--missing)"};
const e=s=>String(s).replace(/[&<>"]/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"})[c]);
document.getElementById("sum").innerHTML=SUM.map(([l,c,sub])=>{const n=c[0]||1;const segs=["tested","done","partial"].map((s,i)=>`<span style="width:${100*c[i+1]/n}%;background:${C[s]}" title="${s} ${c[i+1]}"></span>`).join("");
return `<div class="srow${sub?" sub":""}${/overall|^All$/.test(l)?" tot":""}"><div class="lab">${e(l)} <small>(${c[0]})</small></div><div class="bar" role="img" aria-label="${c[6]}% passing">${segs}</div><div class="pct">${c[6]}%</div></div>`}).join("");
const PAGE=200;
function draw(){const q=document.getElementById("q").value.toLowerCase(),st=document.getElementById("st").value;
document.getElementById("areas").innerHTML=AREAS.map(([a,t])=>{const rs=ROWS.filter(r=>r[1]===a&&(!st||r[4]===st)&&(!q||(r[0]+" "+r[3]+" "+r[8]).toLowerCase().includes(q)));
return `<details data-a="${a}"${q||st?" open":""}><summary>${e(t)} <small>${rs.length} rows</small></summary><div class="wrap"><table><thead><tr><th>Status</th><th>Row</th><th>Item</th><th>Evidence</th><th>PR</th><th>Changed</th><th>Note</th></tr></thead><tbody data-n="0"></tbody></table></div><div class="more"></div></details>`}).join("");
document.querySelectorAll("#areas details").forEach(d=>{const fill=()=>{const a=d.dataset.a,tb=d.querySelector("tbody");const rs=ROWS.filter(r=>r[1]===a&&(!st||r[4]===st)&&(!q||(r[0]+" "+r[3]+" "+r[8]).toLowerCase().includes(q)));
const n=+tb.dataset.n,m=Math.min(rs.length,n+PAGE);tb.insertAdjacentHTML("beforeend",rs.slice(n,m).map(r=>`<tr><td class="st"><i class="dot" style="background:${C[r[4]]}"></i>${r[4]}</td><td class="id">${e(r[0])}</td><td>${e(r[3])}</td><td>${r[5]?`<a href="${REPO}${e(r[5])}">${e(r[5].split("/").pop())}</a>`:""}</td><td>${r[6]?r[6].split(",").map(p=>p[0]==="K"?`<a href="https://github.com/DerpcatMusic/KONTRA/pull/${e(p.slice(1))}">KONTRA#${e(p.slice(1))}</a>`:"#"+e(p)).join(", "):""}</td><td>${e(r[7])}</td><td>${e(r[8])}</td></tr>`).join(""));
tb.dataset.n=m;const more=d.querySelector(".more");more.innerHTML=m<rs.length?`<button>Show ${Math.min(PAGE,rs.length-m)} more of ${rs.length-m}</button>`:"";const b=more.querySelector("button");if(b)b.onclick=fill};
if(d.open)fill();else d.addEventListener("toggle",()=>{if(d.open&&d.querySelector("tbody").dataset.n==="0")fill()},{once:true})})}
document.getElementById("q").addEventListener("input",draw);document.getElementById("st").addEventListener("change",draw);draw();
</script></div><!-- board:end -->"""

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
    spelled = {"$ENGINE_PAR_SF100_OVEDRIVE": "$ENGINE_PAR_SF100_OVERDRIVE"}  # the manual's misspelling of the binary's name
    kontakt = set(read("crates/script/ksp/data/kontakt-8.13.1-names.txt").split())  # names Kontakt 8.13.1 knows
    at = {}
    for i, line in enumerate(manual):
        m = re.match(r"^\d+\. (.+?)\s*$", line)
        if m and m[1] in SECTIONS and i > 300:  # headings appear in the contents and the body: take the body
            at[m[1]] = i
    starts = sorted((i, t) for t, i in at.items())
    section = lambda i: max(((s, t) for s, t in starts if s <= i), default=(0, "Front"))[1]
    declared = {m[1].lower() for m in re.finditer(
        r"(?:declare|decalre)\s+(?:(?:const|polyphonic|global|local|read|pers|instpers|ui_\w+)\s+)*[$%~@!?]?([A-Za-z_]\w*)", whole)}
    items = {}
    fn, var, cb = re.compile(r"^([a-z_][a-z_0-9]*)\(\)\s*$"), re.compile(r"^[\s•\-\*]*([$%~@!][A-Za-z_][A-Za-z_0-9]*)"), re.compile(r"^\s*on ([a-z_]+)")
    last_par = None
    for i, line in enumerate(manual):
        sec = section(i)
        if sec == "Front":
            continue
        if sec == "User Interface Widgets" and re.fullmatch(r"ui_[a-z_]+", line.strip()):
            items.setdefault(("widget", line.strip()), [sec, ""])
        if m := fn.match(line):
            items.setdefault(("fn", m[1]), [sec, ""])
        elif m := var.match(line):
            name = m[1]
            tail = re.match(r"^([A-Z_0-9]+)(?:\s|$)", manual[i + 1] if i + 1 < len(manual) else "")
            if tail and name.isupper() and name + tail[1] in whole:  # a long name the PDF wrapped in a table cell
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
                    continue  # an example's own variable
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
    out = ["kind\tname\tsection\tapplies"] + [f"{k}\t{n}\t{s}\t{a}" for (k, n), (s, a) in items.items()]
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
    (SNAP / "ni-file-census.tsv").write_text("\n".join(out) + "\n")
    print(f"{len(out) - 1} rows -> {SNAP / 'ni-file-census.tsv'}")


# ------------------------------------------------------------------ main
def build(refresh):
    manual, status = ksp_inputs()
    ksp, pars = kontakt_ksp_and_params(manual, status)
    ui, corpus = kontakt_ui(manual)
    fmt, totals, latest = kontakt_format()
    rows = ksp + pars + modulation_targets() + ui + kontakt_dsp(status) + fmt + uvi()
    seen = set()
    for r in rows:  # ids are the links' anchors: keep them unique
        while r["id"] in seen:
            r["id"] += "_"
        seen.add(r["id"])
    apply_overrides(rows)
    flagged = attribute(rows, refresh)
    mism = (sum(int(r["areas"]) for r in corpus), len(corpus))
    sums = summary(rows)
    return render_md(rows, sums, (totals, latest, mism), flagged), render_html(rows, sums), sums


def main():
    args = sys.argv[1:]
    if args[:1] == ["--ksp-manual"]:
        return refresh_ksp_manual(args[1])
    if args[:1] == ["--uvi-reference"]:
        return refresh_uvi_reference(args[1] if len(args) > 1 else None)
    if args[:1] == ["--census"]:
        return refresh_census(args[1])
    check = args[:1] == ["--check"]
    md, page, sums = build(refresh=not check)
    if check:
        stale = [p.relative_to(ROOT).as_posix() for p, t in ((OUT_MD, md), (OUT_HTML, page)) if not p.is_file() or p.read_text() != t]
        if stale:
            sys.exit(f"parity board out of date: {', '.join(stale)}. Run plugins/kontra/tools/parity-board.py and commit the result.")
        print("parity board is current")
        return
    OUT_MD.write_text(md)
    OUT_HTML.write_text(page)
    for label, c in sums:
        print(f"{label:48} {c[0]:6} tested {c[1]:5} done {c[2]:5} partial {c[3]:5} missing {c[4]:5}  {c[5]}% / {c[6]}%")


if __name__ == "__main__":
    main()
