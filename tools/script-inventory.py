#!/usr/bin/env python3
"""script-inventory.py DUMP UI.json [KSP_STATUS.tsv] -- every function, callback and builtin
name a library's scripts use, whether KONTRA implements it, and when a run hit it.

DUMP is the directory `KONTRA_INSPECT_DUMP` wrote (one `<slot>-<name>/source` per script);
UI.json is `kontra-inspect ui --json` of the same instrument, whose `calls` say which
actions load, a note and the UI sweep made and how many went unanswered. KSP_STATUS is
`cargo run -p kontra-host --example ksp_status` (KSP only).

Status per name:
  KSP  fn:  vm / answered = implemented, missing = compiles but the engine does not answer
       var: engine (a builtin the VM reads), measured (constant with Kontakt's value),
            opaque (a catalog name with no value of its own), unknown
  Lua  a global the script reads but neither it nor KONTRA's prelude defines = missing
Prints a TSV (kind, name, status, uses, load/note/ui calls, unanswered) and a summary line.
"""
import json
import pathlib
import re
import sys

import os
ROOT = pathlib.Path(os.environ.get("KONTRA_ROOT") or pathlib.Path(__file__).resolve().parent.parent)


def ksp_names(src):
    src = re.sub(r"\{[^}]*\}", " ", src)
    src = re.sub(r'"[^"\n]*"', " ", src)
    declared = set(re.findall(r"\bdeclare\s+(?:const\s+|polyphonic\s+|ui_\w+\s+|read\s+|pers\s+|instpers\s+)*([$%@!~?]\w+)", src))
    declared |= set(re.findall(r"\bfunction\s+(\w+)", src))
    declared |= set(re.findall(r"\bmacro\s+(\w+)", src))
    # A builtin may carry a leading `_` (`_will_never_terminate`); `on x(` opens a callback.
    calls = [c.lstrip("_") or c for c in re.findall(r"(?<![$%@!~?\w])(?<!on )([a-z_][a-z0-9_]*)\s*\(", src)]
    calls += re.findall(r"^\s*([a-z_][a-z0-9_]*)\s*$", src, re.M)
    callbacks = re.findall(r"^\s*on\s+(\w+)", src, re.M)
    vars_ = [v for v in re.findall(r"([$%@!~?][A-Z][A-Z0-9_]*)\b(?![a-z])", src) if v not in declared]
    return calls, callbacks, vars_, declared


def ksp_tables(status_path):
    status = {}
    if status_path:
        for line in open(status_path):
            k, name, st = (line.rstrip("\n").split("\t") + ["", "", ""])[:3]
            status[(k, name if k == "par" else name.lower())] = st
    builtins = (ROOT / "crates/script/ksp/src/builtins.rs").read_text()
    engine_vars = set(re.findall(r'"([$%@!~?][A-Z][A-Z0-9_]*)"', builtins))
    fns = set(re.findall(r'"([a-z_][a-z0-9_]*)"', builtins))
    engine_vars |= set(re.findall(r'"([$%@!~?][A-Z][A-Z0-9_]*)"', (ROOT / "crates/script/ksp/src/consts.rs").read_text()))
    engine_vars |= {name for (k, name), st in status.items() if k == "par" and st == "carried"}
    measured = {l.split("\t")[0] for l in (ROOT / "crates/script/ksp/data/constants.tsv").read_text().splitlines() if l and not l.startswith("#")}
    opaque = set((ROOT / "crates/script/ksp/data/vendor_constants.txt").read_text().split())
    return status, engine_vars, fns, measured, opaque


KEYWORDS = {"if", "while", "for", "select", "and", "or", "not", "mod", "end", "else", "case", "to", "downto", "call", "on", "function", "declare", "const", "abs", "in_range"}
UVI_CALLBACKS = set("onInit onLoad onSave onNote onRelease onController onPitchBend onAfterTouch onPolyAfterTouch onProgramChange onTransport onEvent".split())
LUA_KEYWORDS = set("and break do else elseif end false for function goto if in local nil not or repeat return then true until while self".split())


# One scan over Lua's strings and comments, so `--` in a string starts nothing.
LUA_TOKENS = re.compile(
    r"""--\[(?P<lc>=*)\[.*?\](?P=lc)\]"""  # long comment
    r"""|--[^\n]*"""  # line comment
    r"""|\[(?P<ls>=*)\[.*?\](?P=ls)\]"""  # long string
    r"""|"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'""",
    re.S,
)


