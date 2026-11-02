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


#: Built by `cargo build --example dump_scripts -p kontra-format-kontakt`.
BIN = os.environ.get("DUMP_SCRIPTS", "target/debug/examples/dump_scripts")

def one(args):
    i, rec, outdir = args
    d = os.path.join(outdir, str(i))
    os.makedirs(d, exist_ok=True)
    try:
        p = subprocess.run([BIN, rec["path"], d], capture_output=True, text=True, timeout=300)
    except subprocess.TimeoutExpired:
        return {"i": i, "path": rec["path"], "ok": False, "err": "timeout"}
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
