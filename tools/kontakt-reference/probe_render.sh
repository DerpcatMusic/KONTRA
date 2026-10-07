#!/usr/bin/env bash
# KONTRA + v1 renders of one probe grid (no audio of the library leaves memory except these renders), then identify against
# whatever kontakt.wav is present. Usage: probe_render.sh ID   (grid made by probe_grid.py in $P)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); ID=$1
read -r _ keys rel < <(grep -P "^$ID\t" "$here/probe_set.tsv"); rel=$(grep -P "^$ID\t" "$here/probe_set.tsv" | cut -f3)
P=/home/derpcat/.cache/kontra-reference/probe/$ID; NKI="/mnt/MAIN_STORAGE/Libraries/Kontakt/$rel"
T=/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target
B=$T/kontakto-kontakt-reference/release/sampler-native; V=$T/kontakto-v1-gate/release/kontakto
H=/home/derpcat/.cache/kontakto-heavy
mkdir -p "$P"; python3 "$here/probe_grid.py" "$P" "$keys" >/dev/null; python3 "$here/scenario.py" "$P/scen.txt" "$P/scen.mid" >/dev/null
rm -f "$P/kontra.wav" "$P/v1.wav"
KONTRA_PLAY_KEYS=1 $H $B render-kontakt-midi "$NKI" "$P/kontra.wav" "$P/scen.mid" >"$P/kontra.log" 2>&1 || echo "$ID: kontra render failed: $(tail -1 "$P/kontra.log")"
$H $V render "$NKI" "$P/v1.wav" all --notes "$(sed -n 1p "$P/v1.args")" --cc "$(sed -n 2p "$P/v1.args")" >"$P/v1.log" 2>&1 || echo "$ID: v1 render failed: $(tail -1 "$P/v1.log")"
