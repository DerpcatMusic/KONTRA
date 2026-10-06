#!/usr/bin/env bash
# Play a MIDI file into the running Kontakt standalone and record its output.
# Usage: record.sh SCENARIO.mid OUT.wav [TAIL_SECONDS=3]
# Needs: Kontakt running on the kontra_ref sink with "Midi Through Port-0" on
# Port A (see kontakt.sh), PipeWire, aplaymidi.
set -euo pipefail
mid=$1 out=$2 tail=${3:-3}
"$(dirname "$0")/kontakt.sh" route || { echo "record: routing guard failed, no MIDI sent" >&2; exit 1; }
pw-record --target kontra_ref -P '{ stream.capture.sink=true }' --rate 48000 --format f32 --channels 2 "$out" &
rec=$!
sleep 1
aplaymidi -p 14:0 "$mid"
sleep "$tail"
kill -INT "$rec"; wait "$rec" 2>/dev/null || true
