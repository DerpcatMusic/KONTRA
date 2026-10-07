#!/bin/bash
# Load D:\kontra_ref_src\noise.wav as instrument "noise" (Kontakt up in classic view, 1600x1000). Then: ksp_apply.sh scenarios/key_fade.ksp
export DISPLAY=:77
python3 "$(dirname "$0")/make_noise.py" /tmp/noise.wav && mkdir -p /mnt/MAIN_STORAGE/kontra_ref_src && cp /tmp/noise.wav /mnt/MAIN_STORAGE/kontra_ref_src/noise.wav
xdotool mousemove 195 152 click 1; sleep 2; xdotool mousemove 42 331 click 1; sleep 2; xdotool mousemove 157 520 click 1; sleep 2
xdotool mousemove 90 663 click 1; sleep 1; xdotool mousemove 90 663 click --repeat 2 --delay 100 1; sleep 4
