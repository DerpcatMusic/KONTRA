#!/usr/bin/env bash
# Run a read-only Ghidra script against the analysed Kontakt / UVI project.
#   tools/re/query.sh 'get_group_volume' [depth]               # Kontakt 8.13.1
#   RE_PROJECT=Workstation409 tools/re/query.sh 'string' [depth]
# Output on stdout (Ghidra log noise stripped). Never re-analyses.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dir=${RE_PROJECT_DIR:-/home/derpcat/kontakto-engine-analysis-20261007/ghidra-project}
project=${RE_PROJECT:-Kontakt8131}
script=${RE_SCRIPT:-DecompileByString.java}
log=$(mktemp)
# One headless session per project at a time (Ghidra locks it): queue.
flock "/tmp/ghidra-query-$project.lock" /home/derpcat/.local/bin/analyzeHeadless "$dir" "$project" -process -noanalysis -readOnly \
  -scriptPath "$here/ghidra" -postScript "$script" "$@" >"$log" 2>&1 || { tail -40 "$log" >&2; exit 1; }
# Script output: everything after "SCRIPT:", minus the logger's prefix/suffix.
awk -v tag="${script%.java}.java> " '
  /^INFO  SCRIPT: /{on=1; next}
  on && /^INFO  (REPORT|HEADLESS)/{on=0}
  on { sub("^INFO  " tag, ""); sub(" [(]GhidraScript[)]  $", ""); print }' "$log"
rm -f "$log"
