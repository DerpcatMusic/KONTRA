# shared helpers for quiet sessions (source me); sets W, OUT must be set by the caller before sess_begin
W=/home/derpcat/.cache/kontra-reference; export KONTRA_MAX_LOAD=${KONTRA_MAX_LOAD:-8}
CAL=/mnt/MAIN_STORAGE/kontra_ref_src/calibration.nki
say() { echo "$@" | tee -a "$OUT"; }
wine_pids() { pgrep -x "wineserver|services.exe|winedevice.exe|explorer.exe|plugplay.exe|svchost.exe|rpcss.exe|Kontakt 8.exe|start.exe|Xvfb" | sort; }
sess_begin() {
  allb=$(wine_pids); before=$(pgrep -x wineserver | sort); wineserver -p; sleep 1
  WS=$(comm -13 <(echo "$before") <(pgrep -x wineserver | sort) | head -1); say "persistent wineserver pid ${WS:-existing}"
  export KONTRA_WS_PID=${WS:-}
  trap 'sleep 2; for p in $(comm -13 <(echo "$allb") <(wine_pids)); do kill $p 2>/dev/null; done; sleep 1; for p in $(comm -13 <(echo "$allb") <(wine_pids)); do kill -9 $p 2>/dev/null; done; true' EXIT
  say "load at start: $(cat /proc/loadavg)"
  "$here/kontakt.sh" start "$CAL" && "$here/kontakt.sh" calibrate | tail -2 | tee -a "$OUT" || { say "PRE-CALIBRATION FAILED"; "$here/kontakt.sh" stop; exit 1; }
  "$here/kontakt.sh" stop; sleep 3
}
rec() { # name scenario spacing n
  python3 "$here/scenario.py" "$here/scenarios/$2.txt" "$W/wav/$1.mid" >/dev/null &&
  "$here/record.sh" "$W/wav/$1.mid" "$W/wav/$1.wav" 3 >/dev/null 2>"$W/log/rec_err.txt" || { say "$1: RECORD FAILED: $(tail -1 "$W/log/rec_err.txt")"; return; }
  say "$1 [$(grep -E 'load1|midi_wall' "$W/wav/$1.wav.log" | tr '\n' ' ')]"; python3 "$here/note_levels.py" "$W/wav/$1.wav" "$3" "$4" | tee -a "$OUT"; }
sess_end() {
  "$here/kontakt.sh" start "$CAL" && "$here/kontakt.sh" calibrate | tail -2 | tee -a "$OUT" || say "POST-CALIBRATION FAILED"
  "$here/kontakt.sh" stop; say "load at end: $(cat /proc/loadavg)"
}
