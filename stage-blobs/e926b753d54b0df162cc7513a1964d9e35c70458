#!/usr/bin/env bash
# Save the edited instrument in the running Kontakt (classic view) as D:\kontra_ref_src\NAME.nki (patch only).
# Usage: save_nki.sh NAME   -> /mnt/MAIN_STORAGE/kontra_ref_src/NAME.nki
export DISPLAY=:77
click() { xdotool mousemove "$1" "$2"; sleep 0.4; xdotool click 1; sleep "${3:-1.2}"; }
click 323 98 1; click 185 159 2          # global menu > Save edited instrument as...
click 168 576 0.5                         # Patch Only
xdotool mousemove 530 729; xdotool click 1; xdotool key ctrl+a; xdotool type --delay 20 "D:\\kontra_ref_src\\$1.nki"
xdotool key Return; sleep 2
xdotool key Return; sleep 1               # overwrite prompt if any (harmless otherwise)
