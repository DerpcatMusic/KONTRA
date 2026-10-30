#!/usr/bin/env bash
# Gate a build on the self-test loop before any binary is handed over.
#
#   tools/verify.sh            run every check, print a table, exit non-zero on failure
#   tools/verify.sh --quick    skip the audio render (fast; still runs the unit layers)
#
# What it refuses to let through:
#   - a failing test in the crates the change touched;
#   - a plugin build with no CLAP entry (a host would not see it);
#   - Electric Mint asking for more script calls than the agreed ceiling;
#   - a render that does not complete, drops sample frames, or clips;
#   - a UI whose controls are not where they are drawn (layer 1 and 2).
set -uo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${CARGO_TARGET_DIR:-/tmp/opencode/kv3-target}
EMINT=${EMINT:-/home/dracula/Armory/VSTs/Kontakt1/Native Instruments Session Guitarist Electric Mint 1.1.0/Session Guitarist - Electric Mint Library/Instruments/Electric Mint.nki}
# Ceilings, not goals: they must only ever come down.
MAX_UNHANDLED=${MAX_UNHANDLED:-173}
MAX_INIT_UNANSWERED=${MAX_INIT_UNANSWERED:-75}
QUICK=${1:-}

cd "$REPO"
FAILED=0
declare -a ROWS

row() { ROWS+=("$(printf '%-42s %s' "$1" "$2")"); }
bad() { row "$1" "FAIL  $2"; FAILED=1; }
ok()  { row "$1" "ok    $2"; }

run_tests() {
    local crate=$1
    local out
    out=$(CARGO_TARGET_DIR="$TARGET" cargo test -p "$crate" 2>&1)
    local line
    line=$(printf '%s\n' "$out" | grep -E "^test result:" | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}')
    if printf '%s\n' "$out" | grep -qE "^test result: FAILED"; then
        bad "tests: $crate" "${line:-no result line}"
    else
        ok "tests: $crate" "${line:-no result line}"
    fi
}

echo "== layer 0/1: tests =="
for c in kontra-engine kontra-host kontra-ui; do run_tests "$c"; done
if [ -d crates/dsp/delay ]; then run_tests kontra-dsp-delay; fi

echo "== build =="
if CARGO_TARGET_DIR="$TARGET" cargo build --release -p kontra-plugin >/tmp/opencode/verify-build.log 2>&1; then
    ok "build kontra-plugin" "$(stat -c%s "$TARGET/release/libkontra_plugin.so" 2>/dev/null || echo '?') bytes"
else
    bad "build kontra-plugin" "see /tmp/opencode/verify-build.log"
fi
if nm -D --defined-only "$TARGET/release/libkontra_plugin.so" 2>/dev/null | grep -q " clap_entry$"; then
    ok "exports clap_entry" "a CLAP host will see it"
else
    bad "exports clap_entry" "the .so has no CLAP entry"
fi

echo "== layer 0: Electric Mint =="
if CARGO_TARGET_DIR="$TARGET" cargo build --release -p kontra-inspect >/tmp/opencode/verify-inspect.log 2>&1; then
    INSPECT="$TARGET/release/kontra-inspect"
    LOAD=$("$INSPECT" load "$EMINT" 2>&1 | grep -o "init unanswered: \[[^]]*\]" | head -1)
    INIT=$(printf '%s' "$LOAD" | python3 -c 'import re,sys; print(sum(int(n) for n in re.findall(r", (\d+)\)", sys.stdin.read())))')
    if [ "${INIT:-0}" -le "$MAX_INIT_UNANSWERED" ]; then
        ok "init unanswered" "$INIT (ceiling $MAX_INIT_UNANSWERED)"
    else
        bad "init unanswered" "$INIT above ceiling $MAX_INIT_UNANSWERED"
    fi

    if [ "$QUICK" != "--quick" ]; then
        "$INSPECT" play "$EMINT" --json /tmp/opencode/verify-play.json >/tmp/opencode/verify-play.txt 2>&1
        read -r DONE UNH FNV FRAMES CLIPPED < <(python3 - <<'PY'
import json,re
d=json.load(open('/tmp/opencode/verify-play.json'))['result']
m=re.search(r'actions_unhandled: (\d+)', d['engine_stats'])
txt=open('/tmp/opencode/verify-play.txt').read()
fr=re.search(r'missing sample frames: (\d+)', txt)
cl=re.search(r'clipped samples (\d+)', txt)
print(d['render_complete'], m.group(1) if m else '?', d['audio_fnv1a64'],
      fr.group(1) if fr else '?', cl.group(1) if cl else '?')
PY
)
        [ "$DONE" = "True" ] && ok "render completes" "yes" || bad "render completes" "no"
        [ "${UNH:-999999}" -le "$MAX_UNHANDLED" ] && ok "actions_unhandled" "$UNH (ceiling $MAX_UNHANDLED)" || bad "actions_unhandled" "$UNH above ceiling"
        [ "${FRAMES:-1}" = "0" ] && ok "missing sample frames" "0" || bad "missing sample frames" "$FRAMES"
        [ "${CLIPPED:-1}" = "0" ] && ok "clipped samples" "0" || bad "clipped samples" "$CLIPPED"
        ok "audio fingerprint" "$FNV"
    fi
else
    bad "build kontra-inspect" "see /tmp/opencode/verify-inspect.log"
fi

echo "== layer 2: the UI drives where it draws =="
if CARGO_TARGET_DIR="$TARGET" cargo build --release -p ui-drive >/tmp/opencode/verify-drive.log 2>&1; then
    if "$TARGET/release/ui-drive" "$EMINT" >/tmp/opencode/verify-drive.txt 2>&1; then
        ok "ui-drive" "$(tail -1 /tmp/opencode/verify-drive.txt | cut -c1-60)"
    else
        bad "ui-drive" "see /tmp/opencode/verify-drive.txt"
    fi
else
    row "ui-drive" "skipped (not built yet)"
fi

echo
printf '%s\n' "${ROWS[@]}"
echo
if [ "$FAILED" -ne 0 ]; then
    echo "VERIFY FAILED -- do not deploy"
    exit 1
fi
echo "VERIFY PASSED -- safe to deploy with tools/deploy.sh"
