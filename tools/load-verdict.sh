#!/usr/bin/env bash
# Load-only verdict for every Kontakt/UVI instrument: read, translate and native
# lowering, the same path the plugin takes, with no decoding or rendering.
# Writes one JSONL row per item to OUT and prints the failures.
# Usage: tools/load-verdict.sh OUT.jsonl [ROOT...]
#   default ROOTs: $KONTRA_LIBRARIES/{Kontakt,UVI} (kontra_libraries.sh)
# Known failures on main: ANALOG STRINGS, Una Corda Pure. Anything else is a regression.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?usage: tools/load-verdict.sh OUT.jsonl [ROOT...]}"
shift
. "$here/kontra_libraries.sh"
[ $# -gt 0 ] || set -- "$KONTRA_LIBRARIES/Kontakt" "$KONTRA_LIBRARIES/UVI"
nice -n 19 "$here/corpus-health/run.sh" run "$out" --tier parse --workers 2 "$@"
python3 -I - "$out" <<'PY'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
done = [r for r in rows if "load" in r]
bad = [r for r in done if not (r["load"] or {}).get("ok")]
print(f"{len(done)} instruments: {len(done) - len(bad)} load ok, {len(bad)} failed")
for r in bad:
    print(f"FAIL {r['id']}: {(r['load'] or {}).get('error')}")
PY
