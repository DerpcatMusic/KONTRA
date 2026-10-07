#!/usr/bin/env bash
# One quiet session (run as ~/.cache/kontakto-quiet tools/kontakt-reference/session_una.sh): calibrate, Una recordings x2, calibrate again.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); W=/home/derpcat/.cache/kontra-reference; export KONTRA_MAX_LOAD=${KONTRA_MAX_LOAD:-8}
OUT=$W/log/session_una.txt; : >"$OUT"; say() { echo "$@" | tee -a "$OUT"; }
CAL=/mnt/MAIN_STORAGE/kontra_ref_src/calibration.nki
UNA="/mnt/MAIN_STORAGE/Libraries/Kontakt/Una Corda Library/Instruments/Una Corda Cotton.nki"
# persistent wineserver so the calibration stamp (sink + wineserver pid) survives Kontakt restarts; killed by PID at the end
before=$(pgrep -x wineserver | sort); wineserver -p; sleep 1
WS=$(comm -13 <(echo "$before") <(pgrep -x wineserver | sort) | head -1); say "persistent wineserver pid ${WS:-existing}"
trap '[ -n "${WS:-}" ] && kill "$WS"' EXIT
say "load at start: $(cat /proc/loadavg)"
"$here/kontakt.sh" start "$CAL" && "$here/kontakt.sh" calibrate | tail -2 | tee -a "$OUT" || { say "PRE-CALIBRATION FAILED"; "$here/kontakt.sh" stop; exit 1; }
"$here/kontakt.sh" stop; sleep 3
"$here/kontakt.sh" start "$UNA" && "$here/una_setup.sh"
rec() { # name scenario spacing n
  python3 "$here/scenario.py" "$here/scenarios/$2.txt" "$W/wav/$1.mid" >/dev/null &&
  "$here/record.sh" "$W/wav/$1.mid" "$W/wav/$1.wav" 3 >/dev/null 2>&1 || { say "$1: RECORD FAILED"; return; }
  say "$1 [$(grep load1 "$W/wav/$1.wav.log")]"; python3 "$here/note_levels.py" "$W/wav/$1.wav" "$3" "$4" | tee -a "$OUT"; }
for r in a b; do
  rec g39_v100_$r una_g39_v100 6 1; rec g94_v100_$r una_g94_v100 6 1
  rec g39_cc64_$r una_g39_v100_cc64 6 1; rec g39_multi_$r una_g39_multi 6 3
done
"$here/kontakt.sh" stop; sleep 3
"$here/kontakt.sh" start "$CAL" && "$here/kontakt.sh" calibrate | tail -2 | tee -a "$OUT" || say "POST-CALIBRATION FAILED"
"$here/kontakt.sh" stop; say "load at end: $(cat /proc/loadavg)"
