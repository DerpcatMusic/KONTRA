#!/usr/bin/env python3
"""Kontakt 8 KSP surface map: symbol catalogue, coverage, corpus weight, doctor.

  refresh --manual PDF [--binary EXE]   rebuild the symbol universe (SYMBOLS.tsv) from
                                        the KSP reference manual and the Kontakt 8 binary
  sync                                  recompute status, owner, tests and corpus rank;
                                        write docs/re/ksp/SYMBOLS.tsv and COVERAGE.md
  check                                 CI gate: every row has a status and a test link,
                                        full rows keep their test (docs/re/ksp/FULL_UNTESTED.txt
                                        lists the known debt), and the committed files match a fresh sync
  doctor FILE... [--fail-on STATUS]     per-symbol status for KSP text (.ksp) or .nki
                                        instruments (scripts extracted with dump_scripts)

Status vocabulary (KONTRA, from parity_ledger.rs): full = Implemented, simplified =
Approximate, stub = Stub, unsupported = Unsupported or no ledger/frontend binding.
Rows with class `modern-extension` are KONTRA proposals, not Kontakt symbols.
"""
import argparse
import collections
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "docs/re/ksp"
SYMBOLS = OUT / "SYMBOLS.tsv"
COVERAGE = OUT / "COVERAGE.md"
LEDGER = ROOT / "crates/sampler-ksp/src/parity_ledger.rs"
BUILTINS = ROOT / "crates/sampler-ksp/src/builtins.rs"
CORPUS = ROOT / "docs/architecture-v2/KSP_COVERAGE.md"
SCRATCH = Path("/data/dev/workspace/scratch")
TEST_DIRS = [ROOT / "crates/sampler-ksp/tests", ROOT / "crates/sampler-kontakt/tests"]

STATUSES = ("full", "simplified", "stub", "unsupported")
LEDGER_STATUS = {"Implemented": "full", "Approximate": "simplified", "Stub": "stub",
                 "Unsupported": "unsupported", "Fault": "unsupported"}
CATEGORY_ORDER = ["callback", "command", "keyword", "datatype", "ui_control", "preprocessor",
                  "constant", "variable", "engine_par", "control_par", "event_par", "zone_par",
                  "loop_par", "sample_par", "other_par"]
KEYWORDS = ["declare", "const", "polyphonic", "function", "call", "if", "else", "end", "while",
            "select", "case", "continue", "exit", "and", "or", "not", "xor", "mod"]
PREPROCESSOR = ["SET_CONDITION", "RESET_CONDITION", "USE_CODE_IF", "USE_CODE_IF_NOT", "END_USE_CODE"]
# KONTRA proposals (docs/architecture-v2/KSP_MODERN.md). Not implemented: status unsupported.
MODERN = [
    ("$EVENT_PAR_MPE_PITCH", "per-note pitch bend in semitones (MPE / MIDI 2.0 per-note pitch)"),
    ("$EVENT_PAR_MPE_PRESSURE", "per-note channel pressure, 0..1 (MPE pressure)"),
    ("$EVENT_PAR_MPE_SLIDE", "per-note slide / timbre, 0..1 (MPE CC74)"),
]
COLUMNS = ["id", "class", "category", "name", "value", "since", "section", "signature",
           "kontra_sig", "restriction", "binary", "source", "status", "owner", "detail",
           "tests", "corpus", "corpus_rank"]
SIGIL_CATEGORY = {"%": "variable", "!": "constant", "~": "constant", "$": "constant"}
PAR_FAMILIES = {"ENGINE_PAR": "engine_par", "CONTROL_PAR": "control_par", "EVENT_PAR": "event_par",
                "ZONE_PAR": "zone_par", "LOOP_PAR": "loop_par", "SAMPLE_PAR": "sample_par"}


def category_for(name):
    """Category of a sigil-prefixed constant or variable name."""
    if name[0] != "$":
        return SIGIL_CATEGORY[name[0]]
    for prefix, cat in PAR_FAMILIES.items():
        if name[1:].startswith(prefix + "_"):
            return cat
    return "constant"


# ---------------------------------------------------------------- manual (refresh)

