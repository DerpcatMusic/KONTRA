#!/usr/bin/env python3
"""Regenerate the KONTRA coverage dashboard: docs/coverage/index.html + data.json.

    python3 tools/coverage/build.py

No cargo, no plugin build. Inputs (all read-only):
  format   docs/re/kontakt-catalog/{ser_classes,reader_schemas}.tsv, parser_coverage.md
  engine   tools/coverage/engine.tsv (hand rows, each with an evidence path and a code probe),
           modulation targets (specs/modulation-laws.md, snapshotted into tools/coverage/data/),
           effect modules (crates/sampler-kontakt/src/effects.rs MODULES + ser_classes)
  library  tools/coverage/cache/libraries.json (written by tools/coverage/survey.py)
  uvi      ~/.cache/kontakto-wave3/specs/uvi-elements.tsv when present
Statuses: exact | mapped | approx | reported | absent (see the legend in the page).
Nothing decrypted is read or written: no script source, no sample data.
"""
import json, os, re, shutil, time
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
SPECS = os.path.expanduser("~/.cache/kontakto-wave2/specs")
UVI = os.path.expanduser("~/.cache/kontakto-wave3/specs/uvi-elements.tsv")
CAT = os.path.join(ROOT, "docs/re/kontakt-catalog")


def tsv(path):
    rows = [l.rstrip("\n").split("\t") for l in open(path, encoding="utf-8") if l.strip()]
    return [dict(zip(rows[0], r)) for r in rows[1:]]


_src = {}
def source(name):
    """Text of the first source file called `name` under crates/ or vendor/ni-file/."""
    if name not in _src:
        _src[name] = ""
        for base in ("crates", "vendor/ni-file"):
            for d, _, fs in os.walk(os.path.join(ROOT, base)):
                if "target" in d.split(os.sep):
                    continue
                for f in fs:
                    p = os.path.join(d, f)
                    if f == name or p.endswith("/" + name):
                        _src[name] = open(p, encoding="utf-8", errors="replace").read()
                        return _src[name]
    return _src[name]


def probe_ok(probe):
    if not probe:
        return None
    neg = probe.startswith("!")
    f, _, rx = probe.lstrip("!").partition("::")
    hit = re.search(rx, source(f)) is not None
    return hit != neg


# ---------------------------------------------------------------- format layer
def format_layer():
    classes = tsv(os.path.join(CAT, "ser_classes.tsv"))
    schemas = {}
    for r in tsv(os.path.join(CAT, "reader_schemas.tsv")):
        schemas.setdefault(r["class"], []).append(r)
    cov = {}
    for line in open(os.path.join(CAT, "parser_coverage.md"), encoding="utf-8"):
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(c) == 6 and c[0].startswith("0x"):
            cov[c[1]] = c
    rows = []
    for c in classes:
        name = c["class"]
        sch = schemas.get(name, [])
        cv = cov.get(name, ["", name, "", "", "", ""])
        native, hand, note = cv[3], cv[4], cv[5]
        stub = "no stream reader" in note or native in ("stub", "not-a-reader", "")
        typed = [s for s in sch if s["status"] == "typed"]
        status = "absent" if stub and not hand else "exact" if not stub else "mapped"
        rows.append({
            "class": re.sub(r"^(class|struct) ", "", name), "sertype": c["sertype"],
            "native_version": c["version"], "native_schema": native, "hand_reader": hand, "status": status,
            "typed": f"{len(typed)}/{len(sch)}", "layout": (typed or sch or [{}])[-1].get("layout", ""),
            "note": note, "reader_va": c["reader_va"],
        })
    return rows


