#!/usr/bin/env bash
# ours.sh STANDALONE OUT_DIR PAGE... -- KONTRA's side of the probe suite: loads each
# lib/Instruments/PAGE.nki (built by shoot.sh) in the real standalone window under Xvfb, shoots it
# to OUT_DIR/o-PAGE.png, drives it (drive.py), shoots o-PAGE-driven.png and copies the page's trace to OUT_DIR/o-PAGE.nka. Diff against Kontakt's
# with `trace.py`.
set -u
bin=$1 out=$2; shift 2
here=$(cd "$(dirname "$0")" && pwd)
# A script's saves land under the user's array folder at the path it named (rack/files.rs).
saves=${XDG_DATA_HOME:-$HOME/.local/share}/kontra/arrays/Z:/data/dev/workspace/scratch/ksp-ui-trace
d=${KSP_UI_DISPLAY:-:94}
mkdir -p "$out"
Xvfb "$d" -screen 0 1920x1280x24 -nolisten tcp >/dev/null 2>&1 & xv=$!
trap 'kill $xv 2>/dev/null' EXIT
sleep 1
for page in "$@"; do
  python3 "$here/state.py" "$out/$page.state" "$here/lib/Instruments/$page.nki"
  rm -f "$saves/$page.nka"
  env -u WAYLAND_DISPLAY -u XDG_SESSION_TYPE DISPLAY=$d MUI_RENDERER=cpu nice -n 19 \
    "$bin" --state "$out/$page.state" --output Discard >"$out/o-$page.log" 2>&1 & ed=$!
  sleep "${WAIT:-20}"
  DISPLAY=$d xdotool mousemove 1900 1270; sleep 1
  DISPLAY=$d import -window root "$out/o-$page.png"
  [ -n "${NODRIVE:-}" ] || { DISPLAY=$d python3 "$here/drive.py" "$here/probes/$page.drive" "$out/o-$page.png"
    DISPLAY=$d xdotool mousemove 1900 1270; sleep 4; DISPLAY=$d import -window root "$out/o-$page-driven.png"; }
  kill $ed; wait $ed 2>/dev/null
  cp "$saves/$page.nka" "$out/o-$page.nka" 2>/dev/null || echo "$page: no trace" >&2
done
