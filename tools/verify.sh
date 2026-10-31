#!/usr/bin/env bash
# Gate a build on the self-test loop before any binary is handed over.
#
#   tools/verify.sh            run every check, print a table, exit non-zero on failure
#   tools/verify.sh --quick    skip the audio render (fast; still runs the unit layers)
#
# What it refuses to let through:
#   - a failing test in the crates the change touched;
#   - a plugin build with no CLAP entry (a host would not see it);
#   - Electric Mint asking for more script calls than the agreed ceiling, or a
#     load that produced no answer at all;
#   - a render that does not complete, drops sample frames, or clips;
#   - a UI whose controls are not where they are drawn (layer 2), including a
#     ui-drive that does not build and a page that drew no control to check;
#   - a page that no longer looks like the reviewed one (layer 3: a golden
#     regression check, not a live Kontakt comparison -- it catches a page that
#     changes, not a page that was wrong to begin with). A live comparison is
#     now possible: tools/oracle-mcp renders and screenshots real Kontakt on a
#     private Xvfb with DXVK + lavapipe (docs/re/kontakt-headless-gui.md), so
#     `tools/oracle-mcp/parity.py` is the on-demand check and this gate stays
#     offline and fast. A geometry check cannot
#     see a picture that resolved to nothing -- it still has a rect -- which is
#     how a missing graphic shipped once. If no reference capture exists the
#     gate fails: ALLOW_NO_PIXEL_REF=1 is a logged admission that layer 3 is off.
set -uo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
# /tmp is a 16 GB tmpfs on this machine; a release build of the workspace does
# not fit beside three lanes. Keep the target on real disk.
TARGET=${CARGO_TARGET_DIR:-/var/tmp/tgt-int}
# $HOME rather than an absolute path: the gate is run from a checkout, and a
# committed home directory names whoever ran it last.
EMINT=${EMINT:-$HOME/Armory/VSTs/Kontakt1/Native Instruments Session Guitarist Electric Mint 1.1.0/Session Guitarist - Electric Mint Library/Instruments/Electric Mint.nki}
# Ceilings, not goals: they must only ever come down.
MAX_UNHANDLED=${MAX_UNHANDLED:-55}
MAX_INIT_UNANSWERED=${MAX_INIT_UNANSWERED:-51}
# Percent of the page allowed to differ from Kontakt's own render.
MAX_PIXEL_PCT=${MAX_PIXEL_PCT:-0.5}
QUICK=${1:-}

cd "$REPO"
# One private directory holds every log, JSON and PNG this gate writes, made
# before the first redirect. A fixed /tmp path is a symlink a local user can
# pre-create (CWE-377), and on a machine without that directory the redirect
# fails and the step "fails" without ever running. Kept after the run so a
# failure message can name a file that still exists.
LOGS=${KONTRA_VERIFY_LOGS:-$(mktemp -d "${TMPDIR:-/tmp}/kontra-verify.XXXXXX")} \
    || { echo "verify: cannot create a log directory (mktemp failed)" >&2; exit 2; }
mkdir -p "$LOGS"
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
    # A crate that fails to COMPILE prints no result line at all, so "no
    # result" is a failure, not a pass -- that is how a broken test file once
    # reported ok.
    if printf '%s\n' "$out" | grep -qE "^test result: FAILED"; then
        bad "tests: $crate" "${line:-some tests failed}"
    elif ! printf '%s\n' "$out" | grep -qE "^test result:"; then
        bad "tests: $crate" "did not compile or did not run"
    else
        ok "tests: $crate" "${line:-no result line}"
    fi
}

echo "== layer 0/1: tests =="
# kontra-plugin and kontra-ui-pictures are here because the plugin is what
# ships: a page that renders in the harness and not in the host is exactly how
# the wallpaper went missing (a 7952-row picture became 15904 at the live 2x
# device scale, past the renderer's 8192-pixel atlas, and was dropped silently).
for c in kontra-engine kontra-host kontra-ui mui kontra-native-host kontra-plugin kontra-ui-pictures; do run_tests "$c"; done
if [ -d crates/dsp/delay ]; then run_tests kontra-dsp-delay; fi

