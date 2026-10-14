#!/bin/bash
# Audit batch A: save the levels probe instrument with Kontakt, re-dump it natively, diff against KONTRA's IR.
# Outputs in /tmp/w3/audit (mkdir first).  IR_JSON = the ir_json example binary.
set -e
cd "$(dirname "$0")/../.."
OUT=${OUT:-/tmp/w3/audit}
ORACLE_LUA=tools/kontakt-oracle/probes/audit_levels.lua python3 tools/kontakt-oracle/dump.py $OUT/levels.jsonl
python3 tools/kontakt-oracle/dump.py $OUT/levels_native.jsonl $OUT/levels.nki
python3 tools/kontakt-oracle/diff.py $OUT/levels_native.jsonl 2>&1 | head -20
