#!/usr/bin/env python3
"""Part A census: kontra-inspect load + kontra-render --key 60 over every .nki.

Read-only w.r.t. the repo. Writes one `<OUTDIR>/shard-<label>.jsonl` per shard.

  sweep.py FILELIST SHARD OUTDIR [--workers N] [--render-keys 60]
"""
import json
import os
import re
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


BIN_INSPECT = os.environ.get("KONTRA_INSPECT", "target/release/kontra-inspect")
BIN_RENDER = os.environ.get("KONTRA_RENDER", "target/release/kontra-render")
OUT = Path(sys.argv[3])
OUT.mkdir(parents=True, exist_ok=True)
FILES = [l.rstrip("\n") for l in open(sys.argv[1]) if l.strip()]
SHARD = sys.argv[2]
WORKERS = int(os.environ.get("WORKERS", "6"))
KEYS = [int(k) for k in os.environ.get("KEYS", "60").split(",")]

lock = threading.Lock()
done = 0
t0 = time.time()

RE_SAMPLES = re.compile(r"samples: (\d+); groups: (\d+); zones: (\d+) translated / (\d+) prepared; scripts: (\d+); UI models: (\d+)")
RE_UNPREP = re.compile(r"unprepared zones by reason: (\{.*?\})\s*$", re.M)
RE_DIAG = re.compile(r"diagnostics: (\d+); init unanswered: (\[.*?\]); script faults: (\[.*)", re.S)
RE_UI = re.compile(r"referenced UI assets: (\d+) resolved / (\d+) not resolved")


def faults_total(faults_text):
    """Sum every numeric fault counter across the per-slot Faults records."""
    return sum(int(n) for n in re.findall(r"(?:divide_by_zero|out_of_range|stack_overflow|texts_cut|loop_limits|no_coroutine|bad_code): (\d+)", faults_text))


def one(path):
    global done
    wav = OUT / f"render-{SHARD}.wav"
    rec = {"path": path, "name": Path(path).stem}
    s = time.time()
    try:
        p = subprocess.run([BIN_INSPECT, "load", path], capture_output=True, text=True, timeout=600)
        rec["inspect_ms"] = int((time.time() - s) * 1000)
        rec["inspect_exit"] = p.returncode
        out, err = p.stdout, p.stderr
        if p.returncode != 0:
            rec["loaded"] = False
            # last non-empty stderr line, path stripped where it repeats
            lines = [l.strip() for l in err.strip().splitlines() if l.strip()]
            msg = lines[-1] if lines else ""
            rec["refusal"] = msg.replace(path, "<file>")
            rec["refusal_head"] = msg.split(": ", 2)[-1][:200] if msg else ""
        else:
            rec["loaded"] = True
            rec["stdout"] = out[:8000]
            m = RE_SAMPLES.search(out)
            if m:
                rec["samples"], rec["groups"], rec["zones_total"], rec["zones_ready"], rec["scripts"], rec["ui_models"] = map(int, m.groups())
            m = RE_UNPREP.search(out)
            if m:
                try:
                    rec["unprepared"] = json.loads(m.group(1))
                except Exception:
                    rec["unprepared_raw"] = m.group(1)[:800]
            m = RE_DIAG.search(out)
            if m:
                rec["diagnostics"] = int(m.group(1))
                try:
                    rec["init_unanswered"] = len(json.loads(m.group(2)))
                except Exception:
                    body = m.group(2)[:2000]
                    rec["init_unanswered"] = len(re.findall(r"\(\w+, \d+\)", body))
                    if body.strip() not in ("[]", ""):
                        rec["init_unanswered_raw"] = body
                rec["script_faults"] = faults_total(m.group(3))
                rec["script_faults_raw"] = m.group(3)[:800]
            m = RE_UI.search(out)
            if m:
                rec["ui_resolved"], rec["ui_unresolved"] = map(int, m.groups())
    except subprocess.TimeoutExpired:
        rec["loaded"] = None
        rec["refusal"] = "kontra-inspect timeout 600s"
        rec["inspect_ms"] = int((time.time() - s) * 1000)

    # render (single note, default velocity 100, hold 1.5 s)
    try:
        r = subprocess.run([BIN_RENDER, "--instrument", path, "--key", str(KEYS[0]),
                            "--out", str(wav), "--quiet"],
                           capture_output=True, text=True, timeout=600)
        rec["render_exit"] = r.returncode
        if r.returncode == 0:
            j = json.loads(r.stdout)
            rec["peak"] = j.get("peak")
            rec["peak_dbfs"] = j.get("peak_dbfs")
            rec["rms_dbfs"] = j.get("rms_dbfs")
            rec["frames"] = j.get("frames")
            rec["render_load_s"] = j.get("load_s")
            rec["render_zones"] = j.get("zones")
            rec["sha256"] = j.get("sha256")
            rec["note_key"] = j["note"]["key"]
        else:
            lines = [l.strip() for l in r.stderr.strip().splitlines() if l.strip()]
            rec["render_refusal"] = (lines[-1] if lines else "").replace(path, "<file>")
    except subprocess.TimeoutExpired:
        rec["render_exit"] = None
        rec["render_refusal"] = "kontra-render timeout 600s"
    try:
        wav.unlink()
    except FileNotFoundError:
        pass

    with lock:
        done += 1
        if done % 50 == 0:
            el = time.time() - t0
            print(f"[{SHARD}] {done}/{len(FILES)} {el:.0f}s "
                  f"({el / done:.2f}s/file, eta {(len(FILES) - done) * el / done / 60:.0f}min)",
                  file=sys.stderr, flush=True)
    with open(OUT / f"shard-{SHARD}.jsonl", "a") as fh:
        fh.write(json.dumps(rec) + "\n")


with ThreadPoolExecutor(max_workers=WORKERS) as ex:
    list(ex.map(one, FILES))
print(f"[{SHARD}] finished {len(FILES)} in {time.time() - t0:.0f}s", file=sys.stderr)
