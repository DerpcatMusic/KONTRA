#!/usr/bin/env bash
# Dumps the full IR (Debug) plus every law application for each .nki under
# ROOTs, one file per instrument, so a translator refactor can prove the IR
# did not change: run it before and after, then `diff -r BEFORE AFTER`.
# Usage: tools/migrations/kontakt-ir-dump.sh INSPECT_BIN OUT_DIR ROOT...
#   INSPECT_BIN: a copy of `cargo build --release -p kontra-format-kontakt --example inspect`
set -euo pipefail
bin="${1:?usage: kontakt-ir-dump.sh INSPECT_BIN OUT_DIR ROOT...}"
out="${2:?usage}"
shift 2
mkdir -p "$out"
find "$@" -name '*.nki' -print0 | sort -z | while IFS= read -r -d '' f; do
  name="$(printf '%s' "$f" | sed 's#[/ ]#_#g')"
  # Resumable: a finished dump is renamed into place, so it is skipped.
  [ -e "$out/$name.txt" ] && continue
  KONTRA_DUMP=1 "$bin" "$f" >"$out/$name.part" 2>&1 || echo "exit $?" >>"$out/$name.part"
  mv "$out/$name.part" "$out/$name.txt"
done
echo "$(find "$out" -name '*.txt' | wc -l) dumps in $out"
