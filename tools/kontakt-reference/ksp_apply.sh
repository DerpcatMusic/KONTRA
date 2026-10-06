#!/usr/bin/env bash
# Replace the script in the open Kontakt script slot (Script Editor open, slot's Edit view showing) with FILE and Apply.
# Usage: [TY=650] [AY=454] ksp_apply.sh FILE    TY = y of the text area, AY = y of the Apply button (x 1426); both
# move with the slot's Edit row (Edit row y + 32 = Apply y; text area starts ~26 px below).
export DISPLAY=:77
xdotool mousemove 1000 ${TY:-650}; sleep 0.3; xdotool click 1; sleep 0.3; xdotool key ctrl+a; xdotool key Delete
while IFS= read -r line; do xdotool type --delay 20 -- "$line"; xdotool key Return; sleep 0.1; done < "$1"
sleep 0.5; xdotool mousemove 1426 ${AY:-454}; sleep 0.3; xdotool click 1; sleep 1.5
import -window root /home/derpcat/.cache/kontra-reference/log/ksp.png
