#!/usr/bin/env bash
# Build KONTRA and put it where hosts actually load it.
#
# REAPER loads the CLAP. Installing only the VST3 means the host keeps running
# the previous build and every test is of the wrong binary -- which is exactly
# what happened on 2026-10-30. Both are installed here, always.
#
#   tools/deploy.sh            build, install, and say what changed
#   tools/deploy.sh --reaper   also restart REAPER and place it on the KRDP monitor
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${CARGO_TARGET_DIR:-/tmp/opencode/kv3-target}
CLAP="$HOME/.clap/KONTRA.clap"
VST3="$HOME/.vst3/KONTRA.vst3/Contents/x86_64-linux/KONTRA.so"

cd "$REPO"
echo "building kontra-plugin (release)"
CARGO_TARGET_DIR="$TARGET" cargo build --release -p kontra-plugin

BUILT="$TARGET/release/libkontra_plugin.so"
[ -f "$BUILT" ] || { echo "no $BUILT" >&2; exit 1; }
STAMP=$(date +%H:%M:%S)

install -Dm644 "$BUILT" "$CLAP"
echo "  $CLAP  $(stat -c%s "$CLAP") bytes  $STAMP"
if [ -d "$(dirname "$VST3")" ]; then
    install -Dm644 "$BUILT" "$VST3"
    echo "  $VST3  $(stat -c%s "$VST3") bytes  $STAMP"
fi

# Both files must be the same build, or the host and the next test disagree.
if [ -f "$VST3" ] && ! cmp -s "$CLAP" "$VST3"; then
    echo "CLAP and VST3 differ -- one host will run a stale build" >&2
    exit 1
fi

if [ "${1:-}" = "--reaper" ]; then
    export XAUTHORITY=${XAUTHORITY:-/run/user/1000/xauth_FXUsrQ} DISPLAY=${DISPLAY:-:0}
    for p in $(pgrep -x reaper); do kill "$p" 2>/dev/null || true; done
    sleep 3
    setsid nohup "$HOME/ProgramFiles/REAPER/reaper" -new >"$HOME/.cache/kontra/launch/reaper.log" 2>&1 </dev/null &
    sleep 16
    ID=$(wmctrl -lG 2>/dev/null | grep -i "REAPER v" | head -1 | awk '{print $1}')
    if [ -n "$ID" ]; then
        # KWin doubles a requested position; the KRDP monitor starts at physical x 10881.
        wmctrl -i -r "$ID" -b remove,maximized_vert,maximized_horz 2>/dev/null || true
        wmctrl -i -r "$ID" -e 0,5440,0,2880,1536
        sleep 2
        wmctrl -lG | grep -i "$ID" | awk '{printf "  REAPER at x=%s y=%s %sx%s\n",$3,$4,$5,$6}'
        wmctrl -i -a "$ID" 2>/dev/null || true
    else
        echo "  REAPER did not open a window" >&2
    fi
fi
