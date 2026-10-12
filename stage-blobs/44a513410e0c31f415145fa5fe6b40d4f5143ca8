#!/usr/bin/env bash
# Level of one instrument in Kontakt vs KONTRA: three notes (vel 64/100/127) 5 s apart.
# Usage: loudness.sh NAME INSTRUMENT.nki KEY    (starts and stops Kontakt itself)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../.." && pwd)
W=${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}; export DISPLAY=:77
name=$1 nki=$2 key=$3
printf '0 note %s 64 3\n5 note %s 100 3\n10 note %s 127 3\n14 end\n' $key $key $key > "$W/loud_$name.txt"
python3 "$here/scenario.py" "$W/loud_$name.txt" "$W/wav/loud_$name.mid"
"$here/kontakt.sh" start "$nki"
"$here/record.sh" "$W/wav/loud_$name.mid" "$W/wav/loud_$name.kontakt.wav" 4
"$here/kontakt.sh" stop; sleep 3
(cd "$repo" && /home/derpcat/.cache/kontakto-heavy cargo build --release -q -p sampler-native 2>&1 | grep -E "^error" || true)
bin=$(cd "$repo" && cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import json,sys;print(json.load(sys.stdin)['target_directory'])")/release/sampler-native
rm -f "$W/wav/loud_$name.kontra.wav"; "$bin" render-kontakt-midi "$nki" "$W/wav/loud_$name.kontra.wav" "$W/wav/loud_$name.mid" 2>&1 2>&1 | grep -E "outcome|rendered|rror" || true
python3 "$here/loudness.py" "$W/wav/loud_$name.kontakt.wav" "$W/wav/loud_$name.kontra.wav" 5 64 100 127
