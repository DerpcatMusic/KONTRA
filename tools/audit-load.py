#!/usr/bin/env python3
"""Run one short plugin load shard; persist numeric metadata only.

Usage: audit-load.py MANIFEST OUT_DIR BIN MODE START END [REPEAT]
MODE: v2, v1user (read existing cache), v1fresh (no preset/header cache).
Run through kontakto-heavy. Requires Linux /proc and bwrap.
"""
import json
import os
from pathlib import Path
import subprocess
import hashlib
import sys
import time


def sanitized(probe):
    probe = dict(probe)
    trace = probe.pop("trace", None)
    probe.pop("status", None)
    if trace:
        for prefix, report in [("load", trace), ("artwork", trace.get("artwork")), ("preload", trace.get("preload"))]:
            if not report:
                continue
            probe[prefix + "_trace"] = {
                "elapsed_ms": report.get("elapsed_ms"),
                "stages_ms": {k: v for k, v in report.get("stages_ms", {}).items() if isinstance(v, (int, float))},
                "status": report.get("status"),
                "counts": {k: v for k, v in report.get("details", {}).items() if k in {
                    "groups", "zones_total", "zones_playable", "zones_skipped", "samples_loaded",
                    "samples_streamed", "resident_bytes", "memory_budget_bytes", "missing_samples",
                    "script_slots", "pictures_loaded", "controls", "sample_rate"
                } and isinstance(v, (int, float, bool))},
            }
    return probe


def main():
    manifest, out, binary, mode, start, end, *repeat = sys.argv[1:]
    assert mode in ("v2", "v1user", "v1fresh")
    repeat = repeat[0] if repeat else "1"
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    deadline = time.monotonic() + 235
    for line in Path(manifest).read_text().splitlines()[int(start):int(end)]:
        name, path, program = line.split("\t")
        output = out / f"{mode}-{name}-{repeat}.json"
        if output.exists():
            continue
        # Resume the same slice after releasing the heavy slot. Never start a
        # worker unless its entire timeout fits in this short shard.
        if deadline - time.monotonic() < 130:
            return 75
        env = os.environ.copy()
        env.update(PROBE_PATH=path, PROBE_PROGRAM=program, TMPDIR="/proc/self/no-audit-tmp",XDG_DATA_HOME="/tmp/audit-data", KONTRA_LOG_DIR="/proc/self/no-audit-log", KONTRA_DISABLE_NETWORK="1")
        env["XDG_CACHE_HOME"] = "/proc/self/no-audit-cache" if mode == "v1fresh" else str(Path.home() / ".cache")
        env["KONTRA_AUDIT_LOAD"] = "1"
        # Only the v2-owned numeric metadata namespace may be writable.
        cache_home = env.get("PROBE_CACHE_HOME") if mode == "v2" else None
        if cache_home:
            cache_home = Path(cache_home).resolve()
            numeric = cache_home / "kontra" / "v2-headers"
            numeric.mkdir(parents=True, exist_ok=True)
            env["XDG_CACHE_HOME"] = str(cache_home)
        product_root = os.environ.get('PROBE_PRODUCT_CACHE_ROOT')
        product_cache = None
        if product_root:
            root = Path(product_root).resolve()
            if root.parent != Path('/dev/shm') or not root.name.startswith('kontra-gate-cache-'):
                raise ValueError('probe cache must be private tmpfs')
            product_cache = root / 'v1-probes' / hashlib.sha256(path.encode()).hexdigest()
            product_cache.mkdir(parents=True, exist_ok=True)
            env['XDG_CACHE_HOME'] = str(product_cache)
        # Everything else stays read-only; TMPDIR and LOG_DIR cannot be created.
        # Capture output in memory and persist only the sanitized probe metadata.
        cmd = ["bwrap", "--die-with-parent", "--ro-bind", "/", "/", "--proc", "/proc", "--dev-bind", "/dev", "/dev",
               "--tmpfs", "/tmp", str(Path(binary).resolve()), "--ignored", "--exact", env.get("PROBE_TEST", "plugin::tests::probe_load"), "--nocapture"]
        if product_cache:
            cmd[5:5] = ['--bind', str(product_cache), str(product_cache)]
        if cache_home:
            cmd[5:5] = ["--bind", str(numeric), str(numeric)]
        stages, result = [], {}
        begin = time.monotonic()
        try:
            run = subprocess.run(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=130)
            for line in run.stdout.splitlines():
                if line.startswith("AUDIT "):
                    stages.append(json.loads(line[6:]))
                if line.startswith("PROBE "):
                    result = sanitized(json.loads(line[6:]))
            result["returncode"] = run.returncode
        except subprocess.TimeoutExpired:
            result = {"timeout_s": 130}
        result.update(library=name, mode=mode, repeat=repeat, stages=stages, process_wall_s=time.monotonic() - begin)
        output.write_text(json.dumps(result, indent=2) + "\n")
        print(name, mode, result.get("load_run_ms", result.get("returncode", "timeout")), flush=True)


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-check"]:
        result = sanitized({"trace": {"details": {"resident_bytes": 12, "secret": 9}, "issues": ["source"], "stages_ms": {"parse": 2}}, "peak": 1})
        assert result["load_trace"]["counts"] == {"resident_bytes": 12}
        assert "source" not in json.dumps(result) and "secret" not in json.dumps(result)
        print("ok")
    else:
        sys.exit(main() or 0)
