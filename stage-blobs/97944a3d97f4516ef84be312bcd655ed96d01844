#!/usr/bin/env bash
# Quiet session: Gainer smoothing. Held 9 s noise note, instrument-insert Gainer Gain stepped mid-note with the GUI (type value, Return at ~+4.5 s).
# Steps 0 -> -24 dB and 0 -> -6 dB, two repeats each. Run as ~/.cache/kontakto-quiet tools/kontakt-reference/session_gain.sh
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/session_lib.sh"; . "$here/gui_lib.sh"
export KONTAKT_RES=1600x1400 DISPLAY=:77
OUT=$W/log/session_gain.txt; : >"$OUT"
sess_begin
export KONTAKT_NO_VIEW_FIX=1 KONTRA_NO_STATE_CHECK="GUI edited (Gainer added, taller desktop); calibration before and after on the unedited instrument"
"$here/kontakt.sh" start "$CAL" || { say "start failed"; exit 1; }
classic_group_editor; shot gain_setup0
addfx 700 580 1041 1; edit_open || say "edit panel did not open"; shot gain_setup1
python3 "$here/scenario.py" "$here/scenarios/gain_step.txt" "$W/wav/gs.mid" >/dev/null
for step in -24 -6 -24 -6; do
  setf 715 662 0; typef 715 662 "$step"; shot "gain_pre_$step"
  nm=gs_${step#-}_$RANDOM
  ( "$here/record.sh" "$W/wav/gs.mid" "$W/wav/$nm.wav" 3 >/dev/null 2>"$W/log/rec_err.txt" ) &
  rp=$!
  for _ in $(seq 150); do pgrep -x pw-record >/dev/null && break; sleep .1; done
  sleep 4.6; xdotool key Return
  wait $rp || say "$nm: RECORD FAILED: $(tail -1 "$W/log/rec_err.txt")"
  say "== step 0 -> $step dB ($nm) [$(grep -E 'load1|midi_wall' "$W/wav/$nm.wav.log" | tr '\n' ' ')]"
  python3 "$here/gain_step.py" "$W/wav/$nm.wav" "$step" | tee -a "$OUT"
done
shot gain_end
"$here/kontakt.sh" stop; sleep 3
sess_end
