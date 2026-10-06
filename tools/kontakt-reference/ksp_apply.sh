#!/usr/bin/env bash
# Replace the script in Kontakt's script slot 1 (Script Editor tab open, edit view) with FILE and Apply.
export DISPLAY=:77
xdotool mousemove 1000 650; sleep 0.3; xdotool click 1; sleep 0.3; xdotool key ctrl+a; xdotool key Delete
while IFS= read -r line; do xdotool type --delay 20 -- "$line"; xdotool key Return; sleep 0.1; done < "$1"
sleep 0.5; xdotool mousemove 1426 454; sleep 0.3; xdotool click 1; sleep 1.5
import -window root /home/derpcat/.cache/kontra-reference/log/ksp.png
