#!/usr/bin/env bash
# Census of the convolution record flags (0x16 / BParFXIRC) across Kontakt's factory presets.
#   conv_flag_census.sh NKP_DECODE_BIN PRESETS_DIR > census.tsv
# NKP_DECODE_BIN: target/debug/examples/nkp_decode (cargo build -p sampler-kontakt --example nkp_decode).
# Field ids: 10 reverse, 11 auto gain, 12 preserve length, 13 latency bypass, 14 volume envelope.
set -euo pipefail
bin=$1; dir=$2
find "$dir" -name '*.nkp' -print0 | while IFS= read -r -d '' f; do
  "$bin" dump "$f" 2>/dev/null | awk -v f="$f" -F'\t' '$2=="BParFXIRC"{v[$4]=$9} END{if (length(v)) printf "%s\t%s\t%s\t%s\t%s\t%s\n", f, v[10], v[11], v[12], v[13], v[14]}'
done
