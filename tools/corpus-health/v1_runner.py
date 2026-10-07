#!/usr/bin/env python3
"""Run the v1 binary (kontakto, built from main) over a corpus item list and
write corpus-health records, so `corpus_health summary|diff` compares v1 and v2.

    v1_runner.py --bin PATH/kontakto --ids V2.jsonl --out V1.jsonl [--shard I/N]
                 [--timeout 300] [--audit-every K]

Items come from the `id`/`kind` fields of a v2 corpus-health run (the same
list). One process per item, so a crash only costs that item. Resumable: a
`started` line without a result is recorded as a crash on the next run.

Per kind:
  kontakt        `render` (one note, offline); load ok = exit 0 or "Rendered
                 silence"; sounds = peak > 1e-4 (v2's threshold, on the same
                 0.25-scaled output v1 writes, so compare dB loosely).
  kontakt-multi  `playback-audit` program 0; ok = exit 0 and JSON parsed.
  uvi-*          v1 has no UVI reader: recorded as an unsupported load failure.
Peak RSS is the child's ru_maxrss (whole process). v1's `render` does not
report load time on its own, so load_ms is the process wall time (an upper
bound that includes the offline render). Deadline misses come from
`playback-audit --realtime` on every K-th Kontakt item (0 = never).
Rendered audio goes to /dev/shm and is deleted at once; nothing is kept.
"""
import argparse
import json
import math
import os
import re
import resource
import signal
import subprocess
import sys
import tempfile
import time


def norm(text):
    line = (text or "").strip().splitlines()[0] if (text or "").strip() else ""
    out = []
    for word in line.split():
        out.append("<path>" if "/" in word or "\\" in word else re.sub(r"\d+", "N", word))
    return " ".join(out)[:140]


def run(cmd, timeout):
    """(returncode or -signal, stdout, stderr, wall_ms, rss_kib, timed_out)."""
    started = time.monotonic()
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        proc = subprocess.Popen(cmd, stdout=out, stderr=err, start_new_session=True)
        timed_out = False
        deadline = started + timeout
        while True:
            pid, status, usage = os.wait4(proc.pid, os.WNOHANG)
            if pid:
                break
            if time.monotonic() > deadline:
                timed_out = True
                os.killpg(proc.pid, signal.SIGKILL)
                pid, status, usage = os.wait4(proc.pid, 0)
                break
            time.sleep(0.05)
        out.seek(0)
        err.seek(0)
        code = os.waitstatus_to_exitcode(status)
        return (code, out.read().decode("utf-8", "replace"), err.read().decode("utf-8", "replace"),
                int((time.monotonic() - started) * 1000), usage.ru_maxrss, timed_out)


