#!/usr/bin/env bash
# Builds kontra-inspect (release, no LTO) and runs the parity census into
# $1 (default /data/dev/workspace/PluginArtifacts/kontra-parity/<git short sha>).
# Re-run before and after a fix; compare the two census-*.tsv tables.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-/data/dev/workspace/PluginArtifacts/kontra-parity/$(git -C "$here" rev-parse --short HEAD)}
mkdir -p "$out"
bin=$(cd "$here/../.." && BUILD_SLOT_NO_BLAZE=1 CARGO_PROFILE_RELEASE_LTO=false \
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_INCREMENTAL=true \
  build-slot cargo build --release -p kontra-inspect --message-format=json 2>/dev/null |
  python3 -c 'import sys,json
for l in sys.stdin:
    m=json.loads(l)
    if m.get("reason")=="compiler-artifact" and (m.get("executable") or "").endswith("/kontra-inspect"): print(m["executable"])')
cp "$bin" "$out/kontra-inspect"
KONTRA_INSPECT_BIN="$out/kontra-inspect" KONTRA_CENSUS_OUT="$out/play.jsonl" \
  KONTRA_CENSUS_JOBS=${KONTRA_CENSUS_JOBS:-5} python3 "$here/census.py" "${@:2}"
