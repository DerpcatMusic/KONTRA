#!/usr/bin/env bash
# Usage: probe_identify.sh ID  -> $P/id.json from whichever of kontakt.wav, kontra.wav, v1.wav exist
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); ID=$1; rel=$(grep -P "^$ID\t" "$here/probe_set.tsv" | cut -f3)
P=/home/derpcat/.cache/kontra-reference/probe/$ID; NKI="/mnt/MAIN_STORAGE/Libraries/Kontakt/$rel"
B=/home/derpcat/.cache/kontra-reference/bin/sampler-native-ksp
args=(); [ -s "$P/kontakt.wav" ] && args+=("kontakt=$P/kontakt.wav"); [ -s "$P/kontra.wav" ] && args+=("kontra=$P/kontra.wav"); [ -s "$P/v1.wav" ] && args+=("v1=$P/v1.wav*$(python3 -c "import json,sys;print(json.load(open(sys.argv[1])).get('scale',0.5))" "$P/grid.json")")
/home/derpcat/.cache/kontakto-heavy $B identify-kontakt "$NKI" "$P/grid.json" "$P/id.json" "${args[@]}" 2>&1 | grep -v '^note ' | tail -5
