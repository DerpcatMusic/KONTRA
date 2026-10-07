#!/usr/bin/env bash
# Stereo Modeller sweep on the stereo noise instrument (group insert slot 1 open, scrolled as in KONTAKT_REFERENCE.md section 18).
# Usage: sm_sweep.sh TAG FIELDX FIELDY VALUE...   types each VALUE into the field, records one 3.5 s note, prints matrix_report.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); W=${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}; export DISPLAY=:77
printf '0 note 60 100 3.5\n5 end\n' > "$W/sm.txt"; python3 "$here/scenario.py" "$W/sm.txt" "$W/sm.mid"
tag=$1; fx=$2; fy=$3; shift 3
for v in "$@"; do
  if [ "$v" != keep ]; then xdotool mousemove $fx $fy; sleep 0.3; xdotool click --repeat 2 --delay 100 1; sleep 0.5; xdotool key ctrl+a; xdotool type --delay 40 -- "$v"; xdotool key Return; sleep 0.8; fi
  xdotool mousemove 1300 300; "$here/record.sh" "$W/sm.mid" "$W/wav/sm_${tag}_${v}.wav" 2 >/dev/null
  echo "== $tag $v"; python3 "$here/matrix_report.py" "$W/wav/sm_${tag}_${v}.wav"
done
