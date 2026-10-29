#!/usr/bin/env bash
# Run from any directory; all Wine work goes through inspect's oracle-slot path.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1
exec python3 "$root/tools/inspect/oracle-diff.py" "$@"