def lua_names(src):
    src = LUA_TOKENS.sub(lambda m: " " if m.group(0).startswith("--") else '""', src)
    # Names this file alone sees (locals, parameters, loop variables) and
    # those every file sees (globals it defines).
    local = set(re.findall(r"\blocal\s+function\s+([A-Za-z_]\w*)", src))
    for names in re.findall(r"\blocal\s+([A-Za-z_][\w\s,]*?)(?:=|\n|$)", src):
        local |= {n.strip() for n in names.split(",") if n.strip()}
    for args in re.findall(r"\bfunction\b[^(]*\(([^)]*)\)", src):
        local |= {a.strip() for a in args.split(",") if a.strip() and a.strip() != "..."}
    for names in re.findall(r"\bfor\s+([\w\s,]+?)\s+(?:in|=)", src):
        local |= {n.strip() for n in names.split(",")}
    shared = set(re.findall(r"(?<!local )\bfunction\s+([A-Za-z_]\w*)\s*\(", src))
    shared |= set(re.findall(r"\bfunction\s+[A-Za-z_][\w.]*[.:]([A-Za-z_]\w*)", src))
    shared |= set(re.findall(r"^\s*([A-Za-z_]\w*)\s*=[^=]", src, re.M)) - local
    # Globals read: an identifier not after `.` or `:`.
    used = re.findall(r"(?<![.:\w])([A-Za-z_]\w*)", src)
    members = re.findall(r"(?<=[.:])([A-Za-z_]\w*)\s*\(", src)
    callbacks = [c for c in re.findall(r"\bfunction\s+(on[A-Z]\w*)", src) if c in UVI_CALLBACKS]
    return used, members, callbacks, local, shared


def lua_api():
    """KONTRA's Lua host: the names it defines, and those it defines as empty stubs."""
    names, stubs = set(), set()
    for f in ("prelude.lua", "widgets.lua"):
        t = (ROOT / "crates/script/lua/src" / f).read_text()
        names |= set(re.findall(r"^\s*(?:local\s+)?function\s+([A-Za-z_]\w*)", t, re.M))
        names |= set(re.findall(r"^\s*([A-Za-z_]\w*)\s*=", t, re.M))
        names |= set(re.findall(r"\bfunction\s+[A-Za-z_]\w*[.:]([A-Za-z_]\w*)", t))
        stubs |= set(re.findall(r"^\s*function\s+([A-Za-z_]\w*)\([^)]*\)\s*end", t, re.M))
        # Classes made in a loop over their names: `for _, kind in ipairs({ "Knob", ... })`.
        names |= set(re.findall(r'"([A-Z]\w*)"', t))
        # `for _, name in ipairs({...}) do _G[name] = function() end end`
        for block in re.findall(r"ipairs\(\{([^}]*)\}\)\s*do\s*_G\[name\]\s*=\s*function\(\)\s*end", t, re.S):
            stubs |= set(re.findall(r'"(\w+)"', block))
    for f in (ROOT / "crates/script/lua/src").rglob("*.rs"):
        names |= set(re.findall(r'"([A-Za-z_]\w*)"', f.read_text()))
    names |= set("assert error getmetatable ipairs next pairs pcall print rawequal rawget rawlen rawset require select setmetatable tonumber tostring type unpack xpcall math string table coroutine os bit32 utf8 _G _VERSION".split())
    names |= set("abs ceil floor fmod log log10 max min pow random sqrt exp sin cos huge pi insert remove sort concat getn maxn find sub gsub gmatch gfind format len lower upper rep byte char match mod".split())
    return names, stubs


