#!/usr/bin/env bash
# Quiet session: Kontakt plays the probe grid of each instrument in IDS (probe_set.tsv); one launch per instrument.
# Run as: IDS="una_cotton" ~/.cache/kontakto-quiet tools/kontakt-reference/session_probe.sh
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/session_lib.sh"
OUT=$W/log/session_probe.txt; : >"$OUT"
sess_begin
for id in $IDS; do
  rel=$(grep -P "^$id\t" "$here/probe_set.tsv" | cut -f3); keys=$(grep -P "^$id\t" "$here/probe_set.tsv" | cut -f2)
  P=$W/probe/$id; mkdir -p "$P"; mode=$(grep -P "^$id\t" "$here/probe_set.tsv" | cut -f4); python3 "$here/probe_grid.py" "$P" "$keys" MODE=${mode:-grid} | tee -a "$OUT"; python3 "$here/scenario.py" "$P/scen.txt" "$P/scen.mid" >/dev/null
  KONTAKT_NO_VIEW_FIX=1 "$here/kontakt.sh" start "/mnt/MAIN_STORAGE/Libraries/Kontakt/$rel" || { say "$id: start failed"; "$here/kontakt.sh" stop; continue; }
  export KONTAKT_NO_VIEW_FIX=1 KONTRA_NO_STATE_CHECK="probe $id: instrument GUI differs from the golden header, default state and nothing touched; calibration before and after"
  rm -f "$P/kontakt.wav"
  "$here/record.sh" "$P/scen.mid" "$P/kontakt.wav" 3 >/dev/null 2>"$W/log/rec_err.txt" && say "$id: recorded $(grep -E 'capture_actual|load1' "$P/kontakt.wav.log" | tr '\n' ' ')" || say "$id: RECORD FAILED: $(tail -1 "$W/log/rec_err.txt")"
  "$here/kontakt.sh" stop; sleep 3
done
if [ -n "${SHOT_NKI:-}" ]; then  # GUI-only look (no MIDI, no recording): screenshot the performance view and grep Kontakt's log
  KONTAKT_NOAUDIO=1 KONTAKT_NO_VIEW_FIX=1 "$here/kontakt.sh" start "$SHOT_NKI" && { sleep 5; import -window root "$W/log/shot_${SHOT_NAME:-gui}.png"; grep -iE 'articulation_list|missing|resource|cannot|not found' "$W/log/kontakt.log" | head -20 >"$W/log/shot_${SHOT_NAME:-gui}.txt"; say "shot ${SHOT_NAME:-gui}: $W/log/shot_${SHOT_NAME:-gui}.png"; }
  "$here/kontakt.sh" stop; sleep 3
fi
sess_end