def check(binary, item, timeout, audit):
    path, kind = item["id"], item["kind"]
    record = {"id": path, "kind": kind, "status": "done", "engine": "v1"}
    if kind.startswith("uvi"):
        record["load"] = {"ok": False, "stage": "parse", "error": "v1 has no UVI reader"}
        record["stage"] = "parse"
        record["load_ms"] = 0
        record["perf"] = {"load_ms": 0, "peak_rss_kib": 0, "render": {}}
        return record
    wav = f"/dev/shm/v1gate-{os.getpid()}.wav"
    try:
        if kind == "kontakt-multi":
            cmd = [binary, "playback-audit", path, "0"]
        else:
            cmd = [binary, "render", path, wav]
        code, out, err, wall, rss, timed_out = run(cmd, timeout)
    finally:
        if os.path.exists(wav):
            os.remove(wav)
    record["load_ms"] = wall
    record["perf"] = {"load_ms": wall, "peak_rss_kib": rss, "render": {}}
    message = (err.strip().splitlines() or [""])[-1]
    if timed_out:
        record["status"] = "crash"
        record["load"] = {"ok": False, "error": "timed out"}
        return record
    if code < 0:
        record["status"] = "crash"
        record["load"] = {"ok": False, "error": f"killed by signal {-code}"}
        return record
    if kind == "kontakt-multi":
        ok = code == 0
        record["load"] = {"ok": ok}
        if not ok:
            record["load"].update(stage="load", error=norm(message), raw=message[:300])
    else:
        silent = "Rendered silence" in err
        nonfinite = "Nonfinite" in err
        ok = code == 0 or silent or nonfinite
        record["load"] = {"ok": ok}
        if not ok:
            record["load"].update(stage="load", error=norm(message), raw=message[:300])
        else:
            peak = re.search(r"peak ([0-9.]+)", out)
            peak = float(peak.group(1)) if peak else 0.0
            # v1 scales its WAV by 0.25 before measuring the peak.
            db = 20 * math.log10(peak) if peak > 0 else None
            record["sound"] = {
                "note": "started",
                "peak_db": db,
                "sounds": code == 0 and peak > 1e-4,
                "finite": not nonfinite,
                "script_faults": [],
                "perf": {},
            }
    if audit and kind == "kontakt" and record["load"]["ok"]:
        c2, o2, e2, w2, r2, t2 = run([binary, "playback-audit", path, "0", "--realtime"], timeout * 2)
        try:
            data = json.loads(o2)
            misses = sum(case.get("blocks_exceeding_duration", 0) for case in data.get("cases", []))
            record["perf"]["render"] = {
                "deadline_misses": misses,
                "blocks": sum(case.get("blocks", 0) for case in data.get("cases", [])),
                "source": "playback-audit --realtime",
            }
        except (ValueError, AttributeError):
            record["perf"]["render"] = {"deadline_misses": None, "audit_error": norm(e2)}
    return record


def stage(r):
    if r["load"]["ok"] is not True:
        return r["load"].get("stage", "load")
    if r["kind"] == "kontakt-multi":
        return "ok"
    s = r.get("sound", {})
    if s.get("sounds") is not True:
        return "note-on/selection"
    if s.get("finite") is False:
        return "render"
    return "ok"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--ids", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--shard", default="0/1")
    ap.add_argument("--timeout", type=int, default=300)
    ap.add_argument("--audit-every", type=int, default=0)
    args = ap.parse_args()
    shard, shards = map(int, args.shard.split("/"))
    items, seen = [], set()
    with open(args.ids) as f:
        for line in f:
            r = json.loads(line)
            if r.get("status") == "started" or r["id"] in seen or "kind" not in r:
                continue
            seen.add(r["id"])
            items.append({"id": r["id"], "kind": r["kind"]})
    items.sort(key=lambda i: i["id"])
    items = [i for n, i in enumerate(items) if n % shards == shard]
    done = set()
    if os.path.exists(args.out):
        with open(args.out) as f:
            lines = [json.loads(l) for l in f if l.strip()]
        done = {l["id"] for l in lines if l.get("status") != "started"}
        started = {l["id"] for l in lines if l.get("status") == "started"} - done
        with open(args.out, "a") as f:
            for id in sorted(started):
                crash = {"id": id, "kind": "unknown", "status": "crash",
                         "load": {"ok": False, "error": "process died (abort, OOM or hang)"}}
                f.write(json.dumps(crash) + "\n")
                done.add(id)
    with open(args.out, "a") as out:
        for n, item in enumerate(items):
            if item["id"] in done:
                continue
            out.write(json.dumps({"id": item["id"], "status": "started"}) + "\n")
            out.flush()
            audit = args.audit_every and n % args.audit_every == 0
            record = check(args.bin, item, args.timeout, audit)
            if "stage" not in record:
                record["stage"] = stage(record)
            out.write(json.dumps(record) + "\n")
            out.flush()
            print(f"[{n + 1}/{len(items)}] {record['status']} {record['stage'] if 'stage' in record else ''} {item['id']}",
                  file=sys.stderr)


if __name__ == "__main__":
    main()
