#!/usr/bin/env python3
"""Public feature tracker: the FINITE set of format features, not library specific.

    python3 tools/coverage/tracker.py [OUT_DIR]      (default OUT_DIR=tools/coverage/_site, gitignored)

Writes OUT_DIR/index.html (+ features.json). Static, self-contained, no external fetches.
Never prints or lists per-library counts or route instances: those stay on the private dashboard.

Status (done | partial | todo) is derived on every run from files in the repo:
  engine.tsv rows + their code probes, effects.rs decode arms, data/modulation-targets.tsv,
  the KSP parity ledger (crates/sampler-ksp/src/parity_ledger.rs), docs/re/kontakt-catalog/*.tsv,
  data/uvi-features.tsv (snapshot of the UVI catalog, refreshed by this script when the
  private ~/.cache copy exists). data/overrides.tsv is the only hand-typed layer and always wins.
"""
import html, json, os, re, sys, time
from collections import Counter, OrderedDict

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import build  # existing private dashboard helpers: tsv(), source(), probe_ok(), engine_rows(), target_rows()

ROOT, CAT = build.ROOT, build.CAT
REPO = "https://github.com/DerpcatMusic/KONTRA/blob/main/"
UVI_CACHE = build.UVI
UVI_SNAP = os.path.join(HERE, "data/uvi-features.tsv")
OVERRIDES = os.path.join(HERE, "data/overrides.tsv")
LABEL = {"done": "Done", "partial": "Partial", "todo": "Not started"}
BASE = {"exact": "done", "mapped": "done", "approx": "partial", "reported": "todo", "absent": "todo"}

_paths = {}
def repo_path(fname):
    """Repo-relative path of the first source file called fname (for the 'source' link)."""
    if not _paths:
        for base in ("crates", "vendor/ni-file"):
            for d, _, fs in os.walk(os.path.join(ROOT, base)):
                if "target" in d.split(os.sep):
                    continue
                for f in fs:
                    _paths.setdefault(f, os.path.relpath(os.path.join(d, f), ROOT))
    return _paths.get(os.path.basename(fname.split("::")[0]))


def feat(product, area, key, name, status, note="", src=None, proof="unproven"):
    return {"product": product, "area": area, "id": f"{product}/{area}/{key}".lower().replace(" ", "-"),
            "name": name, "status": status, "note": note, "src": src, "proof": proof}


def derive(row):
    """engine.tsv row -> (status, note). A probe that stops matching means the code changed under it."""
    status, note = BASE[row["status"]], row["note"]
    if build.probe_ok(row["probe"]) is False:
        note = (note + "; " if note else "") + ("unsupported marker gone from code, needs review"
                                                  if status == "todo" else "code moved, needs re-verification")
        status = "partial"
    return status, note


def proof_of(row):
    """engine.tsv 'exact' means the law was proved on the binary or the native oracle."""
    return "proved" if row["status"] == "exact" and build.probe_ok(row["probe"]) is not False else "unproven"


def engine_features(category, product, area, label=lambda r: r["name"], rows=None):
    rows = rows if rows is not None else [r for r in build.engine_rows() if r["category"] == category]
    out = []
    for r in rows:
        s, n = derive(r)
        out.append(feat(product, area, r["key"], label(r), s, n, repo_path(r["probe"]), proof_of(r)))
    return out