echo "== build =="
if CARGO_TARGET_DIR="$TARGET" cargo build --release -p kontra-plugin >"$LOGS/verify-build.log" 2>&1; then
    ok "build kontra-plugin" "$(stat -c%s "$TARGET/release/libkontra_plugin.so" 2>/dev/null || echo '?') bytes"
else
    bad "build kontra-plugin" "see $LOGS/verify-build.log"
fi
if nm -D --defined-only "$TARGET/release/libkontra_plugin.so" 2>/dev/null | grep -q " clap_entry$"; then
    ok "exports clap_entry" "a CLAP host will see it"
else
    bad "exports clap_entry" "the .so has no CLAP entry"
fi

echo "== layer 0: Electric Mint =="
if CARGO_TARGET_DIR="$TARGET" cargo build --release -p kontra-inspect >"$LOGS/verify-inspect.log" 2>&1; then
    INSPECT="$TARGET/release/kontra-inspect"
    LOADOUT=$("$INSPECT" load "$EMINT" 2>&1)
    LOADSTATUS=$?
    printf '%s\n' "$LOADOUT" >"$LOGS/verify-load.txt"
    LOAD=$(printf '%s\n' "$LOADOUT" | grep -o "init unanswered: \[[^]]*\]" | head -1)
    # A sum over nothing is 0, which used to read as "nothing unanswered" when
    # the load had failed, the instrument was missing, or the output changed
    # shape. An empty sum is now distinguishable from a measured zero -- and a
    # present, well-formed empty list (`init unanswered: []`, which is what a
    # load with nothing unanswered prints) IS that measured zero.
    INIT=$(printf '%s' "$LOAD" | python3 -c '
import re, sys
text = sys.stdin.read().strip()
m = re.fullmatch(r"init unanswered: \[(.*)\]", text)
if not m:
    print("")
else:
    body = m.group(1).strip()
    if not body:
        print(0)
    else:
        n = re.findall(r", (\d+)\)", body)
        print(sum(int(i) for i in n) if n else "")
')
    if [ "$LOADSTATUS" -ne 0 ]; then
        bad "init unanswered" "kontra-inspect load exited $LOADSTATUS; see $LOGS/verify-load.txt"
    elif [ -z "$LOAD" ] || [ -z "$INIT" ]; then
        bad "init unanswered" "load printed no 'init unanswered:' counts; see $LOGS/verify-load.txt"
    elif [ "$INIT" -le "$MAX_INIT_UNANSWERED" ]; then
        ok "init unanswered" "$INIT (ceiling $MAX_INIT_UNANSWERED)"
    else
        bad "init unanswered" "$INIT above ceiling $MAX_INIT_UNANSWERED"
    fi

    if [ "$QUICK" != "--quick" ]; then
        "$INSPECT" play "$EMINT" --json "$LOGS/verify-play.json" >"$LOGS/verify-play.txt" 2>&1
        PLAYSTATUS=$?
        if [ "$PLAYSTATUS" -ne 0 ] || [ ! -f "$LOGS/verify-play.json" ]; then
            bad "render completes" "kontra-inspect play exited $PLAYSTATUS; see $LOGS/verify-play.txt"
        else
            read -r DONE UNH FNV FRAMES CLIPPED < <(python3 - "$LOGS/verify-play.json" "$LOGS/verify-play.txt" <<'PY'
import json,re,sys
d=json.load(open(sys.argv[1]))['result']
m=re.search(r'actions_unhandled: (\d+)', d['engine_stats'])
txt=open(sys.argv[2]).read()
fr=re.search(r'missing sample frames: (\d+)', txt)
cl=re.search(r'clipped samples (\d+)', txt)
print(d['render_complete'], m.group(1) if m else '?', d['audio_fnv1a64'],
      fr.group(1) if fr else '?', cl.group(1) if cl else '?')
PY
)
            [ "$DONE" = "True" ] && ok "render completes" "yes" || bad "render completes" "no"
            [ "${UNH:-999999}" -le "$MAX_UNHANDLED" ] 2>/dev/null && ok "actions_unhandled" "$UNH (ceiling $MAX_UNHANDLED)" || bad "actions_unhandled" "$UNH above ceiling"
            [ "${FRAMES:-1}" = "0" ] && ok "missing sample frames" "0" || bad "missing sample frames" "$FRAMES"
            [ "${CLIPPED:-1}" = "0" ] && ok "clipped samples" "0" || bad "clipped samples" "$CLIPPED"
            ok "audio fingerprint" "$FNV"
        fi
    fi
else
    bad "build kontra-inspect" "see $LOGS/verify-inspect.log"
fi

echo "== layer 2: the UI drives where it draws =="
if CARGO_TARGET_DIR="$TARGET" cargo build --release -p ui-drive >"$LOGS/verify-drive.log" 2>&1; then
    if "$TARGET/release/ui-drive" "$EMINT" >"$LOGS/verify-drive.txt" 2>&1; then
        ok "ui-drive" "$(tail -1 "$LOGS/verify-drive.txt" | cut -c1-60)"
    else
        bad "ui-drive" "see $LOGS/verify-drive.txt"
    fi
else
    # ui-drive is a workspace member: a build failure is a defect, not "not
    # built yet". Recorded as skipped, a compile break silently switched off
    # layer 2 while the gate still printed PASSED.
    bad "ui-drive" "build failed; see $LOGS/verify-drive.log"
fi

echo "== layer 3: the page still looks like the reviewed capture =="
REF=${PIXEL_REF:-$(ls "$REPO"/tools/kontakt-reference/*-page.png 2>/dev/null | head -1)}
if [ -n "$REF" ] && [ -f "$REF" ]; then
    SHOTS=$LOGS/verify-shots
    rm -rf "$SHOTS"; mkdir -p "$SHOTS"
    if CARGO_TARGET_DIR="$TARGET" cargo run --release -q -p ui-shots -- "$SHOTS" "$EMINT" >"$LOGS/verify-shots.log" 2>&1; then
        SHOT=$(ls "$SHOTS"/*.png 2>/dev/null | head -1)
        if [ -n "$SHOT" ]; then
            OUT=$(python3 "$REPO/tools/pixel-diff.py" "$REF" "$SHOT" --scale --max-pct "$MAX_PIXEL_PCT" --out "$LOGS/verify-pixel" 2>&1)
            STATUS=$?
            if [ "$STATUS" -eq 0 ]; then
                ok "pixel diff vs golden" "$(printf '%s\n' "$OUT" | grep -o 'differing.*%' | head -1)"
            elif [ "$STATUS" -eq 2 ]; then
                # "cannot be compared" (missing, unreadable, or different-sized
                # without --scale) is neither a threshold failure nor a pass.
                bad "pixel diff vs golden" "$(printf '%s\n' "$OUT" | head -1)"
            else
                REASON=$(printf '%s\n' "$OUT" | grep -oE '[0-9.]+% differs, ceiling [0-9.]+%' | head -1)
                bad "pixel diff vs golden" "${REASON:-pixel-diff exited $STATUS with no summary}"
                printf '%s\n' "$OUT" | grep -E '^ +[0-9]+ px' | head -5 | sed 's/^/      /'
            fi
        else
            bad "pixel diff vs golden" "ui-shots produced no png"
        fi
    else
        bad "pixel diff vs golden" "see $LOGS/verify-shots.log"
    fi
elif [ "${ALLOW_NO_PIXEL_REF:-0}" = "1" ]; then
    row "pixel diff vs golden" "OFF (ALLOW_NO_PIXEL_REF=1)"
else
    bad "pixel diff vs golden" "no golden in tools/kontakt-reference/"
fi

echo
printf '%s\n' "${ROWS[@]}"
echo
echo "logs: $LOGS"
if [ "$FAILED" -eq 0 ]; then
    # The play dump is a few hundred MB of per-voice detail that nothing reads
    # after the rows above, and /tmp is a 16 GB tmpfs here. Keep it (and the
    # rest) only when something failed and it is worth reading.
    rm -f "$LOGS/verify-play.json"
fi
if [ "$FAILED" -ne 0 ]; then
    echo "VERIFY FAILED -- do not deploy"
    exit 1
fi
echo "VERIFY PASSED -- safe to deploy with tools/deploy.sh"
