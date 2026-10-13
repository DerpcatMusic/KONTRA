#!/usr/bin/env python3
"""Survey the installed Kontakt libraries once and cache summaries for build.py.

    python3 tools/coverage/survey.py   # builds ir_json + prepare_check (one small crate build), runs both

Per library (one instrument each, taken from the native-oracle records):
prepare_check result, native-oracle diff (tools/kontakt-oracle/diff.py logic) and the
unsupported features ranked by count. Only counts and feature names are cached: no
script source, no sample data, no decrypted payloads.
Output: tools/coverage/cache/libraries.json
"""
import argparse, glob, importlib.util, json, os, re, subprocess, sys, time
from collections import Counter

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ORACLE = os.path.expanduser("~/.cache/kontra-native-oracle/out/*.jsonl")
HEAVY = os.path.expanduser("~/.cache/kontakto-heavy")
WIN_ROOT = ("D:\\Libraries\\", "/mnt/MAIN_STORAGE/Libraries/")


def unix(path):
    p = path.replace("\\", "/")
    w = WIN_ROOT[0].replace("\\", "/")
    return WIN_ROOT[1] + p[len(w):] if p.startswith(w) else p


def library_of(path):
    parts = unix(path).split("/")
    return parts[parts.index("Kontakt") + 1] if "Kontakt" in parts else parts[-1]


def build_examples():
    cmd = [HEAVY, "cargo", "build", "-q", "-p", "sampler-kontakt", "--example", "ir_json",
           "--example", "prepare_check", "--message-format=json"]
    out = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    if out.returncode:
        sys.exit(out.stderr[-2000:])
    exes = {}
    for line in out.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("executable") and m.get("target", {}).get("kind") == ["example"]:
            exes[m["target"]["name"]] = m["executable"]
    return exes


def norm(feature):
    # sampler-ir's normalisation: numbers and paths removed so equal causes group.
    return re.sub(r"\d+", "N", re.sub(r"\(.*?\)", "", feature)).strip()


def main():
    exes = build_examples()  # no-op when cargo already built them
    spec = importlib.util.spec_from_file_location("diff", os.path.join(ROOT, "tools/kontakt-oracle/diff.py"))
    diff = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(diff)

    records = {}
    for f in sorted(glob.glob(ORACLE)):
        for line in open(f):
            r = json.loads(line) if line.strip() else {}
            if "input" in r and "instrument" in r:
                records[r["input"]] = r
            elif "input" in r:
                records.setdefault(r["input"], r)
    libraries = []
    for inp, native in sorted(records.items(), key=lambda kv: library_of(kv[0]).lower()):
        path = unix(inp)
        entry = {"library": library_of(inp), "instrument": os.path.basename(path), "oracle": None,
                 "prepare": None, "unsupported": []}
        t = time.time()
        p = subprocess.run([exes["prepare_check"], path], capture_output=True, text=True, timeout=1800)
        entry["prepare"] = {"text": (p.stdout.strip() or p.stderr.strip())[:200],
                            "ok": p.stdout.startswith("ok"), "secs": round(time.time() - t, 1)}
        ours = subprocess.run([exes["ir_json"], path], capture_output=True, text=True, timeout=1800)
        try:
            ours = json.loads(ours.stdout)
        except ValueError:
            ours = {"load_error": "ir_json produced no JSON"}
        if "instrument" in ours:
            c = Counter()
            reasons = {}
            for u in ours["instrument"].get("unsupported", []):
                key = norm(u["feature"])
                c[key] += 1
                reasons[key] = u.get("reason", "")
            entry["unsupported"] = [[k, reasons[k], n] for k, n in c.most_common()]
            if "instrument" in native:
                d = diff.diff_record(native, ours)
                entry["oracle"] = {"checked": sum(d.checked.values()), "mismatch": sum(d.count.values()),
                                   "fields": [[f, d.count[f], d.checked[f]] for f in d.checked],
                                   "examples": {f: [list(map(str, e)) for e in ex[:2]] for f, ex in d.examples.items() if d.count[f]}}
            elif native.get("load_error"):
                entry["oracle"] = {"error": "native Kontakt could not load it: " + str(native["load_error"])[:120]}
        else:
            entry["unsupported"] = [["load error: " + str(ours.get("load_error"))[:100], "", 1]]
        libraries.append(entry)
        print(entry["library"], entry["prepare"]["text"][:80], file=sys.stderr)
    out = os.path.join(ROOT, "tools/coverage/cache/libraries.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    json.dump({"surveyed": time.strftime("%Y-%m-%d %H:%M"), "libraries": libraries}, open(out, "w"), indent=1)
    print("wrote", out)


if __name__ == "__main__":
    main()
