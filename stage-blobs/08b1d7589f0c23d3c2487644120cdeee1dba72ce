#!/usr/bin/env bash
# Quiet session: ANALOG STRINGS default state (scripts on, nothing touched), vel 100, 3 s notes C4/E4/G4 each from a fresh note-on, then the
# same note twice 6 s apart. REPS="a" or "b" = one fresh Kontakt launch per rep (round-robin / script state starts equal).
# Run as ~/.cache/kontakto-quiet tools/kontakt-reference/session_analog.sh
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/session_lib.sh"
OUT=$W/log/session_analog.txt; : >"$OUT"
sess_begin
NKI="/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki"
lv() { python3 "$here/comp_levels.py" "$W/wav/$1.wav" "${2:-3}" "${3:-1}" "${4:-6}" | tee -a "$OUT"; }
for r in ${REPS:-a b}; do
  KONTAKT_NO_VIEW_FIX=1 "$here/kontakt.sh" start "$NKI" || { say "start failed"; continue; }
  export KONTAKT_NO_VIEW_FIX=1 KONTRA_NO_STATE_CHECK="ANALOG STRINGS GUI is wider than the 1600x1000 desktop so the golden header crops do not match; nothing touched (default state), calibration before and after"
  for k in c4 e4 g4; do rec an_${k}_$r analog_$k 6 1 && lv an_${k}_$r; done
  rec an_c4x2_$r analog_c4_twice 6 2 && lv an_c4x2_$r 3 2 6
  "$here/kontakt.sh" stop; sleep 3
done
sess_end
