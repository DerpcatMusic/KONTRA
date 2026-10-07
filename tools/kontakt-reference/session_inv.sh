#!/usr/bin/env bash
# Quiet session: Inverter Output sweep on the stereo noise instrument (calibration.nki), Inverter in instrument insert slot 1 and
# group insert slot 1. Taller desktop so the whole FX add menu fits. Run as ~/.cache/kontakto-quiet tools/kontakt-reference/session_inv.sh
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/session_lib.sh"
export KONTAKT_RES=1600x1400 DISPLAY=:77
OUT=$W/log/session_inv.txt; : >"$OUT"
sess_begin
export KONTAKT_NO_VIEW_FIX=1 KONTRA_NO_STATE_CHECK="GUI edited (Inverter FX added, taller desktop); calibration before and after on the unedited instrument"
"$here/kontakt.sh" start "$CAL" || { say "start failed"; exit 1; }
clk() { xdotool mousemove "$1" "$2"; sleep .5; xdotool click 1; sleep "${3:-2}"; }
shot() { import -window root -crop 1000x800+540+180 "$W/log/inv_$1.png"; }
two() { rec "$1_a" inv_note 6 1; rec "$1_b" inv_note 6 1; }
setf() { xdotool mousemove "$1" "$2"; sleep .5; xdotool click --repeat 2 --delay 100 1; sleep .6; xdotool key ctrl+a; xdotool type --delay 40 -- "$3"; xdotool key Return; sleep 1; xdotool mousemove 1300 300; sleep 1; shot "$4"; }
say "== no FX control"; two inv_none
clk 1178 981; clk 325 98; clk 128 579 5; clk 565 203; clk 770 307
xdotool mousemove 1000 500; for i in $(seq 25); do xdotool click 5; sleep .2; done; sleep 4; shot setup0
addfx() { # plus-x plus-y utilities-y
  clk 590 540 1; clk "$1" "$2" 3; xdotool mousemove 730 "$3"; sleep 1; xdotool key Right; sleep .5; xdotool key Down; sleep .2; xdotool key Down; sleep .3; xdotool key Return; sleep 3; }
addfx 700 580 1041; clk 590 540 1; clk 562 598 3; shot setup1
addfx 700 328 684; shot setup2
# layout now: group Output (1410,410), instrument Output (1410,740)
say "== instrument Output sweep (group Inverter 0 dB)"
n=0; for v in 0 -6 6 0; do n=$((n+1)); setf 1410 740 "$v" "i$n"; two "inv_inst${n}_$v"; done
say "== group Output sweep (instrument Inverter 0 dB)"
setf 1410 740 0 ig0
n=0; for v in -6 6 0; do n=$((n+1)); setf 1410 410 "$v" "g$n"; two "inv_grp${n}_$v"; done
"$here/kontakt.sh" stop; sleep 3
sess_end
