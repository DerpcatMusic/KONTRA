#!/usr/bin/env bash
# render (KONTRA + v1), identify and score the given ids; prints the score lines
here=$(cd "$(dirname "$0")" && pwd)
for ID in "$@"; do
  P=/home/derpcat/.cache/kontra-reference/probe/$ID
  "$here/probe_render.sh" "$ID" 2>&1 | grep failed
  "$here/probe_identify.sh" "$ID" >/dev/null 2>&1
  python3 "$here/probe_report.py" "$ID" "$P/id.json" "$P/report.json"
done
