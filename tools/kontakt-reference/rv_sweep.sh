#!/usr/bin/env bash
# Reverb (Send FX slot 1, plain "Reverb", Mode: Room) sweep on the burst instrument; reverb panel open and scrolled as in KONTAKT_REFERENCE.md section 21.
# Needs $W/wav/dry.wav = same scenario with the reverb bypassed.
# Usage: rv_sweep.sh TAG FIELDX FIELDY VALUE...   types each VALUE (or `keep`), records a 14 s note, prints reverb_report; deletes the WAV.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); W=${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}; export DISPLAY=:77
python3 "$here/scenario.py" "$here/scenarios/reverb_burst.txt" "$W/rb.mid"
tag=$1; fx=$2; fy=$3; shift 3
for v in "$@"; do
  if [ "$v" != keep ]; then xdotool mousemove $fx $fy; sleep 0.3; xdotool click --repeat 2 --delay 100 1; sleep 0.5; xdotool key ctrl+a; xdotool type --delay 40 -- "$v"; xdotool key Return; sleep 0.8; fi
  xdotool mousemove 1300 300; "$here/record.sh" "$W/rb.mid" "$W/wav/rv_${tag}_${v}.wav" 2 >/dev/null
  python3 "$here/reverb_report.py" "$W/wav/rv_${tag}_${v}.wav" "$W/wav/dry.wav" "$tag=$v"
  rm -f "$W/wav/rv_${tag}_${v}.wav"
done
