#!/usr/bin/env bash
# Quiet session: ANALOG STRINGS with the instrument Insert FX slot 2 compressor (Classic, thr -14.2 dB, 1:2.0, 26 ms, 200.5 ms, out +9.0 dB) BYPASSED
# in the GUI, C4/E4/G4 vel 100 held 3 s, twice each in one launch. Run as ~/.cache/kontakto-quiet tools/kontakt-reference/session_analog_byp.sh
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/session_lib.sh"; . "$here/gui_lib.sh"
export DISPLAY=:77
OUT=$W/log/session_analog_byp.txt; : >"$OUT"
sess_begin
NKI="/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki"
lv() { python3 "$here/comp_levels.py" "$W/wav/$1.wav" 3 1 6 | tee -a "$OUT"; }
export KONTAKT_NO_VIEW_FIX=1 KONTRA_NO_STATE_CHECK="ANALOG STRINGS GUI is wider than the 1600x1000 desktop so the golden header crops do not match; only the instrument compressor slot was bypassed in the GUI, calibration before and after"
"$here/kontakt.sh" start "$NKI" || { say "start failed"; exit 1; }
sleep 10; xdotool mousemove 1000 20
clk 565 203 3; clk 1370 308 3; clk 830 308 4          # Edit mode, close Script Editor, open Group Editor
xdotool mousemove 1000 700; for _ in $(seq 50); do xdotool click 5; sleep .15; done; sleep 2
clk 763 520 2; xdotool mousemove 1000 640; for _ in $(seq 5); do xdotool click 5; sleep .15; done; sleep 2   # select Comp slot, scroll to its panel
clk 560 533 2; xdotool mousemove 1000 640; sleep 1
import -window root -crop 40x20+545+523 +repage /tmp/byp.png; px=$(magick /tmp/byp.png -scale 1x1 -format '%[fx:int(255*r)] %[fx:int(255*g)]' info:); say "Byp button pixel R G: $px"
import -window root -crop 1000x200+540+380 "$W/log/analog_byp.png"
set -- $px; { [ "$1" -gt 200 ] && [ "$2" -lt 90 ]; } || { say "compressor NOT bypassed, abort"; "$here/kontakt.sh" stop; sess_end; exit 1; }
for r in x y; do for k in c4 e4 g4; do rec an_byp_${k}_$r analog_$k 6 1 && lv an_byp_${k}_$r; done; done
"$here/kontakt.sh" stop; sleep 3
sess_end
