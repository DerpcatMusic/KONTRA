#!/bin/sh
# Run every generated batch on the real Kontakt 8 oracle, then decode golden.json.
#   tools/ksp-conformance/run.sh [OUTDIR]
# Each batch is one dump.py session under the oracle lock (one Kontakt at a time). A session
# that finds another Kontakt running is retried, so this can wait behind other agents.
set -e
cd "$(dirname "$0")/../.."
OUT=${1:-$HOME/.cache/kontra-ksp-conformance}
mkdir -p "$OUT"
rm -f "$OUT"/batch_*.jsonl*
for b in tools/ksp-conformance/generated/batch_*.lua; do
  n=$(basename "$b" .lua)
  for try in $(seq 1 80); do
    timeout 900 flock /tmp/kontra-oracle.lock env ORACLE_LUA="$PWD/$b" nice -n 10 \
      python3 tools/kontakt-oracle/dump.py "$OUT/$n.jsonl" > "$OUT/$n.log" 2>&1 || true
    grep -q "another Kontakt is running" "$OUT/$n.log" || break
    sleep 20
  done
done
python3 tools/ksp-conformance/gen.py decode "$OUT"/batch_*.jsonl
