#!/usr/bin/env python3
"""Extract KSP scripts for every census-loaded instrument into <outdir>/scripts/<n>/.

Usage: extract.py <census.json> <outdir> [workers]
Writes <outdir>/manifest.jsonl: {"i":n,"path":...,"ok":bool,"slots":k,"bytes":[],"err":...}
"""
import json, os, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
import os

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


#: The dumper the 2026-11-01 pass used. Its example lived in
#: `crates/sampler-kontakt`, which the 2026-11-02 sync removed, so this path names
#: a binary this branch cannot build; point `DUMP_SCRIPTS` at a replacement.
BIN = os.environ.get("DUMP_SCRIPTS", "target/debug/examples/dump_scripts")

#: Seconds one extraction may take. A constant, not a literal, so a test can
#: prove the timeout record has the same shape as the others.
TIMEOUT = 300

def one(args):
    i, rec, outdir = args
    d = os.path.join(outdir, str(i))
    os.makedirs(d, exist_ok=True)
    try:
        p = subprocess.run([BIN, rec["path"], d], capture_output=True, text=True, timeout=TIMEOUT)
    except subprocess.TimeoutExpired:
        # Same fields as every other record: the module docstring lists `name`,
        # `slots` and `bytes` as manifest fields, and `instruments.py` reads
        # `r["name"]` unguarded -- one timeout used to make it raise KeyError.
        return {"i": i, "path": rec["path"], "name": rec.get("name"), "ok": False,
                "slots": 0, "bytes": [], "err": "timeout"}
    files = sorted(os.listdir(d)) if os.path.isdir(d) else []
    err = "" if p.returncode == 0 else (p.stderr.strip().splitlines() or ["exit %d" % p.returncode])[-1][:300]
    return {"i": i, "path": rec["path"], "name": rec.get("name"), "ok": p.returncode == 0,
            "slots": len(files), "bytes": [os.path.getsize(os.path.join(d, f)) for f in files], "err": err}

def main():
    recs = [r for r in json.load(open(sys.argv[1])) if r.get("loaded")]
    outdir = sys.argv[2]
    workers = int(sys.argv[3]) if len(sys.argv) > 3 else 8
    os.makedirs(outdir, exist_ok=True)
    jobs = [(i, r, os.path.join(outdir, "scripts")) for i, r in enumerate(recs)]
    with open(os.path.join(outdir, "manifest.jsonl"), "w") as f, ThreadPoolExecutor(workers) as ex:
        done = 0
        for res in ex.map(one, jobs):
            f.write(json.dumps(res) + "\n"); f.flush()
            done += 1
            if done % 250 == 0:
                print(f"{done}/{len(jobs)}", file=sys.stderr, flush=True)
    print("done", len(jobs), file=sys.stderr)

if __name__ == "__main__":
    main()
