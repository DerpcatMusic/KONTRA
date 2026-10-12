#!/usr/bin/env bash
# Render one scenario through Kontakt (already started via kontakt.sh) and KONTRA, then compare.
# Usage: run.sh INSTRUMENT.nki SCENARIO.txt [NAME]   (outputs in ~/.cache/kontra-reference/wav/)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
W="${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}"
nki=$1 scen=$2 name=${3:-$(basename "$scen" .txt)}
python3 "$here/scenario.py" "$scen" "$W/wav/$name.mid"
"$here/record.sh" "$W/wav/$name.mid" "$W/wav/$name.kontakt.wav" "${TAIL:-3}"
(cd "$repo" && /home/derpcat/.cache/kontakto-heavy cargo run --release -q -p sampler-native -- \
  render-kontakt-midi "$nki" "$W/wav/$name.kontra.wav" "$W/wav/$name.mid")
python3 "$here/compare.py" "$W/wav/$name.kontakt.wav" "$W/wav/$name.kontra.wav" ${HOP:+--hop $HOP}
