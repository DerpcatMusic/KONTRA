#!/usr/bin/env bash
# Volume AHD(SR) envelope sweep on the noise instrument (Group Editor, rack scrolled so the Modulation > Volume row is at y 818).
# Usage: env_sweep.sh TAG FIELDX FIELDY VALUE...   fields: Curve 700, Attack 860, Hold 998, Decay 1135, Sustain 1265, Release 1410 (all y 818)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); W=${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}; export DISPLAY=:77
python3 "$here/scenario.py" "$here/scenarios/${SCEN:-ahd_note}.txt" "$W/en.mid"
tag=$1; fx=$2; fy=$3; shift 3
for v in "$@"; do
  if [ "$v" != keep ]; then xdotool mousemove $fx $fy; sleep 0.3; xdotool click --repeat 2 --delay 100 1; sleep 0.5; xdotool key ctrl+a; xdotool type --delay 40 -- "$v"; xdotool key Return; sleep 0.8; fi
  xdotool mousemove 1300 300; "$here/record.sh" "$W/en.mid" "$W/wav/en_${tag}_${v}.wav" 2 >/dev/null
  python3 "$here/env_ahd.py" "$W/wav/en_${tag}_${v}.wav" "$tag=$v"
  rm -f "$W/wav/en_${tag}_${v}.wav"
done