# ------------------------------------------------------------------ Kontakt
def effects():
    mods = {int(h, 16): n for h, n in re.findall(r'\(0x([0-9a-f]+), "([^"]+)"\)', build.source("effects.rs").split("const MODULES")[1].split("];")[0])}
    arms = {int(h, 16) for h in re.findall(r"^\s+0x([0-9a-f]{2}) =>", build.source("effects.rs"), re.M)}
    rows = {r["key"]: r for r in build.engine_rows() if r["category"] == "effect"}
    out, seen = [], set()
    for c in build.tsv(os.path.join(CAT, "ser_classes.tsv")):
        cls = re.sub(r"^(class|struct) ", "", c["class"])
        if not cls.startswith("BParFX") or cls == "BParFX":
            continue
        sid = int(c["sertype"], 16)
        if sid in seen:  # classes sharing a serialization id are one effect
            continue
        seen.add(sid)
        name = mods.get(sid) or cls.removeprefix("BParFX")
        row = rows.get(f"0x{sid:02x}")
        if row:
            s, n = derive(row)
        elif sid in arms:
            s, n = "partial", "parameters decoded; no audio kernel yet"
        else:
            s, n = "todo", "no kernel yet; reported as not modeled"
        out.append(feat("Kontakt", "Effects", f"{sid:02x}", name, s, n, "crates/sampler-kontakt/src/effects.rs", proof_of(row) if row else "unproven"))
        out[-1]["keys"] = [cls, cls.removeprefix("BParFX")]
    return out


def targets():
    """Targets individually (collapsed in the page) and the law families they share."""
    rows = build.target_rows(build.engine_rows())
    per = [feat("Kontakt", "Modulation targets", r["key"], r["name"].split(" (ID")[0] + f" (ID {r['key']})",
                BASE[r["status"]], r["note"], "crates/sampler-kontakt/src/library.rs") for r in rows]
    fam = {}
    for r in rows:
        fam.setdefault(r["law"], []).append(BASE[r["status"]])
    desc = {"add": "source added to the parameter", "mul": "source multiplies the parameter",
            "complement": "attenuating law, source shrinks the value toward silence",
            "invert-add": "inverted-add law", "cubic": "cubic-shaped law for external sources"}
    law = []
    for k, v in sorted(fam.items()):
        c = Counter(v)
        s = "done" if c["done"] == len(v) else "todo" if not (c["done"] or c["partial"]) else "partial"
        law.append(feat("Kontakt", "Modulation law families", k, f"{k} law", s,
                        f"{desc.get(k, k)}; {c['done'] + c['partial']} of {len(v)} targets wired", "crates/sampler-kontakt/src/library.rs"))
    return law, per


def file_format():
    out = []
    for r in build.format_layer():
        out.append(feat("Kontakt", "File format objects", r["sertype"], r["class"],
                        "done" if r["status"] == "exact" else "partial" if r["status"] == "mapped" else "todo",
                        r["note"] if r["status"] != "exact" else "", "docs/re/kontakt-catalog/parser_coverage.md",
                        "proved" if r["status"] == "exact" else "unproven"))
    return out


