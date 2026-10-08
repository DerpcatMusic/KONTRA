#!/usr/bin/env python3
"""Resume metadata-only Kontakt usage census in <=240-second heavy shards."""
import argparse
import collections
import hashlib
import json
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("cache", type=Path)
    parser.add_argument("--items", type=Path, default=Path.home() / ".cache/kontakto-corpus/items.tsv")
    parser.add_argument("--shard", action="store_true")
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    items = [(family, path) for family, path in (line.split("\t", 1) for line in args.items.read_text().splitlines()) if family.startswith("kontakt")]
    records = [args.cache / (hashlib.sha256(path.encode()).hexdigest()[:20] + ".json") for _, path in items]
    def current(record, path):
        if not record.exists():
            return False
        row, info = json.loads(record.read_text()), Path(path).stat()
        return (row.get("size_bytes"), row.get("mtime_ns")) == (info.st_size, info.st_mtime_ns)

    if not args.shard:
        while not all(current(record, path) for (_, path), record in zip(items, records)):
            result = subprocess.run([str(Path.home() / ".cache/kontakto-heavy"), "python3", __file__, str(args.binary), str(args.cache), "--items", str(args.items), "--shard"])
            if result.returncode:
                raise SystemExit(result.returncode)
            # The wrapper has no FIFO: give already queued jobs a polling turn.
            time.sleep(5)
        rows = [json.loads(p.read_text()) for p in records]
        counts = collections.Counter(token for r in rows if r["status"] == "ok" for token in r["tokens"])
        by_family = {family: dict(sorted(collections.Counter(token for r in rows if r["family"] == family and r["status"] == "ok" for token in r["tokens"]).items())) for family in sorted({f for f, _ in items})}
        summary = {"items": len(items), "statuses": dict(collections.Counter(r["status"] for r in rows)), "families": dict(collections.Counter(f for f, _ in items)), "usage_items": dict(sorted(counts.items())), "usage_by_family": by_family, "scope": "lexical source identifier presence, excluding comments/strings; NKM unions all programs; no initialization/pass claim"}
        (args.cache / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps(summary, indent=2), flush=True)
        return
    start = time.monotonic()
    for (family, path), record in zip(items, records):
        if current(record, path):
            continue
        if time.monotonic() - start > 215:
            return
        began = time.monotonic()
        try:
            run = subprocess.run([str(args.binary), "usage", path], capture_output=True, timeout=20)
            status = "ok" if run.returncode == 0 else "reader_error"
            tokens = [line.split("\t")[1] for line in run.stdout.decode().splitlines() if line.startswith("use\t")] if status == "ok" else []
            error_hash = hashlib.sha256(run.stderr).hexdigest() if run.stderr else None
        except subprocess.TimeoutExpired:
            status, tokens, error_hash = "timeout", [], None
        info = Path(path).stat()
        record.write_text(json.dumps({"family": family, "path": path, "size_bytes": info.st_size, "mtime_ns": info.st_mtime_ns, "status": status, "tokens": tokens, "stderr_sha256": error_hash, "seconds": time.monotonic() - began}) + "\n")
    print("shard complete", flush=True)


if __name__ == "__main__":
    main()
