#!/usr/bin/env bash
# Play a MIDI file into the running Kontakt standalone and record its output.
# Usage: record.sh SCENARIO.mid OUT.wav [TAIL_SECONDS=3]
# Needs: Kontakt running on the kontra_ref sink with "Midi Through Port-0" on
# Port A (see kontakt.sh), PipeWire, aplaymidi.
set -euo pipefail
mid=$1 out=$2 tail=${3:-3}
here=$(cd "$(dirname "$0")" && pwd); W=${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}
# --- reference protocol gates (docs/architecture-v2/REFERENCE_PROTOCOL.md) ---
if [ -z "${KONTRA_NO_STAMP_CHECK:-}" ]; then
  [ -f "$W/calibrated" ] || { echo "record: no calibration stamp, run kontakt.sh calibrate first" >&2; exit 1; }
  read -r sink wsrv _ <"$W/calibrated"
  [ "$sink" = "$(pactl list short modules | awk '/kontra_ref/{print $1}')" ] && [ "$wsrv" = "$("$here/wsid.sh")" ] || { echo "record: calibration stamp is from another sink/wine session, recalibrate" >&2; exit 1; }
  ok=; for _ in 1 2 3; do "$here/kontakt.sh" state >"$W/log/state.txt" 2>&1 && { ok=1; break; }; sleep 3; done   # retry: a transient redraw can fail one capture
  [ -n "$ok" ] || { cat "$W/log/state.txt" >&2; echo "record: Kontakt state is not pinned ($(grep MISMATCH "$W/log/state.txt" | tr '\n' ';')), no MIDI sent" >&2; exit 1; }
fi
load1() { cut -d' ' -f1 /proc/loadavg; }
maxload=${KONTRA_MAX_LOAD:-8}; for _ in $(seq 12); do load_start=$(load1); awk -v l="$load_start" -v m="$maxload" 'BEGIN{exit !(l<m)}' && break; sleep 5; done   # wait up to 60 s for the load to drop
[ -n "${KONTRA_NO_LOAD_CHECK:-}" ] || awk -v l="$load_start" -v m="$maxload" 'BEGIN{exit !(l<m)}' || { echo "record: load average $load_start >= $maxload, Kontakt drops voices under load; wait or run inside a kontakto-heavy slot" >&2; exit 1; }
[ -f "$mid.proto" ] || { echo "record: $mid has no .proto sidecar, build it with scenario.py" >&2; exit 1; }
"$here/kontakt.sh" route || { echo "record: routing guard failed, no MIDI sent" >&2; exit 1; }
# keep-alive: the null sink suspends after a few idle seconds and pw-record then stops capturing (notes after a long silence were lost);
# a silent playback stream keeps the graph running and adds exact zeros
pactl list short sinks | grep -q kontra_ref || { echo "record: no kontra_ref sink" >&2; exit 1; }
pw-cat -p --raw --rate 48000 --channels 2 --format f32 --target kontra_ref /dev/zero & ka=$!
trap 'kill $ka 2>/dev/null' EXIT
pw-record --target kontra_ref -P '{ stream.capture.sink=true }' --rate 48000 --format f32 --channels 2 "$out" &
rec=$!
sleep 1
t_m0=$(date +%s.%N); aplaymidi -p 14:0 "$mid"; t_m1=$(date +%s.%N)
sleep "$tail"
alive=no; kill -0 "$rec" 2>/dev/null && alive=yes
kill -INT "$rec"; wait "$rec" 2>/dev/null || true
{ echo "time=$(date -Is)"; echo "midi_sha256=$(sha256sum "$mid" | cut -d' ' -f1) controller_state=$(cat "$mid.proto")"
  echo "midi_wall_s=$(echo "$t_m1 - $t_m0" | bc) recorder_alive_after_tail=$alive"; echo "load1_start=$load_start load1_end=$(load1) (max $maxload)"; echo "sample_rate=$(python3 -c "import struct,sys;print(struct.unpack('<I',open(sys.argv[1],'rb').read(28)[24:28])[0])" "$out") (protocol: 48000)"
  echo "calibration=$(cat "$W/log/calibration.txt" 2>/dev/null | tail -1)"; cat "$W/log/state.txt" 2>/dev/null || echo "state=unchecked (calibration recording)"; } >"$out.log"