def parse_manual(text):
    """Return {(category, name): {section, signature, restriction, since}} from the manual text.

    A command title is `name()` at column 0 (optionally `a()/b()`); its signature is the
    indented syntax line under it. Titles without a syntax line (See Also lists) are ignored.
    """
    lines = text.splitlines()
    body = next(i for i, l in enumerate(lines) if i > 100 and l.strip() == "1. Disclaimer")
    history = next(i for i, l in enumerate(lines) if i > body and l.strip() == "29. Version History")
    rows, chapter, entry = {}, (0, ""), None

    def row(cat, name):
        return rows.setdefault((cat, name), {"section": chapter[1], "signature": "",
                                             "restriction": "", "since": ""})

    def syntax_after(i, name):
        """Signature from the first `name(args)` line after a title; wrapped lines are joined."""
        for j in range(i + 1, min(i + 4, len(lines))):
            t = lines[j].strip()
            if not t.startswith(name + "("):
                continue
            if ")" not in t and j + 1 < len(lines):
                t += " " + lines[j + 1].strip()
            m = re.match(rf"{name}\(([^()]*)\)", t)
            if m and m[1].strip():
                return f"{name}({re.sub(r'[<>]', '', m[1])})"
        return ""

    restr = re.compile(r"(can only be used|cannot be used|only be used|can only be)")
    body_text = "\n".join(lines[body:history])
    for i in range(body, history):
        l = lines[i]
        s = l.strip()
        m = re.fullmatch(r"(\d{1,2})\. ([A-Z].*?)\s*", l)
        if m and int(m[1]) == chapter[0] + 1:
            chapter, entry = (int(m[1]), m[2]), None
            continue
        indent = len(l) - len(l.lstrip(" "))
        if chapter[0] == 3 and indent <= 1:
            m = re.fullmatch(r"on ([a-z_0-9]+(?:/[a-z_0-9]+)?)", s)
            if m:
                for n in m[1].split("/"):
                    row("callback", f"on {n}")
                entry = ("callback", f"on {m[1].split('/')[0]}")
                continue
        if indent == 0:
            m = re.fullmatch(r"([a-z][a-z0-9_]*)\(\)(?:/([a-z][a-z0-9_]*)\(\))?", s)
            if m:
                for n in [g for g in m.groups() if g]:
                    sig = syntax_after(i, n)
                    if sig:
                        r = row("command", n)
                        r["signature"], r["section"] = sig, chapter[1]
                entry = ("command", m[1]) if syntax_after(i, m[1]) else None
                continue
        if indent <= 2:
            m = re.fullmatch(r"([a-z][a-z0-9_]*)\(([^()]*)\)(\s{2,}\S.*)?", s)
            if m and (re.search(r"[a-z]", m[2]) or m[3]):
                r = row("command", m[1])
                r["signature"] = r["signature"] or f"{m[1]}({m[2]})"
                entry = ("command", m[1])
                continue
            if chapter[0] == 8 and re.fullmatch(r"ui_[a-z_]+", s):
                row("ui_control", s)
                entry = ("ui_control", s)
                continue
            m = re.fullmatch(r"([$%~@!?]) \((.*)\)", s)
            if m and chapter[0] == 4:
                row("datatype", m[1])["section"] = m[2]
                continue
            m = re.fullmatch(r"([$%!~][A-Z][A-Z0-9_]*)", s)
            if m:
                entry = (category_for(m[1]), m[1])
                row(*entry)
        if entry and restr.search(s) and not rows[entry]["restriction"]:
            rows[entry]["restriction"] = re.sub(r"\s+", " ", s.lstrip("•").strip())
    # Built-in $/%/!/~ tokens and keywords mentioned anywhere in the body.
    for m in re.finditer(r"(?<![A-Za-z0-9_])([$%!~])([A-Z][A-Z0-9_]*)(?![A-Za-z0-9_])", body_text):
        name = m[1] + m[2]
        if not name.endswith("_"):
            row(category_for(name), name)
    for kw in KEYWORDS:
        if re.search(rf"(?<![A-Za-z0-9_]){kw}(?![A-Za-z0-9_])", body_text):
            row("keyword", kw)
    for kw in PREPROCESSOR:
        if re.search(rf"(?<![A-Za-z0-9_]){kw}(?![A-Za-z0-9_])", body_text):
            row("preprocessor", kw)
    # Version history: the oldest changelog block whose New Features mention a symbol wins.
    block, section = None, None
    for l in lines[history:]:
        s = l.strip()
        m = re.fullmatch(r"Kontakt (\d+(?:\.\d+)*)", s)
        if m:
            block, section = m[1], None
            continue
        if s in ("New Features", "Improved Features", "Fixes"):
            section = s
            continue
        if block and section == "New Features":
            for key in re.findall(r"\b([a-z][a-z0-9_]*)\(\)", s):
                if ("command", key) in rows:
                    rows[("command", key)]["since"] = block
            for key in re.findall(r"\$[A-Z][A-Z0-9_]*", s):
                if (category_for(key), key) in rows:
                    rows[(category_for(key), key)]["since"] = block
            for key in re.findall(r"\bon ([a-z_]+)\b", s):
                if ("callback", f"on {key}") in rows:
                    rows[("callback", f"on {key}")]["since"] = block
    return rows


