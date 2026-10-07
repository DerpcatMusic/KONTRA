#!/bin/bash
# Resumable sharded corpus run: tools/corpus-health/run.sh OUTDIR [SHARDS=24]
# Shards with a .done file are skipped; claims left by a killed run are released.
# Each shard runs through the heavy-job wrapper (one slot, so timing is quieter).
set -u
D=${1:?output dir}; N=${2:-24}
cd "$(dirname "$0")/../.."
mkdir -p "$D"
for i in $(seq 0 $((N - 1))); do
  [ -e "$D/shard-$i.done" ] && continue
  rmdir "$D/shard-$i.claim" 2>/dev/null
  mkdir "$D/shard-$i.claim" 2>/dev/null || continue
  KONTAKTO_HEAVY_SLOTS=${KONTAKTO_HEAVY_SLOTS:-1} ~/.cache/kontakto-heavy timeout 7200 \
    cargo run --offline --release -p corpus-health -- run "$D/shard-$i.jsonl" --shard "$i/$N" >/dev/null 2>&1 \
    && touch "$D/shard-$i.done"
done
