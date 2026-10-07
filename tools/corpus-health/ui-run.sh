#!/usr/bin/env bash
# Run an already built UI census in resumable four-minute heavy-slot shards.
set -euo pipefail
if (( $# < 3 )); then
  echo 'Usage: ui-run.sh BINARY OUT.jsonl CACHE_DIR [ui_health options...]' >&2
  exit 2
fi
binary=$1
out=$2
cache=$3
shift 3
while :; do
  status=0
  /home/derpcat/.cache/kontakto-heavy "$binary" "$out" --cache "$cache" --budget-seconds 240 "$@" || status=$?
  case $status in
    0) exit 0 ;;
    75) sleep 5 ;; # Release the slot between shards so queued agents can build.
    *) exit "$status" ;;
  esac
done
