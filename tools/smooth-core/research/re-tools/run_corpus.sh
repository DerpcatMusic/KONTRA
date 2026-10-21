#!/usr/bin/env bash
# Build the soothe3 pseudocode corpus end to end. Resumable: re-run after a stop.
#   run_corpus.sh [BINARY]            decompile (batches), then index
#   run_corpus.sh index [BINARY]      rebuild only the sqlite index + annotated files
# Outputs live on the dev drive, never in the repo:
#   $RUN_DIR/corpus/   manifest.tsv, fn/<rva>.c, batches/*.done
#   $RUN_DIR/index/    corpus.sqlite, annotated/<rva>.c
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
RUN_DIR="${SMOOTH_CORPUS_RUN_DIR:-/data/dev/workspace/RunScratch/smooth/re-corpus}"
SWEEP="${SMOOTH_SWEEP_DB:-/data/dev/workspace/graft-scratch/sweep-soothe3/index-default.sqlite}"
WORKERS="${SMOOTH_CORPUS_WORKERS:-8}"

MODE=build
if [[ "${1:-}" == "index" ]]; then MODE=index; shift; fi
BINARY="${1:-/tmp/opencode/rea/soothe3-restored.bin}"

mkdir -p "$RUN_DIR"
if [[ "$MODE" == build ]]; then
  python3 "$HERE/corpus_build.py" --binary "$BINARY" --out "$RUN_DIR/corpus" \
    --workers "$WORKERS" --batch 120 --sections .text,IPPCODE \
    | tee -a "$RUN_DIR/build.log"
fi
python3 "$HERE/corpus_index.py" --binary "$BINARY" --corpus "$RUN_DIR/corpus" \
  --sweep "$SWEEP" --out "$RUN_DIR/index"
echo "index: $RUN_DIR/index/corpus.sqlite"
