#!/usr/bin/env python3
"""Run existing UI/playback audits in isolated processes, retaining timeout evidence."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path, help="Preset files or library folders")
    parser.add_argument("--bin", required=True, type=Path, help="Built kontakto executable")
    parser.add_argument("--mode", choices=("ui", "playback"), default="ui")
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    if args.timeout <= 0 or not args.timeout < float("inf"):
        parser.error("--timeout must be positive and finite")
    files = set()
    for root in args.paths:
        if not root.exists():
            parser.error(f"Path does not exist: {root}")
        files.update(p.resolve() for p in ([root] if root.is_file() else root.rglob("*"))
                     if p.is_file() and p.suffix.lower() in ((".nki", ".nkm") if args.mode == "ui" else (".nki",)))
    if not files:
        parser.error("No supported presets found")
    args.out.mkdir(parents=True, exist_ok=True)
    failures = 0
    with (args.out / "results.jsonl").open("a", encoding="utf-8") as results:
        for index, path in enumerate(sorted(files), 1):
            folder = args.out / hashlib.sha256(str(path).encode()).hexdigest()[:16]
            folder.mkdir(exist_ok=True)
            report = folder / "report.json"
            report.unlink(missing_ok=True)
            env = dict(os.environ, KONTRA_LOG_DIR=str((folder / "logs").resolve()))
            command = [str(args.bin.resolve()), "audit-ui", str(path), "--json", str(report)] if args.mode == "ui" else [str(args.bin.resolve()), "audit-patch", str(path)]
            started = time.monotonic()
            with (folder / "process.log").open("wb") as log:
                try:
                    child = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, env=env, timeout=args.timeout)
                    status = "completed" if child.returncode == 0 else "failed"
                except subprocess.TimeoutExpired:
                    status = "timeout"
            if args.mode == "playback":
                for line in reversed((folder / "process.log").read_text(errors="replace").splitlines()):
                    try:
                        row = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    report.write_text(json.dumps(row, indent=2), encoding="utf-8")
                    break
            last = None
            for journal in sorted((folder / "logs").glob("*.jsonl")):
                for line in journal.read_text(errors="replace").splitlines():
                    try:
                        event = json.loads(line)
                    except json.JSONDecodeError:
                        continue  # A terminated write can leave one incomplete final line.
                    if last is None or event.get("timestamp_ms", 0) >= last.get("timestamp_ms", 0):
                        last = event
            row = {"path": str(path), "mode": args.mode, "status": status, "elapsed_s": round(time.monotonic() - started, 3), "evidence": str(folder.resolve()), "last_event": last}
            results.write(json.dumps(row) + "\n")
            results.flush()
            failures += status != "completed"
            print(f"{index}/{len(files)} {status}: {path}", flush=True)
    raise SystemExit(bool(failures))


if __name__ == "__main__":
    main()