def binary_evidence(exe, names):
    """Evidence per name in a PE: lea/abs refs to an exact string, message-only or absent."""
    import numpy as np
    data = Path(exe).read_bytes()
    e_lfanew = int.from_bytes(data[0x3C:0x40], "little")
    nsec = int.from_bytes(data[e_lfanew + 6:e_lfanew + 8], "little")
    opt = int.from_bytes(data[e_lfanew + 20:e_lfanew + 22], "little")
    base = int.from_bytes(data[e_lfanew + 24 + 28:e_lfanew + 24 + 32], "little")
    secs = []
    for k in range(nsec):
        o = e_lfanew + 24 + opt + 40 * k
        sname = data[o:o + 8].rstrip(b"\0").decode(errors="replace")
        vsz, va, rsz, raw = (int.from_bytes(data[o + d:o + d + 4], "little") for d in (8, 12, 16, 20))
        secs.append((sname, va, vsz, raw, rsz))
    wanted = {n for n in names}
    rva_of, messages = {}, collections.defaultdict(set)
    ident = re.compile(rb"[\x20-\x7e]{2,160}")
    for sname, va, vsz, raw, rsz in secs:
        if not raw or not rsz:
            continue
        blob = data[raw:raw + rsz]
        for mt in ident.finditer(blob):
            s = mt.group().decode()
            a, b = mt.start(), mt.end()
            bounded = (a == 0 or not 0x20 <= blob[a - 1] <= 0x7E) and (b >= len(blob) or not 0x20 <= blob[b] <= 0x7E)
            if bounded and s in wanted:
                rva_of.setdefault(s, va + a)
            for call in re.findall(r"([A-Za-z_$%!~][A-Za-z0-9_]*)\(", s):
                if call in wanted:
                    messages[call].add(s[:140])
            m = re.match(r"([a-z][a-z0-9_]*)\(\)", s)
            if m and m[1] in wanted and re.search(r"can only|cannot|only be used", s):
                messages[m[1]].add(s[:140])
    refs = collections.Counter()
    text = next((x for x in secs if x[0] == ".text"), None)
    if text and rva_of:
        _, tva, tsz, traw, trsz = text
        buf = np.frombuffer(data, dtype=np.uint8, count=trsz, offset=traw)
        n = trsz - 3
        b = buf
        disp = (b[0:n].astype(np.uint32) | (b[1:n + 1].astype(np.uint32) << 8)
                | (b[2:n + 2].astype(np.uint32) << 16) | (b[3:n + 3].astype(np.uint32) << 24))
        disp = disp.view(np.int32).astype(np.int64)
        tgt = tva + np.arange(n, dtype=np.int64) + 4 + disp
        lea = np.zeros(n, dtype=bool)
        lea[3:] = (((b[0:n - 3] & 0xF0) == 0x40) & (b[1:n - 2] == 0x8D) & ((b[2:n - 1] & 0xC7) == 0x05))
        keys = np.array(sorted(set(rva_of.values())), dtype=np.int64)
        for t in tgt[np.nonzero(np.isin(tgt, keys) & lea)[0]]:
            refs[int(t)] += 1
        del disp, tgt, lea
    absrefs = collections.Counter()
    if rva_of:
        vas = np.array(sorted(set(rva_of.values())), dtype=np.int64) + base
        for sname, va, vsz, raw, rsz in secs:
            if sname in (".rdata", ".data", "_RDATA") and rsz >= 8:
                arr = np.frombuffer(data, dtype=np.uint64, count=rsz // 8, offset=raw)
                for x in arr[np.isin(arr.astype(np.int64), vas)]:
                    absrefs[int(x) - base] += 1
    out = {}
    for n in names:
        rva = rva_of.get(n)
        if rva is not None and refs.get(rva):
            kind = "lea-ref"
        elif rva is not None and absrefs.get(rva):
            kind = "data-ref"
        elif rva is not None:
            kind = "string"
        elif messages.get(n):
            kind = "message"
        else:
            kind = "absent"
        msg = sorted(messages.get(n, ()))[:1]
        out[n] = kind + (f" | {msg[0]}" if msg and kind != "message" else "")
        if kind == "message":
            out[n] = f"message | {msg[0]}" if msg else "message"
    return out


def refresh(manual_pdf, binary, existing):
    """Universe rows: manual headings and tokens, ledger names, builtins, KONTRA proposals."""
    text = subprocess.run(["pdftotext", "-layout", str(manual_pdf), "-"], check=True,
                          capture_output=True, text=True).stdout
    rows = parse_manual(text)
    blank = {"section": "ledger", "signature": "", "restriction": "", "since": ""}
    for name in ledger_entries():
        # `#` marks a digit or word family template (`$ENGINE_PAR_RV#_DAMPING`); a trailing `_`
        # is a prefix placeholder and `@` entries are internal diagnostics. Neither is a symbol.
        if not re.fullmatch(r"[$%!~]?[A-Za-z_][A-Za-z0-9_#]*|on [a-z_/]+", name) or name.endswith("_"):
            continue
        if name.startswith("on "):
            key = ("callback", name)
        elif name[0] in "$%!~":
            key = (category_for(name), name)
        elif name in KEYWORDS:
            key = ("keyword", name)
        elif re.fullmatch(r"[a-z][a-z0-9_]*", name):
            key = ("command", name)
        else:
            continue
        rows.setdefault(key, dict(blank))
    for name in builtins_table()[0]:
        if not name.startswith("_"):
            rows.setdefault(("command", name), dict(blank, section="kontra-builtins"))
    universe = {}
    for (cat, name), meta in rows.items():
        universe[f"{cat}:{name}"] = dict(meta, category=cat, name=name, **{"class": "compat"})
    for name, detail in MODERN:
        universe[f"event_par:{name}"] = {
            "category": "event_par", "name": name, "class": "modern-extension",
            "section": "KONTRA proposal", "signature": "", "restriction": "", "since": "",
            "detail": detail}
    evidence = {}
    if binary:
        names = [r["name"][3:] if r["category"] == "callback" else r["name"] for r in universe.values()]
        evidence = binary_evidence(binary, names)
    prior = {r["id"]: r for r in existing}
    for rid, row in universe.items():
        row["id"] = rid
        old = prior.get(rid, {})
        bname = row["name"][3:] if row["category"] == "callback" else row["name"]
        row["binary"] = evidence.get(bname, old.get("binary", "")) if binary else old.get("binary", "")
        if "#" in bname:
            row["binary"] = "template"
    return list(universe.values())


# ---------------------------------------------------------------- KONTRA facts

def ledger_entries():
    src = LEDGER.read_text()
    pat = re.compile(r'LedgerEntry \{ name: "((?:[^"\\]|\\.)*)", status: Status::(\w+), '
                     r'owner: "([^"]*)", spec: "[^"]*", detail: (\w+|"(?:[^"\\]|\\.)*") \}')
    consts = {m[1]: m[2] for m in re.finditer(r'const (\w+): &str =\s*"([^"]*)";', src)}
    out = {}
    for m in pat.finditer(src):
        detail = m[4][1:-1] if m[4].startswith('"') else consts.get(m[4], m[4])
        out.setdefault(m[1], {"status": LEDGER_STATUS[m[2]], "owner": m[3], "detail": detail})
    return out


def builtins_table():
    """Return ({name: sig}, {name: value}, {name: owner}) from builtins.rs."""
    src = BUILTINS.read_text()
    sigs, owners = {}, {}
    for m in re.finditer(r'^\s+(\w+) "([^"]+)" \[([^\]]*)\] (\d+) (\w+);', src, re.M):
        args = " ".join(m[3].split())
        sigs[m[2]] = f"({args}{' +' + m[4] + ' opt' if m[4] != '0' else ''}) -> {m[5]}"
        owners[m[2]] = f"crates/sampler-ksp/src/builtins.rs:{src[:m.start()].count(chr(10)) + 1}"
    valued = {}
    block = src.split("const VALUED", 1)[1].split("];", 1)[0]
    for m in re.finditer(r'\("([^"]+)",\s*(-?\d+)\)', block):
        valued[m[1]] = m[2]
    return sigs, valued, owners


def corpus_counts():
    counts, section = {}, ""
    for line in CORPUS.read_text().splitlines():
        if line.startswith("## "):
            section = line[3:]
            continue
        m = re.match(r"\| `([^`]+)` \| (\d+) \|", line)
        if m:
            key = f"on {m[1]}" if section == "Callbacks" else m[1]
            counts[key] = int(m[2])
    return counts


def test_index():
    files = []
    for d in TEST_DIRS:
        if d.is_dir():
            files += sorted(d.glob("*.rs"))
    words, texts = collections.defaultdict(set), {}
    for f in files:
        text = f.read_text(errors="replace")
        texts[f.name] = text
        for w in set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", text)):
            words[w].add(f.name)
    return words, texts


def bare(row):
    if row["category"] == "callback":
        return row["name"]
    return row["name"].lstrip("$%!~@?").rstrip("()")


# ---------------------------------------------------------------- sync

def frontend_owner(name):
    """file:line where the frontend spells the token (keyword, sigil or directive)."""
    needles = [f"b'{name}'"] if len(name) == 1 else [f'"{name}"']
    for f in ("lexer.rs", "parser.rs", "sema.rs", "parity.rs"):
        path = ROOT / "crates/sampler-ksp/src" / f
        for i, line in enumerate(path.read_text().splitlines(), 1):
            if any(n in line for n in needles):
                return f"crates/sampler-ksp/src/{f}:{i}"
    return ""


def sync(rows):
    ledger = ledger_entries()
    sigs, valued, owners = builtins_table()
    words, texts = test_index()
    corpus = corpus_counts()
    for row in rows:
        name, key = row["name"], bare(row)
        entry = ledger.get(name)
        if row.get("class") != "compat":
            row["status"], row["owner"] = "unsupported", ""
            row["detail"] = row.get("detail") or "KONTRA proposal, not implemented"
        elif entry:
            row["status"], row["owner"], row["detail"] = entry["status"], entry["owner"], entry["detail"]
        elif row["category"] in ("keyword", "datatype", "preprocessor"):
            owner = frontend_owner(name)
            row["status"] = ("unsupported" if not owner else
                             "full" if row["category"] == "keyword" else "simplified")
            row["owner"] = owner or ""
            row["detail"] = "frontend token" if owner else "no frontend token"
        elif name in valued or key in sigs:
            row["status"] = "simplified"
            row["owner"] = owners.get(key, "")
            row["detail"] = "frontend binding without a ledger entry"
        else:
            row["status"], row["owner"] = "unsupported", ""
            row["detail"] = "no ledger entry or frontend binding"
        row["value"] = valued.get(name, valued.get(key, row.get("value", "")))
        row["kontra_sig"] = sigs.get(key, "")
        if row["category"] == "callback":
            phrase = re.escape(name[3:])
            hits = sorted(f for f, t in texts.items() if re.search(rf"\bon {phrase}\b", t))
        else:
            hits = sorted(words.get(key, ())) if key else []
        row["tests"] = f"{len(hits)} ({'; '.join(hits[:3])})" if hits else "untested"
        row["corpus"] = str(corpus.get(name, corpus.get(key, 0)))
    ranked = sorted((r for r in rows if int(r["corpus"]) > 0),
                    key=lambda r: (-int(r["corpus"]), r["id"]))
    for r in rows:
        r["corpus_rank"] = ""
    for i, r in enumerate(ranked, 1):
        r["corpus_rank"] = str(i)
    for r in rows:
        for col in COLUMNS:
            r.setdefault(col, "")
        r["class"] = r["class"] or "compat"
    rows.sort(key=lambda r: (r["class"] != "compat", CATEGORY_ORDER.index(r["category"])
                             if r["category"] in CATEGORY_ORDER else 99, r["name"]))
    return rows



def tsv(rows):
    lines = ["\t".join(COLUMNS)]
    for r in rows:
        lines.append("\t".join(str(r.get(c, "")).replace("\t", " ").replace("\n", " ") for c in COLUMNS))
    return "\n".join(lines) + "\n"


def load_rows():
    lines = SYMBOLS.read_text().splitlines()
    head = lines[0].split("\t")
    rows = []
    for line in lines[1:]:
        rows.append(dict(zip(head, line.split("\t"))))
    return rows


def pct(n, d):
    return f"{100 * n / d:.1f}%" if d else "-"


def coverage_md(rows):
    compat = [r for r in rows if r["class"] == "compat"]
    modern = [r for r in rows if r["class"] != "compat"]
    total_usage = sum(int(r["corpus"]) for r in compat) or 1
    o = ["# KSP coverage (Kontakt 8 KSP reference manual)", "",
         "Generated by `tools/re/ksp_symbols.py sync` from [SYMBOLS.tsv](SYMBOLS.tsv). Do not edit by hand; `ksp_symbols.py check` fails on drift.",
         "",
         "Status (KONTRA, from `crates/sampler-ksp/src/parity_ledger.rs`): **full** = Implemented; **simplified** = Approximate (documented approximation); **stub** = accepted and ignored; **unsupported** = no ledger entry or frontend binding.",
         "",
         "Corpus weight = instruments whose scripts mention the symbol, from the last measured run in `docs/architecture-v2/KSP_COVERAGE.md` (no corpus re-run for this map). Conformance = a test under `crates/sampler-ksp/tests` or `crates/sampler-kontakt/tests` names the symbol; `untested` otherwise.",
         "", "## Evidence per row (compat rows)", "",
         "| source | rows | binary evidence |", "|---|---|---|"]
    for src_name in ("manual", "ledger/builtins only"):
        group = [r for r in compat if ("manual" if r["section"] not in ("ledger", "kontra-builtins") else "ledger/builtins only") == src_name]
        by_binary = collections.Counter((r["binary"].split(" ")[0] or "not scanned") for r in group)
        o.append(f"| {src_name} | {len(group)} | " + ", ".join(f"{k} {v}" for k, v in by_binary.most_common()) + " |")
    o += ["", "Binary evidence: `lea-ref` = exact string referenced by a RIP-relative `lea` in .text; `data-ref` = absolute pointer in data; `string` = exact string, no reference found; `message` = name appears only inside a diagnostic text; `absent` = neither; `template` = `#` family placeholder.",
          "", "## Totals (compat rows)", "",
         "| category | symbols | full | simplified | stub | unsupported | % full | corpus-weighted % full | tested |",
         "|---|---|---|---|---|---|---|---|---|"]
    cats = sorted({r["category"] for r in compat}, key=lambda c: CATEGORY_ORDER.index(c) if c in CATEGORY_ORDER else 99)
    grand = collections.Counter()
    for c in cats + ["ALL"]:
        group = compat if c == "ALL" else [r for r in compat if r["category"] == c]
        cnt = collections.Counter(r["status"] for r in group)
        use = sum(int(r["corpus"]) for r in group)
        use_full = sum(int(r["corpus"]) for r in group if r["status"] == "full")
        tested = sum(1 for r in group if r["tests"] != "untested")
        if c == "ALL":
            o.append(f"| **all** | **{len(group)}** | **{cnt['full']}** | **{cnt['simplified']}** | **{cnt['stub']}** | **{cnt['unsupported']}** | **{pct(cnt['full'], len(group))}** | **{pct(use_full, use)}** | **{tested}** |")
        else:
            o.append(f"| {c} | {len(group)} | {cnt['full']} | {cnt['simplified']} | {cnt['stub']} | {cnt['unsupported']} | {pct(cnt['full'], len(group))} | {pct(use_full, use)} | {tested} |")
    o += ["", "Modern-extension rows (KONTRA proposals, not Kontakt symbols) are listed in SYMBOLS.tsv and KSP_MODERN.md; they are excluded from the totals above.", ""]
    nonfull = sorted((r for r in compat if r["status"] != "full" and int(r["corpus"]) > 0),
                     key=lambda r: (-int(r["corpus"]), r["id"]))
    o += ["## Top 20 not full, by corpus usage", "", "| corpus rank (all symbols) | symbol | category | status | corpus | owner |", "|---|---|---|---|---|---|"]
    for r in nonfull[:20]:
        o.append(f"| {r['corpus_rank']} | `{r['name']}` | {r['category']} | {r['status']} | {r['corpus']} | {r['owner']} |")
    o += ["", "## Backlog by family (non-full, corpus-used, top 12 per family)", ""]
    fams = collections.defaultdict(list)
    for r in nonfull:
        fams[family(r)].append(r)
    for fam in sorted(fams, key=lambda f: -sum(int(r["corpus"]) for r in fams[f])):
        items = fams[fam]
        o.append(f"### {fam} ({len(items)} symbols, {sum(int(r['corpus']) for r in items)} instrument-uses)")
        o.append("")
        o.append(", ".join(f"`{r['name']}` ({r['status']}, {r['corpus']})" for r in items[:12]))
        o.append("")
    o += ["## Kontakt 8 additions (New Features in the manual version history)", "",
          "`since` is exact only where the changelog names the symbol. Kontakt 8 engine parameters for new effects (Beat Masher, Beat Slicer, Gater, Reverse Grain, Transpose Stretch, Replika, Wavetable) are named by effect in the changelog, so they are not attributed here.",
          "", "| symbol | status | corpus |", "|---|---|---|"]
    for r in compat:
        if r["since"] == "8":
            o.append(f"| `{r['name']}` | {r['status']} | {r['corpus']} |")
    o += ["", "## Modern-extension proposals", "", "| symbol | status | note |", "|---|---|---|"]
    for r in modern:
        o.append(f"| `{r['name']}` | {r['status']} | {r['detail']} |")
    return "\n".join(o) + "\n"


def family(r):
    if r["category"] in PAR_FAMILIES.values():
        return r["category"]
    if r["category"] == "constant":
        return "constants " + (r["name"].split("_")[0] if "_" in r["name"] else r["name"])
    return f"{r['category']} / {r['section']}"


def sync_all(write):
    rows = sync(load_rows())
    text_tsv, text_md = tsv(rows), coverage_md(rows)
    if write:
        SYMBOLS.write_text(text_tsv)
        COVERAGE.write_text(text_md)
    return rows, text_tsv, text_md


FULL_UNTESTED = OUT / "FULL_UNTESTED.txt"


def allowed_full_untested():
    """Ledger `full` rows that have no conformance test yet, known debt (one id per line)."""
    if not FULL_UNTESTED.exists():
        return set()
    return {l.strip() for l in FULL_UNTESTED.read_text().splitlines() if l.strip() and not l.startswith("#")}


def full_without_test(rows, allowed):
    """A row marked full must keep its conformance test. Debt in `allowed` is listed, not new."""
    errors = []
    for r in rows:
        untested = r["tests"] == "untested"
        if r["status"] == "full" and untested and r["id"] not in allowed:
            errors.append(f"{r['id']}: marked full but no conformance test names it "
                          "(add a test, or demote the ledger row)")
        if r["id"] in allowed and not (r["status"] == "full" and untested):
            errors.append(f"{r['id']}: no longer full-without-test; remove it from FULL_UNTESTED.txt")
    return errors


def check():
    rows, text_tsv, text_md = sync_all(write=False)
    errors = []
    for r in rows:
        if r["status"] not in STATUSES:
            errors.append(f"{r['id']}: no status ({r['status']!r})")
        if not r["tests"]:
            errors.append(f"{r['id']}: no test link or 'untested'")
    errors += full_without_test(rows, allowed_full_untested())
    if SYMBOLS.read_text() != text_tsv:
        errors.append("SYMBOLS.tsv is out of date: run tools/re/ksp_symbols.py sync")
    if COVERAGE.read_text() != text_md:
        errors.append("COVERAGE.md is out of date: run tools/re/ksp_symbols.py sync")
    for e in errors[:50]:
        print("ksp-symbols:", e, file=sys.stderr)
    print(f"ksp-symbols: {len(rows)} rows, {len(errors)} problems")
    return 1 if errors else 0


# ---------------------------------------------------------------- doctor

DOCTOR_STATUSES = ("full", "simplified", "stub", "unsupported", "modern-extension")
BLOCKING = ("unsupported", "stub")


def strip_source(text):
    text = re.sub(r'"[^"\n]*"', '""', text)
    text = re.sub(r"//[^\n]*", " ", text)
    text = re.sub(r"\{[^}]*\}", " ", text)
    return text


def extract_nki(path):
    """Extract KSP scripts from a .nki with the sampler-kontakt dump_scripts example.

    The file is read in place: its .nicnt access data lives next to it in the library tree.
    """
    tmp = Path(tempfile.mkdtemp(prefix="ksp-doctor-", dir=SCRATCH if SCRATCH.is_dir() else None))
    out = tmp / "scripts"
    cmd = ["nice", "-n", "10", "build-slot", "cargo", "run", "-q", "-p", "sampler-kontakt",
           "--example", "dump_scripts", "--", str(Path(path).resolve()), str(out)]
    subprocess.run(cmd, cwd=ROOT, check=True)
    return out, tmp


def library_of(path):
    """Library folder of an input: `Lib/Instruments/X.nki` belongs to `Lib`, else its parent folder."""
    p = Path(path)
    parent = p.parent
    return parent.parent.name if parent.name.lower() == "instruments" else (parent.name or p.stem)


def scripts_of(path):
    """[(label, text)] for one input: a .ksp/.txt script, or every script of a .nki instrument."""
    p = Path(path)
    if p.suffix.lower() in (".nkb", ".nkm", ".nka"):
        sys.exit(f"doctor: {p.name}: only .nki instruments carry scripts; pass the instruments a bank or multi uses")
    if p.suffix.lower() == ".nki":
        out, cleanup = extract_nki(p)
        try:
            return [(f"{p.stem}:{f.stem}", f.read_text(errors="replace")) for f in sorted(out.glob("*.ksp"))]
        finally:
            shutil.rmtree(cleanup, ignore_errors=True)
    return [(p.name, p.read_text(errors="replace"))]


class Resolver:
    """Maps a script token (`on init`, `$gain`, `inc`) to its ledger row."""

    def __init__(self, rows):
        self.by_name = {r["name"]: r for r in rows}
        self.templates = [(re.compile(re.escape(r["name"]).replace("#", "[A-Z0-9]+")), r)
                          for r in rows if "#" in r["name"]]

    def __call__(self, key):
        if key in self.by_name:
            return self.by_name[key]
        return next((r for rx, r in self.templates if rx.fullmatch(key)), None)

    def is_builtin(self, word):
        return word in self.by_name


def scan_script(text, resolve):
    """Counter of KSP tokens used by one script (comments, strings and blocks stripped)."""
    text = strip_source(text)
    local = set(re.findall(r"\bfunction\s+([A-Za-z_]\w*)", text))
    # Script-declared variables and arrays shadow same-named manual tokens.
    declared = set(re.findall(r"\bdeclare\s+(?:polyphonic\s+|const\s+)?(?:ui_\w+\s+)?([$%~!@?][A-Za-z_]\w*)", text))
    hits = collections.Counter()
    for m in re.finditer(r"^[ \t]*on[ \t]+([a-z_]+)\b", text, re.M):
        hits[f"on {m[1]}"] += 1
    for m in re.finditer(r"(?<![A-Za-z0-9_])([$%!~@?]?)([A-Za-z_][A-Za-z0-9_]*)(\()?", text):
        sigil, word, call = m[1], m[2], m[3]
        if word in local:
            continue
        if sigil:
            key = sigil + word
            if key in declared:
                continue
            if resolve(key):
                hits[key] += 1
            elif sigil == "$" and word.isupper():
                hits[key] += 1
        elif call and resolve.is_builtin(word):
            hits[word] += 1
        elif resolve.is_builtin(word) and resolve.by_name[word]["category"] in ("keyword", "ui_control", "preprocessor"):
            hits[word] += 1
    return hits


def classify(row):
    """(status, code) for a ledger row. Full needs a conformance test; otherwise it is simplified."""
    if row is None:
        return "unsupported", "KSP-UNKNOWN"
    if row["class"] not in ("", "compat"):
        return "modern-extension", "KSP-MODERN"
    status = row["status"]
    if status == "full" and row["tests"] == "untested":
        status = "simplified"
    return status, "KSP-" + status.upper().replace("-", "_")


def symbol_report(key, count, row):
    status, code = classify(row)
    return {
        "symbol": key,
        "count": count,
        "status": status,
        "code": code,
        "category": row["category"] if row else "-",
        "conformance": "untested" if row is None or row["tests"] == "untested" else "tested",
        "corpus_rank": int(row["corpus_rank"]) if row and row["corpus_rank"] else None,
        "owner": row["owner"] if row else "",
    }


def script_report(label, hits, resolve):
    symbols = [symbol_report(k, n, resolve(k)) for k, n in hits.items()]
    symbols.sort(key=lambda s: (s["corpus_rank"] is None, s["corpus_rank"] or 0, s["symbol"]))
    total = len(symbols)
    full = sum(1 for s in symbols if s["status"] == "full")
    blockers = sorted({s["symbol"] for s in symbols if s["status"] in BLOCKING or s["code"] == "KSP-UNKNOWN"})
    pct = round(100.0 * full / total, 1) if total else 0.0
    return {"script": label, "symbols": symbols, "total": total, "full": full,
            "full_percent": pct, "blockers": blockers}


def summary_line(report):
    if not report["total"]:
        return f"{report['script']}: no KSP symbols found"
    blockers = ", ".join(report["blockers"][:8]) or "none"
    return (f"{report['script']}: {report['total']} symbols, {report['full_percent']}% full, "
            f"blockers: {blockers}")


def aggregate(reports):
    """Per-library and per-symbol totals over several doctor reports."""
    libs, symbols = {}, {}
    for rep in reports:
        lib = rep["library"]
        agg = libs.setdefault(lib, {"library": lib, "scripts": 0, "symbols": set(), "blockers": set()})
        agg["scripts"] += 1
        for s in rep["symbols"]:
            agg["symbols"].add(s["symbol"])
            sym = symbols.setdefault(s["symbol"], {**{k: s[k] for k in ("symbol", "status", "code", "category", "conformance", "corpus_rank")},
                                                   "count": 0, "libraries": set()})
            sym["count"] += s["count"]
            sym["libraries"].add(lib)
        agg["blockers"].update(rep["blockers"])
    lib_out = []
    for agg in libs.values():
        full = sum(1 for s in agg["symbols"] if symbols[s]["status"] == "full")
        total = len(agg["symbols"])
        lib_out.append({"library": agg["library"], "scripts": agg["scripts"], "symbols": total,
                        "full_percent": round(100.0 * full / total, 1) if total else 0.0,
                        "blockers": sorted(agg["blockers"])})
    sym_out = []
    for s in symbols.values():
        s = dict(s)
        s["libraries"] = sorted(s["libraries"])
        s["library_count"] = len(s["libraries"])
        sym_out.append(s)
    sym_out.sort(key=lambda s: (s["corpus_rank"] is None, s["corpus_rank"] or 0, s["symbol"]))
    lib_out.sort(key=lambda l: l["library"])
    return {"libraries": lib_out, "symbols": sym_out}


def doctor(paths, as_json=False, strict=False, fail_on=None):
    """Diagnose KSP scripts. Exit 1 with --strict when any unsupported symbol is used."""
    resolve = Resolver(load_rows())
    reports = []
    for raw in paths:
        for label, text in scripts_of(raw):
            rep = script_report(label, scan_script(text, resolve), resolve)
            rep["library"] = library_of(raw)
            reports.append(rep)
    agg = aggregate(reports) if len(reports) > 1 else None
    unsupported = sorted({s["symbol"] for r in reports for s in r["symbols"]
                          if s["code"] in ("KSP-UNSUPPORTED", "KSP-UNKNOWN")})
    if as_json:
        print(json.dumps({"scripts": reports, "aggregate": agg, "unsupported": unsupported}, indent=2))
    else:
        for rep in reports:
            print(f"== {rep['script']}")
            for s in rep["symbols"]:
                rank = s["corpus_rank"] if s["corpus_rank"] is not None else "-"
                print(f"  {s['code']:<18} {s['symbol']:<42} x{s['count']:<3} {s['category']:<12} "
                      f"{s['conformance']:<9} corpus_rank={rank}")
            print("summary:", summary_line(rep))
        if agg:
            print("== by library")
            for lib in agg["libraries"]:
                print(f"  {lib['library']}: {lib['scripts']} scripts, {lib['symbols']} symbols, "
                      f"{lib['full_percent']}% full, blockers: {', '.join(lib['blockers'][:8]) or 'none'}")
            print("== by symbol (corpus rank order)")
            for s in agg["symbols"][:40]:
                print(f"  {s['code']:<18} {s['symbol']:<42} x{s['count']:<4} libraries={s['library_count']}")
    if fail_on:
        if any(s["status"] == fail_on for r in reports for s in r["symbols"]):
            return 1
    if strict and unsupported:
        if not as_json:
            print(f"strict: unsupported symbols used: {', '.join(unsupported)}", file=sys.stderr)
        return 1
    return 0



def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("refresh")
    r.add_argument("--manual", required=True, type=Path, help="KSP_Manual.pdf")
    r.add_argument("--binary", type=Path, help="Kontakt8.exe, for string/xref evidence")
    sub.add_parser("sync")
    sub.add_parser("check")
    d = sub.add_parser("doctor", help="diagnose KSP scripts (.ksp/.txt or .nki instruments)")
    d.add_argument("files", nargs="+")
    d.add_argument("--json", action="store_true", help="machine-readable report, incl. per-library and per-symbol aggregate")
    d.add_argument("--strict", action="store_true", help="exit 1 when any unsupported symbol is used")
    d.add_argument("--fail-on", choices=DOCTOR_STATUSES, help="exit 1 if this status occurs")
    args = ap.parse_args(argv)
    if args.cmd == "refresh":
        existing = load_rows() if SYMBOLS.exists() else []
        rows = refresh(args.manual, args.binary, existing)
        OUT.mkdir(parents=True, exist_ok=True)
        SYMBOLS.write_text(tsv(rows))
        sync_all(write=True)
        print(f"refreshed {len(rows)} rows")
        return 0
    if args.cmd == "sync":
        rows, _, _ = sync_all(write=True)
        print(f"wrote {len(rows)} rows")
        return 0
    if args.cmd == "check":
        return check()
    return doctor(args.files, as_json=args.json, strict=args.strict, fail_on=args.fail_on)


if __name__ == "__main__":
    sys.exit(main())
