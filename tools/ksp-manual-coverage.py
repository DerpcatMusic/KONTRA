#!/usr/bin/env python3
"""KSP manual coverage: every symbol the manual documents against what KONTRA does.

    cargo run --release -p kontra-host --example ksp_status > /tmp/ksp-status.tsv
    tools/ksp-manual-coverage.py MANUAL.txt /tmp/ksp-status.tsv > docs/ksp/MANUAL-COVERAGE.md

A manual item is a command heading (`name()`), a callback (`on name`) or a
`$`/`%` name that opens a line or a bullet. Status per item:

  tested   KONTRA does it and a probe/case/test names it
  done     KONTRA does it (the VM lowers it or the engine answers a probe)
  partial  the name compiles but nothing acts on it (an opaque constant, an
           action the engine does not answer, a parameter KONTRA does not carry)
  missing  the compiler does not know the name

Re-run after a change; `--tsv` prints the rows for the board instead.
"""
import collections
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
manual = pathlib.Path(sys.argv[1]).read_text(errors="ignore").splitlines()
status = {}
for line in pathlib.Path(sys.argv[2]).read_text().splitlines():
    kind, name, st = line.split("\t")
    status[(kind, name.lower() if kind != "par" else name.lstrip("$").upper())] = st

# Names the manual misspells in its own examples (real ones are listed beside them).
TYPOS = {"$EVENT_PAR_MIDI_CHANNNEL", "$CUSTOM_EVENT_PAR_4", "$NI_VL_TMRPO_HQ"}
SECTIONS = ["General Commands", "Array Commands", "Engine Parameter Commands", "Event Commands",
            "Group Commands", "Keyboard Commands", "Load/Save Commands", "MIDI Object Commands",
            "Music Information Retrieval", "Time-Related Commands", "User Interface Commands",
            "Zone Commands", "Built-in Variables and Constants", "Control Parameters",
            "Engine Parameters", "Zone Parameters", "Callbacks", "Variables",
            "Arithmetic Commands & Operators", "Control Statements", "User-defined Functions",
            "User Interface Widgets", "Multi Script", "Advanced Concepts"]
# Section headings are "<n>. <Title>" and appear twice (table of contents, body): take the body.
at = {}
for i, line in enumerate(manual):
    m = re.match(r"^\d+\. (.+?)\s*$", line)
    if m and m[1] in SECTIONS and i > 300:
        at[m[1]] = i
starts = sorted((i, t) for t, i in at.items())


def section(i):
    cur = "Front"
    for s, t in starts:
        if s <= i:
            cur = t
    return cur


items = collections.OrderedDict()  # (kind, name) -> section
fn = re.compile(r"^([a-z_][a-z_0-9]*)\(\)\s*$")
var = re.compile(r"^[\s•\-\*]*([$%~@!][A-Za-z_][A-Za-z_0-9]*)")
cb = re.compile(r"^\s*on ([a-z_]+)")
for i, line in enumerate(manual):
    sec = section(i)
    if sec == "Front":
        continue
    if m := fn.match(line):
        items.setdefault(("fn", m[1]), sec)
    elif m := var.match(line):
        name = m[1]
        if name.endswith(("_", "#")) or name in TYPOS:  # a family prefix, or a typo in the manual
            continue
        up = name.upper()
        if "ENGINE_PAR" in up:
            kind = "par"
        elif "EVENT_PAR" in up:
            kind = "evpar"
        elif up.startswith(("$CONTROL_PAR", "$ZONE_PAR")):
            kind = "uipar"
        elif up.startswith(("$NI_", "$EVENT_", "$KEY_", "$MARK_", "$NUM_", "$CURRENT_", "%")):
            kind = "var"
        else:
            kind = "sym"
        if sec in ("Variables", "Built-in Variables and Constants", "Engine Parameters",
                   "Control Parameters", "Zone Parameters", "Event Commands", "General Commands",
                   "Callbacks") or kind in ("par", "evpar", "uipar"):
            items.setdefault((kind, name.lstrip("$%~@!").lower() if kind == "var" else name), sec)
    elif m := cb.match(line):
        if sec == "Callbacks":
            items.setdefault(("cb", m[1]), sec)