KSP_FAMILIES = [  # (family, regex over parity-ledger names), first match wins; hand taxonomy, statuses derived
    ("Callbacks", r"^on "), ("Language syntax and diagnostics", r"^@syntax|^(if|select|case|declare|const|function|call|exit|continue|polyphonic|pers|instpers|read_persistent_var)$"),
    ("Math and bit operations", r"^(abs|acos|asin|atan|cbrt|ceil|cos|exp2?|floor|in_range|int|int_to_real|real|real_to_int|log2?|log10|lsb|msb|max|min|mod|and|or|not|pow|random|round|sgn|signbit|sin|sh_left|sh_right|sqrt|tan|sort|search|num_elements|array_equal|inc|dec|ms_to_ticks|ticks_to_ms)$"),
    ("Engine parameters", r"^\$ENGINE_PAR|engine_par"), ("Control parameters", r"^\$CONTROL_PAR|control_par|^set_control_help$"),
    ("UI controls", r"^ui_|^\$UI_|^(add_text_line|set_text|set_ui|get_ui|move_control|hide_part|\$HIDE_PART|set_knob|\$KNOB|attach_level_meter|set_skin|set_table|set_script_title|get_font_id|show_library_tab|load_performance_view|make_perfview|load_native_ui|expose_controls|get_key_name|set_key|get_key|\$KEY_COLOR|%KEY|set_map_editor)"),
    ("Menus", r"menu_item|add_menu"), ("Event parameters and note events", r"^\$EVENT|event_par|event_mark|_event|^(play_note|note_off|change_|fade_|ignore_|get_event|\$MARK|%EVENT|%NOTE|\$ALL_EVENTS|\$CURRENT_|\$PLAYED|\$CC_NUM|%CC|%POLY|set_note|set_poly|set_controller|set_midi|set_rpn|set_nrpn|\$RPN|\$MIDI|\$VCC|redirect_output|\$OUTPUT|reset_rls)"),
    ("Groups, zones and key ranges", r"zone|group|keyrange|\$START_CRITERIA|\$LOOP|loop_par|get_sample|set_sample|load_ir|purge|find_|attach_zone|voice_limit|\$SEARCH|\$GET_FOLDER|get_folder|\$INST|%GROUPS|\$NUM_"),
    ("Filter and effect type constants", r"^\$FILTER|^\$EFFECT|^\$NI_(COMP|CHORAL|BITE|FLAIR|DIRT|REPLIKA|S|SUPERGT|KOLOR|BEATMASHER)"),
    ("Modulation and LFO constants", r"^\$INTMOD|^\$LFO|mod|target|^\$ENV"),
    ("MIDI file functions", r"^mf_|midi_file"), ("Persistent global storage (pgs)", r"^pgs_|^\$NI_PGS"),
    ("Wavetable and source constants", r"^\$NI_(WT|SOURCE|SYNC|VC|WF)|wavetable|^\$UI_WF|wf_"),
    ("Audio analysis (detect)", r"detect|^\$NI_DETECT"), ("Async, files and NKS", r"async|^fs_|file|^\$NI_FILE|nks|NKS|_listener|listener|^\$NI_(CB|MOUSE|ASYNC)|^\$NI_EPP|^\$NI_FADE|^\$NI_SIGNAL|_ID$|save_|load_|^\$NI_CONTROL|disable_logging|message|reset_ksp"),
]


def ksp_families():
    text = open(os.path.join(ROOT, "crates/sampler-ksp/src/parity_ledger.rs"), encoding="utf-8").read()
    fam = OrderedDict((f, []) for f, _ in KSP_FAMILIES)
    fam["Other built-ins and constants"] = []
    for name, st in re.findall(r'name: "([^"]+)", status: Status::(\w+)', text):
        if name[0] == "@" and not name.startswith("@syntax") or name.startswith("on ui") and False:
            continue  # per-context diagnostics, not features of the language
        for f, rx in KSP_FAMILIES:
            if re.search(rx, name):
                fam[f].append((name, st))
                break
        else:
            fam["Other built-ins and constants"].append((name, st))
    out = []
    for f, items in fam.items():
        if not items:
            continue
        c = Counter(st for _, st in items)
        works = c["Implemented"] + c["Approximate"]
        s = "done" if works == len(items) else "todo" if works == 0 else "partial"
        bad = [n for n, st in items if st in ("Unsupported", "Stub")]
        note = f"{works} of {len(items)} members run" + (f"; stubbed or unsupported: {', '.join(bad[:4])}{'...' if len(bad) > 4 else ''}" if bad else "")
        out.append(feat("Kontakt", "Script (KSP)", f, f, s, note, "crates/sampler-ksp/src/parity_ledger.rs"))
        out[-1]["keys"] = [n.lstrip("$%!") for n, _ in items if len(n.lstrip("$%!")) >= 7 and not n.startswith("@")]
    return out


# ------------------------------------------------------------------ UVI
def uvi_snapshot():
    """Refresh data/uvi-features.tsv from the private catalog when present (CI uses the committed copy)."""
    if not os.path.exists(UVI_CACHE):
        return
    rows, seen = [], set()
    for r in build.tsv(UVI_CACHE):
        k = r["record_kind"]
        if k == "element":
            key, grp = r["element"], r["category"]
        elif k in ("lua_class", "lua_callbacks"):
            key, grp = r["attribute"], k
        else:
            continue
        if (k, key) not in seen:
            seen.add((k, key))
            rows.append([k, key, grp, r["reader_coverage"]])
    with open(UVI_SNAP, "w", encoding="utf-8") as f:
        f.write("kind\tname\tgroup\tcoverage\n" + "".join("\t".join(x) + "\n" for x in sorted(rows)))