# ---------------------------------------------------------------- engine layer
def engine_rows():
    rows = []
    for line in open(os.path.join(HERE, "engine.tsv"), encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        cat, key, name, status, evidence, probe, note = (line.rstrip("\n").split("@@") + [""] * 7)[:7]
        rows.append({"category": cat, "key": key, "name": name, "status": status, "evidence": evidence,
                     "probe": probe, "note": note, "probe_ok": probe_ok(probe)})
    return rows


def target_table():
    snap = os.path.join(HERE, "data/modulation-targets.tsv")
    law = os.path.join(SPECS, "modulation-laws.md")
    if os.path.exists(law):
        body = open(law, encoding="utf-8").read().split("```tsv")[1].split("```")[0].strip()
        os.makedirs(os.path.dirname(snap), exist_ok=True)
        open(snap, "w", encoding="utf-8").write(body + "\n")
    return [r for r in tsv(snap) if r["target_id"].isdigit()]


H_IDS = {2, 0x44, *range(0x71, 0x8A), *range(0xB8, 0xBC), 0xBD, 0xBF, 0xC0, *range(0xC4, 0xCC)}


def law_family(i, template):
    if "fcubic" in template:
        return "cubic"
    if template.startswith("$out := $in * {}"):
        return "mul"
    if "1.0 - (1.0 - $in)" in template:
        return "invert-add"
    return "complement" if "(1.0 - (1.0" in template else "add"


def target_rows(engine):
    by_key = {r["key"]: r for r in engine if r["category"] == "target"}
    lib = source("library.rs")
    route = lib[lib.index("    fn route("):lib.index("    fn lfo(")]
    mapped = set(re.findall(r'"([A-Za-z_]+)"', route))
    out = []
    for t in target_table():
        i, name = int(t["target_id"]), t["param"]
        fam = law_family(i, t["template"])
        o = by_key.get(name)
        if o:
            status, ev, note = o["status"], o["evidence"], o["note"]
        elif name in mapped:
            status, ev, note = "mapped", "sampler-kontakt/src/library.rs route()", "mapped by route(); law family " + fam
        else:
            status, ev, note = "reported", "sampler-kontakt/src/library.rs route()", "route() reports 'modulation target' not modeled"
        out.append({"category": "target", "key": str(i), "name": f"{name} (ID {i})", "status": status,
                    "evidence": ev + "; specs/modulation-laws.md (law " + fam + ")", "note": note,
                    "probe": "", "probe_ok": None, "law": fam})
    return out


def effect_rows(engine, fmt):
    over = {r["key"].lower(): r for r in engine if r["category"] == "effect"}
    decoded = {r["class"]: r for r in fmt}
    seen, out = set(), []
    for c in tsv(os.path.join(CAT, "ser_classes.tsv")):
        cls = re.sub(r"^(class|struct) ", "", c["class"])
        if not cls.startswith("BParFX") or cls == "BParFX":
            continue
        key = f"0x{int(c['sertype'], 16):02x}"
        dup = key in seen
        o = None if dup else over.get(key)
        seen.add(key)
        f = decoded.get(cls, {})
        out.append({
            "category": "effect", "key": key, "name": f"{cls} ({o['name'] if o else '-'})".replace(" (-)", ""),
            "status": o["status"] if o else "reported",
            "evidence": o["evidence"] if o else "sampler-kontakt/src/effects.rs chain_with: 'effect ... NotModeled'",
            "note": (o["note"] if o else "shares its serialization id with an earlier class" if dup
                     else "no kernel; reported as unmodeled when present in a rack")
                    + f"; format: {f.get('status', '?')} ({f.get('typed', '')} typed)",
            "probe": o["probe"] if o else "", "probe_ok": probe_ok(o["probe"]) if o else None})
    return out


# ---------------------------------------------------------------- libraries / uvi
def library_layer():
    p = os.path.join(HERE, "cache/libraries.json")
    return json.load(open(p)) if os.path.exists(p) else {"surveyed": None, "libraries": []}


def uvi_layer():
    if not os.path.exists(UVI):
        return {"present": False, "rows": [], "columns": []}
    rows = tsv(UVI)
    return {"present": True, "columns": list(rows[0]) if rows else [], "rows": rows}


def main():
    fmt = format_layer()
    eng = engine_rows()
    targets = target_rows(eng)
    effects = effect_rows(eng, fmt)
    engine = [r for r in eng if r["category"] not in ("target", "effect")] + targets + effects
    libs = library_layer()
    data = {"generated": time.strftime("%Y-%m-%d %H:%M"), "format": fmt, "engine": engine,
            "libraries": libs, "uvi": uvi_layer(),
            "totals": {"format": Counter(r["status"] for r in fmt),
                       "engine": Counter(r["status"] for r in engine)}}
    out = os.path.join(ROOT, "docs/coverage")
    os.makedirs(out, exist_ok=True)
    blob = json.dumps(data, separators=(",", ":"))
    open(os.path.join(out, "data.json"), "w").write(json.dumps(data, indent=1))
    page = open(os.path.join(HERE, "page.html"), encoding="utf-8").read()
    open(os.path.join(out, "index.html"), "w", encoding="utf-8").write(page.replace("/*DATA*/null", blob.replace("</", "<\\/")))
    stale = [r["name"] for r in engine if r.get("probe_ok") is False]
    print("format", dict(data["totals"]["format"]), "| engine", dict(data["totals"]["engine"]),
          "| libraries", len(libs["libraries"]), "| stale probes", stale)


if __name__ == "__main__":
    main()
