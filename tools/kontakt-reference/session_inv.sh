#!/usr/bin/env bash
# Quiet session: Inverter Output sweep on the stereo noise instrument (calibration.nki), Inverter in instrument insert slot 1 and
# group insert slot 1. Taller desktop so the whole FX add menu fits. Run as ~/.cache/kontakto-quiet tools/kontakt-reference/session_inv.sh
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/session_lib.sh"; . "$here/gui_lib.sh"
export KONTAKT_RES=1600x1400 DISPLAY=:77
OUT=$W/log/session_inv.txt; : >"$OUT"
sess_begin
export KONTAKT_NO_VIEW_FIX=1 KONTRA_NO_STATE_CHECK="GUI edited (Inverter FX added, taller desktop); calibration before and after on the unedited instrument"
"$here/kontakt.sh" start "$CAL" || { say "start failed"; exit 1; }
two() { rec "$1_a" inv_note 6 1; rec "$1_b" inv_note 6 1; }
setfs() { setf "$1" "$2" "$3"; shot "inv_$4"; }
say "== no FX control"; two inv_none
classic_group_editor; shot inv_setup0
addfx 700 580 1041 2; edit_open || say "instrument edit panel did not open"; shot inv_setup1
addfx 700 328 684 2; shot inv_setup2
# layout now: group Output (1410,410), instrument Output (1410,740)
say "== instrument Output sweep (group Inverter 0 dB)"
n=0; for v in 0 -6 6 0; do n=$((n+1)); setfs 1410 740 "$v" "i$n"; two "inv_inst${n}_$v"; done
say "== group Output sweep (instrument Inverter 0 dB)"
setfs 1410 740 0 ig0
n=0; for v in -6 6 0; do n=$((n+1)); setfs 1410 410 "$v" "g$n"; two "inv_grp${n}_$v"; done
"$here/kontakt.sh" stop; sleep 3
sess_end
