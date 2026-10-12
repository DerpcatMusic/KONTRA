#!/bin/bash
# Build the corpus binary once (heavy slot) and run it directly: run.sh <corpus-health args...>
# e.g. run.sh quick | run.sh run out.jsonl --tier full | run.sh diff old.jsonl new.jsonl
set -eu
cd "$(dirname "$0")/../.."
~/.cache/kontakto-heavy cargo build --profile corpus -p corpus-health
exec "${CARGO_TARGET_ROOT:-$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')}/corpus/corpus-health" "$@"
