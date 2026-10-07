#!/usr/bin/env bash
# Sweep a filter's displayed cutoff (Hz) on a running Kontakt whose instrument on MIDI ch 2
# is white noise with the filter in Group Insert FX slot 1 (edit mode open, cutoff field at 845,562).
# Usage: fx_sweep.sh TAG HZ... ; writes wav/fx_TAG_HZ.wav ; first record "ref" with Byp on (tag ref, no Hz).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); W=${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}
export DISPLAY=:77
tag=$1; shift
printf '0.0 on 60 100\n 2.5 off 60\n3.0 end\n' | sed 's/^/ch=1 /;/end/d' > "$W/fx.txt"
echo "ch=1 0.0 on 60 100" > "$W/fx.txt"; echo "ch=1 2.5 off 60" >> "$W/fx.txt"; echo "3.0 end" >> "$W/fx.txt"
python3 "$here/scenario.py" "$W/fx.txt" "$W/fx.mid"
for hz in "$@"; do
  if [ "$hz" != ref ]; then
    xdotool mousemove ${FIELD_X:-845} ${FIELD_Y:-562}; sleep 0.3; xdotool click --repeat 2 --delay 100 1; sleep 0.5
    xdotool key ctrl+a; xdotool type --delay 40 "$hz"; xdotool key Return; sleep 0.8
  fi
  "$here/record.sh" "$W/fx.mid" "$W/wav/fx_${tag}_${hz}.wav" 1.5
done