# Names the compiler knows: builtins, then constants.
names = set()
tsv = (ROOT / "crates/script/ksp/data/constants.tsv").read_text()
valued = {l.split("\t")[0].lstrip("$").upper() for l in tsv.splitlines() if l and l[0] != "#"}
vendor = {l.strip().lstrip("$%~@!").upper() for l in (ROOT / "crates/script/ksp/data/vendor_constants.txt").read_text().splitlines() if l.strip() and l[0] != "#"}
text = subprocess.run(
    ["grep", "-rIl", "-E", "ksp|KSP", "--include=*.rs", "--include=*.ksp", "--include=*.lua", "--include=*.py",
     "crates/script/ksp/tests", "crates/app/host/tests", "tools/ksp-conformance", "tools/ui-probe"],
    cwd=ROOT, capture_output=True, text=True).stdout.split()
test_text = "\n".join((ROOT / f).read_text(errors="ignore") for f in text if (ROOT / f).is_file()).lower()

CB = {"init": "init", "note": "note", "release": "release", "controller": "controller", "poly_at": "polyat",
      "ui_control": "uicontrol", "ui_controls": "uicontrols", "ui_update": "uiupdate", "listener": "listener",
      "pgs_changed": "pgschanged", "persistence_changed": "persistencechanged",
      "async_complete": "asynccomplete", "rpn": "rpn", "nrpn": "nrpn",
      "note_controller": "notecontroller", "midi_in": "midiin"}

rows = []
for (kind, name), sec in items.items():
    key = name.lower()
    if kind == "fn":
        st = status.get(("fn", key))
        got = {"vm": "done", "answered": "done", "missing": "partial"}.get(st, "missing")
    elif kind == "cb":
        got = "done" if ("cb", CB.get(name, "")) in {(k, v.lower()) for (k, v) in status if k == "cb"} or name in CB else "missing"
        if name not in CB:
            got = "missing"
    elif kind == "par":
        got = "done" if ("par", name.lstrip("$").upper()) in status else "missing"
    else:
        bare = name.lstrip("$%~@!").upper()
        bare_lc = name.lstrip("$%~@!").lower()
        known_fn = any(("fn", p + bare_lc) in status for p in ("", "$", "%"))
        got = "done" if known_fn \
            else "done" if bare in valued else "partial" if bare in vendor else "missing"
    if got == "done" and re.search(r"\b" + re.escape(key.lstrip("$%~@!")) + r"\b", test_text):
        got = "tested"
    rows.append((kind, name, sec, got))

if "--tsv" in sys.argv:
    for r in rows:
        print("\t".join(r))
    sys.exit()

total = collections.Counter(r[3] for r in rows)
out = ["# KSP manual coverage", "",
       "Generated by `tools/ksp-manual-coverage.py` from the KSP manual and `kontra-host --example ksp_status`.",
       "", f"**{len(rows)} items: " + ", ".join(f"{total[s]} {s}" for s in ("tested", "done", "partial", "missing")) + ".**", "",
       "| Kind | Items | tested | done | partial | missing |", "|---|---:|---:|---:|---:|---:|"]
by = collections.defaultdict(collections.Counter)
for kind, _, _, st in rows:
    by[kind][st] += 1
label = {"fn": "commands", "cb": "callbacks", "par": "$ENGINE_PAR_*", "evpar": "$EVENT_PAR_*",
         "uipar": "$CONTROL_PAR_*/$ZONE_PAR_*", "var": "built-in variables", "sym": "other names"}
for kind, c in by.items():
    out.append(f"| {label[kind]} | {sum(c.values())} | {c['tested']} | {c['done']} | {c['partial']} | {c['missing']} |")
out += ["", "## Not yet done", "", "| Kind | Name | Manual section | Status |", "|---|---|---|---|"]
for kind, name, sec, st in rows:
    if st in ("partial", "missing"):
        out.append(f"| {label[kind]} | `{name}` | {sec} | {st} |")
print("\n".join(out))