def main():
    dump, ui = pathlib.Path(sys.argv[1]), json.load(open(sys.argv[2]))
    status_path = sys.argv[3] if len(sys.argv) > 3 else None
    calls = ui["result"].get("calls", {})

    def hit(action):
        key = "".join(w.capitalize() for w in action.split("_"))
        out = []
        for ph in ("load", "note", "ui"):
            c, u = calls.get(ph, {}).get(key, [0, 0])
            out.append((c, u))
        return out

    # Per-name Lua API counts: only when the report carries them (the
    # inspector counts host actions, `calls`, not Lua functions).
    lua_hits = ui["result"].get("lua_calls")

    def lua_calls(ph, name):
        return "-" if lua_hits is None else lua_hits.get(ph, {}).get(name, 0)

    rows = {}
    sources = sorted(dump.glob("*/source"))
    # KSP opens with `on <callback>`, maybe after `{ ... }` comments.
    def ksp_source(text):
        return re.sub(r"^(?:\s*\{[^}]*\})*\s*", "", text).startswith("on ")

    ksp = any(ksp_source(p.read_text(errors="ignore")) for p in sources)
    if ksp:
        status, engine_vars, fns, measured, opaque = ksp_tables(status_path)
        for p in sources:
            calls_, cbs, vars_, declared = ksp_names(p.read_text(errors="ignore"))
            for c in cbs:
                rows.setdefault(("cb", c), [status.get(("cb", c), "compiled" if c else "?"), 0])[1] += 1
            for c in calls_:
                if c in KEYWORDS or c in declared:
                    continue
                st = status.get(("fn", c)) or ("vm" if c in fns else "unknown")
                rows.setdefault(("fn", c), [st, 0])[1] += 1
            for v in vars_:
                st = "engine" if v in engine_vars else "measured" if v in measured else "opaque" if v in opaque else "unknown"
                rows.setdefault(("var", v), [st, 0])[1] += 1
    else:
        # UVI's own API (the names its embedded Lua defines), when given.
        uvi_api = set(pathlib.Path(sys.argv[3]).read_text().split()) if status_path else set()
        # Its free functions; `Class:method` names are its own classes' methods.
        uvi_api = {n.split(".")[-1] for n in uvi_api if ":" not in n}
        api, stubs = lua_api()
        prelude_text = (ROOT / "crates/script/lua/src/prelude.lua").read_text()
        files, shared_all, cbs_all = [], set(), []
        for p in sorted(dump.rglob("*")):
            # The program tree and state the bank carries are data, not script.
            if p.is_file() and (p.name == "source" or (p.suffix == ".lua" and not p.name.startswith("__"))):
                used, members, cbs, local, shared = lua_names(p.read_text(errors="ignore"))
                files.append((used, members, local))
                shared_all |= shared
                cbs_all += cbs
        defined_all = shared_all | {n for _, _, local in files for n in local}
        used_all = []
        for used, members, local in files:
            # A method the script defines is its own, whatever UVI names alike.
            used_all += [u for u in used if u not in local]
            used_all += [m for m in members if m not in defined_all]
        for c in cbs_all:
            rows.setdefault(("cb", c), ["implemented" if c in api or c in prelude_text else "missing", 0])[1] += 1
        for u in used_all:
            if u in LUA_KEYWORDS or u == "to" or u.startswith("on") and u[2:3].isupper():
                continue
            if u not in uvi_api and u not in api:
                continue
            if u in shared_all and u not in uvi_api:
                continue
            st = "stub" if u in stubs else "implemented" if u in api else "missing"
            rows.setdefault(("api", u), [st, 0])[1] += 1

    if not ksp and lua_hits is None:
        print("# no lua_calls in the report: Lua rows show '-' per phase", file=sys.stderr)
    print("kind\tname\tstatus\tuses\tload\tnote\tui\tunanswered")
    counts = {}
    for (kind, name), (st, n) in sorted(rows.items(), key=lambda r: (r[0][0], -r[1][1])):
        h = hit(name) if kind == "fn" else [(0, 0)] * 3
        if kind == "api":
            h = [(lua_calls(ph, name), 0) for ph in ("load", "note", "ui")]
        un = sum(u for _, u in h if isinstance(u, int))
        print(f"{kind}\t{name}\t{st}\t{n}\t" + "\t".join(str(c) for c, _ in h) + f"\t{un}")
        counts.setdefault(kind, {}).setdefault(st, 0)
        counts[kind][st] += 1
    phases = {ph: (sum(c for c, _ in v.values()), sum(u for _, u in v.values()))
              for ph, v in calls.items() if isinstance(v, dict)}
    print(f"# names {json.dumps(counts)}; calls (made, unanswered) {json.dumps(phases)}; "
          f"unanswered pars {len(calls.get('unanswered_pars', []))}; faults {calls.get('faults', '?')}", file=sys.stderr)


if __name__ == "__main__":
    main()