def uvi_status(kind, cov):
    toks = [t.strip() for t in cov.split(";")]
    if kind == "lua_callbacks":
        return ("done", "implemented") if cov.startswith("implemented") else ("todo", "not implemented")
    if kind == "lua_class":
        if cov == "implemented constants/mapping":
            return "done", "implemented"
        return "partial", "proxy exists; exact class parity not proved"
    if cov in ("", "no-specific-translation-read-found", "ignored/unsupported"):
        return "todo", "no translator reads this module yet"
    if any("ignored/unsupported" in t or "parsed-only" in t for t in toks):
        return "partial", "read and partly mapped; some fields ignored or only parsed"
    return "done", "mapped and consumed"


UVI_AREAS = {"FX": "Effects", "Legacy": "Legacy modules", "Oscillator": "Sound sources", "Modulation": "Modulators",
             "Event Processor": "Event processors", "Other": "Structure and routing", "structure-or-native-only": "Structure and routing"}


def uvi_features():
    uvi_snapshot()
    out = []
    for r in build.tsv(UVI_SNAP):
        s, n = uvi_status(r["kind"], r["coverage"])
        area = {"lua_class": "Script (Lua) classes", "lua_callbacks": "Script (Lua) callbacks"}.get(r["kind"]) or UVI_AREAS[r["group"]]
        out.append(feat("UVI", area, r["name"], r["name"], s, n, "crates/sampler-uvi/src/lib.rs" if r["kind"] == "element" else "crates/sampler-uvi/src/script.rs"))
    return out


