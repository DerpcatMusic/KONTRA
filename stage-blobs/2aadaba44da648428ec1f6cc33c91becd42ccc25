#!/usr/bin/env python3
"""Assert repeated synthetic NKX lookups do not repeat filesystem path walks."""
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path


def main(binary):
    with tempfile.TemporaryDirectory(prefix="kontra-path-check-", dir="/dev/shm") as scratch:
        trace = Path(scratch) / "syscalls"
        run = subprocess.run([
            "strace", "-f", "-e", "trace=statx,newfstatat,readlink,readlinkat", "-o", str(trace),
            binary, "--exact", "repeated_archive_resolution_reuses_path_checks", "--nocapture",
        ], capture_output=True, text=True)
        if run.returncode:
            raise RuntimeError(f"synthetic lookup fixture/strace failed ({run.returncode}): {run.stderr}")
        records = [line for line in trace.read_text().splitlines()
                   if re.search(r'"/tmp/kontra-archive-path-cache-\d+/authored\.nkx"', line)]
        counts = {kind: sum(kind + "(" in line for line in records)
                  for kind in ("statx", "newfstatat", "readlink", "readlinkat")}
    print(json.dumps({"lookups": 64, "archive_path_syscalls": counts}), flush=True)
    assert sum(counts.values()) <= 4, f"archive checks must be load-scoped, not per member: {counts}"
    assert records, "strace must observe the synthetic archive's actual filesystem calls"


if __name__ == "__main__":
    main(sys.argv[1])
