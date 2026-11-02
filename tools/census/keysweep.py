#!/usr/bin/env python3
"""Re-render every instrument the census found silent at key 60, across a spread
of keys, to separate "our harness picked a key this instrument does not use"
from "this instrument does not sound".

Writes one JSONL record per instrument to $CENSUS_WORK/keysweep-results.jsonl.
Deterministic; safe to re-run.
"""
import json
import glob
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


RENDER = os.environ.get("KONTRA_RENDER", "target/release/kontra-render")
#: The census shards, as `census_sweep.py` wrote them.
SHARDS = os.path.join(WORK, "partA", "shard-*.jsonl")
OUT = os.path.join(WORK, "keysweep-results.jsonl")
KEYS = [36, 43, 48, 55, 60, 64, 67, 72, 84]
SILENT = 1e-4


def silent_at_60():
    seen, out = set(), []
    for f in sorted(glob.glob(SHARDS)):
        for line in open(f):
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            p = r.get("path")
            if not p or p in seen or not r.get("loaded"):
                continue
            seen.add(p)
            if (r.get("peak") or 0.0) < SILENT:
                out.append(r)
    return out


def render(path, key):
    wav = os.path.join(WORK, f"w-{os.getpid()}-{abs(hash(path)) % 10**9}-{key}.wav")
    try:
        p = subprocess.run(
            [RENDER, "--instrument", path, "--key", str(key), "--velocity", "100",
             "--hold", "1.0", "--tail", "0.5", "--out", wav, "--quiet"],
            capture_output=True, text=True, timeout=180)
        if p.returncode != 0:
            return None, (p.stderr or p.stdout)[:200]
        return float(json.loads(p.stdout)["peak"]), None
    except (subprocess.TimeoutExpired, json.JSONDecodeError, KeyError, ValueError) as e:
        return None, str(e)[:200]
    finally:
        try:
            os.unlink(wav)
        except OSError:
            pass


def one(r):
    peaks, err = {}, None
    for k in KEYS:
        pk, e = render(r["path"], k)
        if e:
            err = e
        if pk is not None:
            peaks[k] = round(pk, 6)
    sounds = [k for k, v in peaks.items() if v >= SILENT]
    rec = {
        "path": r["path"],
        "name": r.get("name"),
        "zones_ready": r.get("zones_ready"),
        "scripts": r.get("scripts"),
        "peaks": peaks,
        "sounds_at": sounds,
        "best_key": max(peaks, key=peaks.get) if peaks else None,
        "best_peak": max(peaks.values()) if peaks else 0.0,
        "error": err,
    }
    with open(OUT, "a") as f:
        f.write(json.dumps(rec) + "\n")
    return rec


def main():
    todo = silent_at_60()
    print(f"silent at key 60: {len(todo)}", flush=True)
    if os.path.exists(OUT):
        done = {json.loads(l)["path"] for l in open(OUT) if l.strip()}
        todo = [r for r in todo if r["path"] not in done]
        print(f"already done: {len(done)}; remaining: {len(todo)}", flush=True)
    n = 0
    with ThreadPoolExecutor(max_workers=int(sys.argv[1]) if len(sys.argv) > 1 else 4) as ex:
        for rec in ex.map(one, todo):
            n += 1
            if n % 25 == 0:
                print(f"  {n}/{len(todo)}", flush=True)
    print(f"done: {n}", flush=True)


if __name__ == "__main__":
    main()