# ------------------------------------------------------------------ assemble
def overrides(feats):
    if not os.path.exists(OVERRIDES):
        return
    byid = {f["id"]: f for f in feats}
    for line in open(OVERRIDES, encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        fid, status, note, proof = (line.rstrip("\n").split("\t") + [""] * 4)[:4]
        f = byid.get(fid.lower())
        if f and status in LABEL:
            f["status"], f["note"] = status, note or f["note"]
            f["proof"] = proof or f["proof"]
        elif not f:
            print("override matches no feature:", fid, file=sys.stderr)


KONTAKT_ORDER = ["Effects", "Modulators", "LFO waveforms", "Modulation sources", "Modulation law families", "Source modes",
                 "Start conditions", "Script (KSP)", "Modulation targets", "File format objects"]
UVI_ORDER = ["Effects", "Legacy modules", "Sound sources", "Modulators", "Event processors", "Structure and routing",
             "Script (Lua) classes", "Script (Lua) callbacks"]
COLLAPSED = {"Modulation targets", "File format objects", "Legacy modules"}


AUDIT = os.path.join(HERE, "data/audit")
ALIASES = os.path.join(HERE, "data/audit-aliases.tsv")
AREA_PATH = {  # an audit row may only join a feature whose area plausibly owns the audited file
    "Effects": r"effects|/dsp/|inserts|reverb|filters?\b", "Script (KSP)": r"sampler-ksp", "File format objects": r"vendor/ni-file|parser",
    "Modulators": r"library|modulation|voice_mod|lower|blocks/|lfo|source_parameters|envelope", "LFO waveforms": r"library|lfo|modulation|blocks/",
    "Modulation sources": r"library|modulation|voice_mod|lower|source_parameters", "Modulation targets": r"library|modulation|voice_mod|lower",
    "Modulation law families": r"library|modulation|voice_mod|lower", "Source modes": r"library|source|time_machine|wavetable",
    "Start conditions": r"library|native_start|keyswitch|start", "Script (Lua) classes": r"script", "Script (Lua) callbacks": r"script",
}
unassigned = []


def _keys(f):
    name = f["name"]
    ks = {name, re.sub(r"\s*\(.*?\)", "", name), f["id"].rsplit("/", 1)[-1], *f.get("keys", ())}
    ks |= {m for m in re.findall(r"\(([^)]*)\)", name) if not m.startswith("0x")}
    return [k.strip() for k in ks if len(k.strip()) >= 4 and not k.startswith("0x")]


def _rx(k, weak):
    k = re.escape(k)
    return re.compile(rf"(^|[^A-Za-z0-9_]){k}@|^{k}\b" if weak else rf"(?<![A-Za-z0-9_]){k}(?![A-Za-z0-9_])", re.I)


def audit(feats):
    """Join per-mapping audit rows (id, file:line, native_thing, verdict) to features by key words found in
    native_thing, gated by the audited file path. data/audit-aliases.tsv (regex<TAB>feature id) wins first."""
    unassigned.clear()
    if not os.path.isdir(AUDIT):
        return
    byid = {f["id"]: f for f in feats}
    alias = []
    if os.path.exists(ALIASES):
        for line in open(ALIASES, encoding="utf-8"):
            if line.strip() and not line.startswith("#"):
                rx, fid = line.rstrip("\n").split("\t")[:2]
                alias.append((re.compile(rx, re.I), fid.lower()))
    matchers = []
    for f in feats:
        gate = re.compile(AREA_PATH[f["area"]], re.I) if f["area"] in AREA_PATH else None
        if f["product"] == "UVI":
            gate = re.compile("sampler-uvi", re.I)
        elif f["product"] == "Kontakt" and f["area"] in ("Effects", "Modulators", "LFO waveforms", "Modulation sources", "Source modes"):
            gate = re.compile(AREA_PATH[f["area"]] + "|sampler-kontakt|sampler-core", re.I)
        for k in _keys(f):
            matchers.append((len(k), _rx(k, len(k) < 6 and not re.search(r"[0-9_]|[a-z][A-Z]", k)), gate, f["id"]))
    matchers.sort(key=lambda m: -m[0])
    rows = {}  # feature id -> [(verdict, native_thing)]
    for fn in sorted(os.listdir(AUDIT)):
        if not fn.endswith(".tsv"):
            continue
        for r in build.tsv(os.path.join(AUDIT, fn)):
            text, where, v = r.get("native_thing", ""), r.get("file:line", ""), r.get("verdict", "").upper()
            hit = next((fid for rx, fid in alias if rx.search(text) or rx.search(where)), None)
            hit = hit or next((fid for _, rx, gate, fid in matchers if rx.search(text) and (gate is None or gate.search(where))), None)
            if hit in byid:
                rows.setdefault(hit, []).append((v, text))
            else:
                unassigned.append((fn[:-4], r.get("id", ""), where, text, v))
    for fid, items in rows.items():
        f = byid[fid]
        n, p = len(items), sum(v == "PROVED" for v, _ in items)
        bad = [x for v, x in items if v == "DEBUNKED"]
        f["proof"] = "debunked" if bad else "proved" if p == n else "unproven"
        f["audit"] = [p, n, len(bad)]
        f["note"] = (f"{p} of {n} audited mappings proved" + (f"; mismatch found: {bad[0][:90]}" if bad else "")
                     + (f"; {f['note']}" if f["note"] else ""))[:260]


def finalize(feats):
    """done = implemented AND proved. Implemented but unproven is partial (unproven)."""
    for f in feats:
        if f["status"] == "todo":
            f["proof"] = "n/a"
        elif f["proof"] == "debunked":
            f["status"] = "partial"
        elif f["status"] == "done" and f["proof"] != "proved":
            f["status"] = "partial"


def all_features():
    k = "Kontakt"
    law, per = targets()
    f = effects() + law + per + file_format() + ksp_families()
    f += engine_features("modulator", k, "Modulators", lambda r: re.sub(r" \(0x[0-9A-F]+\)", "", r["name"]).replace("BPar", "").replace("_", " ") + f" ({r['key']})" if r["key"] != r["name"] else r["name"])
    f += engine_features("lfo-wave", k, "LFO waveforms")
    f += engine_features("mod-source", k, "Modulation sources")
    f += engine_features("source-mode", k, "Source modes")
    f += engine_features("start-mode", k, "Start conditions")
    f += engine_features("start-join", k, "Start conditions", lambda r: "Combine: " + r["name"])
    f += uvi_features()
    audit(f)
    overrides(f)
    finalize(f)
    return f


def stats(fs):
    c = Counter(x["status"] for x in fs)
    n = len(fs) or 1
    pr = Counter(x["proof"] for x in fs)
    return {"proved": pr["proved"], "fixing": pr["debunked"], "done": c["done"], "partial": c["partial"], "todo": c["todo"], "total": len(fs),
            "pct": round(100 * (c["done"] + 0.5 * c["partial"]) / n)}


CSS = """
:root{--bg:#f6f7f9;--fg:#15181d;--mut:#5d6672;--card:#fff;--line:#e2e5ea;--done:#1f9d55;--partial:#d99a00;--todo:#c8cdd4}
@media(prefers-color-scheme:dark){:root{--bg:#101215;--fg:#e8eaed;--mut:#9aa3ad;--card:#181b20;--line:#2a2f36;--done:#3ecf7e;--partial:#e8b730;--todo:#3a4048}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}
main{max-width:860px;margin:0 auto;padding:20px 14px 60px}h1{font-size:1.7rem;margin:.2em 0}h2{font-size:1.25rem;margin:1.8em 0 .5em}
p.lead{color:var(--mut);margin:.3em 0 1em}.bar{display:flex;height:10px;border-radius:5px;overflow:hidden;background:var(--todo)}
.bar i{display:block;height:100%}.bar .d{background:var(--done)}.bar .p{background:var(--partial)}
details{background:var(--card);border:1px solid var(--line);border-radius:10px;margin:10px 0}
summary{cursor:pointer;padding:12px 14px;list-style:none;display:grid;grid-template-columns:1fr auto;gap:4px 12px;align-items:center}
summary::-webkit-details-marker{display:none}summary b{font-size:1.02rem}summary .bar{grid-column:1/-1}
.cnt{color:var(--mut);font-size:.85rem;white-space:nowrap}ul{list-style:none;margin:0;padding:0;border-top:1px solid var(--line)}
li{display:grid;grid-template-columns:7.2em 1fr;gap:2px 10px;padding:8px 14px;border-bottom:1px solid var(--line)}li:last-child{border:0}
.pill{font-size:.75rem;font-weight:600;padding:2px 8px;border-radius:99px;text-align:center;height:1.6em;align-self:start;color:#fff}
.pill.done{background:var(--done)}.pill.partial{background:var(--partial);color:#201800}.pill.fix{background:#c2410c;color:#fff}.pill.todo{background:var(--todo);color:var(--fg)}
.n{color:var(--mut);font-size:.85rem}.n a{color:inherit}.tot{display:grid;grid-template-columns:1fr 1fr;gap:10px}
.tot div{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px}.tot b{font-size:1.4rem}
nav{display:flex;gap:8px;flex-wrap:wrap;margin:14px 0}nav button{border:1px solid var(--line);background:var(--card);color:var(--fg);border-radius:99px;padding:5px 12px;font:inherit;font-size:.85rem}
nav button[aria-pressed=true]{background:var(--fg);color:var(--bg)}.hide{display:none}
@media(max-width:520px){li{grid-template-columns:1fr}.pill{width:max-content;padding:2px 10px}.tot{grid-template-columns:1fr}}
footer{color:var(--mut);font-size:.8rem;margin-top:2.5em}
"""
JS = """
for(const b of document.querySelectorAll('nav button'))b.onclick=()=>{
 for(const x of document.querySelectorAll('nav button'))x.setAttribute('aria-pressed',x===b);
 const s=b.dataset.s;for(const l of document.querySelectorAll('li'))l.classList.toggle('hide',s!=='all'&&!l.classList.contains(s));
 if(s!=='all')for(const d of document.querySelectorAll('details'))d.open=true};
"""


def bar(s):
    t = s["total"] or 1
    return f'<div class="bar" role="img" aria-label="{s["pct"]}% complete"><i class="d" style="width:{100*s["done"]/t:.1f}%"></i><i class="p" style="width:{100*s["partial"]/t:.1f}%"></i></div>'


def item(x):
    e = html.escape
    label = LABEL[x["status"]] + (" · proved" if x["status"] == "done" else "")
    if x["status"] == "partial":
        label = {"debunked": "Fixing", "unproven": "Partial (unproven)"}.get(x["proof"], "Partial")
    cls = x["status"] + (" fix" if x["proof"] == "debunked" else "")
    link = f' <a href="{REPO}{x["src"]}">source</a>' if x["src"] else ""
    return (f'<li class="{x["status"]}"><span class="pill {cls}">{label}</span><span><b>{e(x["name"])}</b>'
            f'<div class="n">{e(x["note"])}{link}</div></span></li>')


def render(fs):
    e = html.escape
    parts = []
    for product, order in (("Kontakt", KONTAKT_ORDER), ("UVI", UVI_ORDER)):
        ps = [x for x in fs if x["product"] == product]
        s = stats(ps)
        parts.append(f'<h2>{product}</h2><p class="lead">{s["done"]} done, {s["partial"]} partial, {s["todo"]} not started of {s["total"]} features ({s["pct"]}%)</p>{bar(s)}')
        for area in order:
            af = [x for x in ps if x["area"] == area]
            if not af:
                continue
            a = stats(af)
            order_key = lambda x: ({"done": 0, "partial": 1, "todo": 2}[x["status"]], x["name"].lower())
            lis = "".join(item(x) for x in sorted(af, key=order_key))
            parts.append(f'<details{"" if area in COLLAPSED else " open"}><summary><b>{e(area)}</b><span class="cnt">{a["done"]}/{a["total"]} done, {a["partial"]} partial</span>{bar(a)}</summary><ul>{lis}</ul></details>')
    o = stats(fs)
    return f"""<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>KONTRA feature tracker</title><meta name="description" content="What KONTRA supports of the Kontakt and UVI formats, regenerated from the repository."><style>{CSS}</style></head><body><main>
<h1>KONTRA feature tracker</h1>
<p class="lead">The finite set of Kontakt and UVI features KONTRA has to implement, independent of any one library. Status is regenerated from the source tree on every push to main. Updated {time.strftime("%Y-%m-%d")}.</p>
<div class="tot"><div><b>{o["pct"]}%</b><br>overall (partial counts half)</div><div><b>{o["done"]}</b> done · <b>{o["partial"]}</b> partial · <b>{o["todo"]}</b> not started<br>{o["total"]} features, {o["proved"]} proved against the original, {o["fixing"]} being fixed</div></div>
<nav aria-label="Filter"><button data-s="all" aria-pressed="true">All</button><button data-s="done" aria-pressed="false">Done</button><button data-s="partial" aria-pressed="false">Partial</button><button data-s="todo" aria-pressed="false">Not started</button></nav>
{"".join(parts)}
<footer>Done: implemented and proved against the original. Partial: implemented but unproven, approximate, or a subset. Fixing: an audit found a mismatch. Not started: the importer reports it as unsupported. KONTRA is alpha software; Done does not claim sample-exact parity with the original instrument.</footer>
</main><script>{JS}</script></body></html>"""


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    out = args[0] if args else os.path.join(HERE, "_site")
    fs = all_features()
    os.makedirs(out, exist_ok=True)
    open(os.path.join(out, "index.html"), "w", encoding="utf-8").write(render(fs))
    json.dump([{k: v for k, v in x.items() if k != "keys"} for x in fs], open(os.path.join(out, "features.json"), "w"), indent=1)
    for p in ("Kontakt", "UVI"):
        for a in OrderedDict.fromkeys(x["area"] for x in fs if x["product"] == p):
            print(p, a, stats([x for x in fs if x["product"] == p and x["area"] == a]))
    print("total", stats(fs))
    by = Counter(u[0] for u in unassigned)
    print("audit rows unassigned:", len(unassigned), dict(by))
    if "--unassigned" in sys.argv:
        for u in unassigned:
            print("\t".join(u))


if __name__ == "__main__":
    main()
