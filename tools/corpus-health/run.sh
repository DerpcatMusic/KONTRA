#!/bin/bash
# Build the corpus binary once via build-slot and run it directly: run.sh <corpus-health args...>
# e.g. run.sh quick | run.sh run out.jsonl --tier full | run.sh diff old.jsonl new.jsonl
# The executable path comes from cargo's JSON output, so build-slot's per-worktree target dir is honoured.
set -euo pipefail
cd "$(dirname "$0")/../.."
exe=$(build-slot cargo build --profile corpus -p corpus-health --bin corpus-health --message-format=json |
  python3 -c 'import json,sys
exe = None
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    if msg.get("reason") == "compiler-artifact" and msg.get("executable"):
        exe = msg["executable"]
if exe is None:
    sys.exit("run.sh: cargo reported no corpus-health executable")
print(exe)')
exec "$exe" "$@"
