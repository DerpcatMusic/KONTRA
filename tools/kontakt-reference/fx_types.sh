#!/usr/bin/env bash
# Record every lowpass type of the group-insert filter at cutoff 1000 Hz and reso 0/50/80 (classic view, group editor, filter in slot 1).
export DISPLAY=:77
here=$(cd "$(dirname "$0")" && pwd)
click() { xdotool mousemove "$1" "$2"; sleep 0.4; xdotool click 1; sleep "${3:-1}"; }
names=(SV_LP1 SV_LP2 SV_LP4 SV_LP6 Ladder_LP1 Ladder_LP2 Ladder_LP3 Ladder_LP4 Monark_LP1 Monark_LP2 Monark_LP4 AR_LP2 AR_LP4 AR_LP2_4 Daft_LP Pro53 Legacy_LP1 Legacy_LP2 Legacy_LP4)
for i in "${!names[@]}"; do
  [ -n "$1" ] && [[ " $* " != *" ${names[$i]} "* ]] && continue
  y=$((321 + 36 * i))
  click 700 754 1; click 700 785 1.2; click 918 $y 1.5
  FIELD_Y=786 FIELD_X=990 "$here/fx_sweep.sh" "t_${names[$i]}_r" 0 >/dev/null  # sets reso 0 (no record needed, overwritten)
  FIELD_Y=786 FIELD_X=845 "$here/fx_sweep.sh" "t_${names[$i]}_c" 1000 >/dev/null
  for r in 0 50 80; do FIELD_Y=786 FIELD_X=990 "$here/fx_sweep.sh" "t_${names[$i]}" $r >/dev/null; done
done
