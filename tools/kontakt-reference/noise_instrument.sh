#!/bin/bash
# Load D:\kontra_ref_src\NAME.wav as an instrument (Kontakt up in classic view, 1600x1000; Files tab, D: expanded once).
# Usage: noise_instrument.sh [noise|stereo|burst]   (writes the sample with make_noise.py first). Then e.g. ksp_apply.sh scenarios/key_fade.ksp
export DISPLAY=:77; n=${1:-noise}
python3 "$(dirname "$0")/make_noise.py" /tmp/$n.wav $n && mkdir -p /mnt/MAIN_STORAGE/kontra_ref_src && cp /tmp/$n.wav /mnt/MAIN_STORAGE/kontra_ref_src/$n.wav
xdotool mousemove 195 152 click 1; sleep 2; xdotool mousemove 42 331 click 1; sleep 2; xdotool mousemove 157 520 click 1; sleep 2
y=663; [ "$n" = noise ] && y=690; [ "$n" = stereo ] && y=717   # alphabetical rows: burst, noise, stereo
xdotool mousemove 90 $y click 1; sleep 1; xdotool mousemove 90 $y click --repeat 2 --delay 100 1; sleep 4
