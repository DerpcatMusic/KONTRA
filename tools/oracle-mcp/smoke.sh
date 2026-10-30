#!/usr/bin/env bash
# smoke.sh - the oracle MCP end to end, from a clean state, printing every measured value.
#
#   up -> load Electric Mint -> shot -> click a known control -> render 2 s of a MIDI
#   -> click outside the window (must report 0 changed) -> down
#
# and, before `up`, `oracle_shot` must fail with a clear error (a tool without a
# session never invents a measurement).
#
# Usage: tools/oracle-mcp/smoke.sh
# Exit status 0 only when every step produced the expected outcome.
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
WORK=${ORACLE_MCP_SMOKE_DIR:-/tmp/opencode/oracle-mcp/smoke-$(date +%Y%m%d-%H%M%S)}
NKI="${ORACLE_MCP_NKI:-/home/dracula/Armory/VSTs/Kontakt1/Native Instruments Session Guitarist Electric Mint 1.1.0/Session Guitarist - Electric Mint Library/Instruments/Electric Mint.nki}"
mkdir -p "$WORK" || { echo "cannot create $WORK"; exit 3; }
STEPS="$WORK/steps.jsonl"
MIDI="$WORK/smoke.mid"

echo "== oracle-mcp smoke"
echo "   work dir : $WORK"
echo "   instrument: $NKI"
echo "   display  : (chosen by the server: :77, else :94..:97 - never :0)"

[ -f "$NKI" ] || { echo "FAIL: instrument not found: $NKI"; exit 3; }

# a 3-note, 2 s MIDI, written with the server's own writer (same code the tool uses)
python3 - "$MIDI" "$HERE/server.py" <<'PY' || exit 3
import importlib.util, sys
from pathlib import Path
spec = importlib.util.spec_from_file_location("oracle_mcp_server", sys.argv[2])
om = importlib.util.module_from_spec(spec)
spec.loader.exec_module(om)
meta = om.write_midi(Path(sys.argv[1]), [
    {"start_s": 0.10, "length_s": 0.80, "key": 60, "velocity": 100},
    {"start_s": 0.90, "length_s": 0.70, "key": 64, "velocity": 90},
    {"start_s": 1.60, "length_s": 0.40, "key": 67, "velocity": 110},
], bpm=120)
print(f"   midi     : {meta['path']} {meta['notes']} notes, {meta['duration_s']:.2f}s")
PY

cat > "$STEPS" <<JSON
{"tool":"oracle_shot","arguments":{},"expect":"error","comment":"no session yet: must fail, not invent a shot"}
{"tool":"oracle_up","arguments":{},"comment":"Xvfb + openbox + Kontakt (DXVK/lavapipe)","checks":{"state":"up","page.mode":"browser","pids.kontakt":{"min":1}}}
{"tool":"oracle_load","arguments":{"nki":"$NKI"},"comment":"Electric Mint","checks":{"page.instrument_page":true}}
{"tool":"oracle_shot","arguments":{},"comment":"client crop + texture stats","checks":{"mode":"instrument"}}
{"tool":"oracle_click","arguments":{"x":722,"y":570},"comment":"the Amps & FX tab (measured tab centre at this window size)","checks":{"changed":{"min":1000}}}
{"tool":"oracle_click","arguments":{"x":5000,"y":5000},"expect":"error","comment":"outside the window: must report changed=0, not success"}
{"tool":"oracle_render","arguments":{"midi":"$MIDI","seconds":2},"comment":"2 s of the MIDI through the null sink","checks":{"frames":{"min":48000},"peak":{"min":0.0001}}}
{"tool":"oracle_down","arguments":{},"comment":"stop only what this server started","checks":{"state":"down"}}
JSON

set -o pipefail
python3 "$HERE/client.py" --script "$STEPS" 2>&1 | tee "$WORK/smoke.log"
rc=${PIPESTATUS[0]}

echo
echo "== artifacts in $WORK"
ls -1 "$WORK" 2>/dev/null
echo "== server log: $(python3 -c 'import os;print(os.environ.get("ORACLE_MCP_LOG") or "/tmp/opencode/oracle-mcp/server.log")')"
echo "== smoke exit $rc"
exit "$rc"
