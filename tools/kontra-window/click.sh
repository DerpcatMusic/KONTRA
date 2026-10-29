#!/usr/bin/env bash
# A click on a library control shows by itself: the pointer rests on the
# control, one click, then no input at all; the control must look
# different. A frame that drew the click's effect only on the next input
# (lane B: Areia's Classic Mix) fails here.
#
#   tools/kontra-window/click.sh <kontra-window binary> <instrument> <x,y> <out dir>
#
# x,y is window pixels at the default 1180x780 and device scale 1.5; the
# default is Areia's Classic Mix switch. XDISPLAY picks the display (:97).
set -euo pipefail
bin=$1 instrument=$2 at=${3:-556,524} out=$4
mkdir -p "$out"
display=${XDISPLAY:-:97}
for _ in $(seq 1 25); do [[ -e /tmp/.X${display#:}-lock ]] || break; sleep 0.2; done
if [[ -e /tmp/.X${display#:}-lock ]]; then
    echo "FAIL display $display is taken (another harness?); set XDISPLAY"; exit 1
fi
Xvfb $display -screen 0 1920x1280x24 -nolisten tcp >"$out/xvfb.log" 2>&1 &
xvfb=$!
sleep 1
kill -0 $xvfb 2>/dev/null || { echo "FAIL Xvfb did not start"; exit 1; }
"$bin" state "$instrument" "$out/click.state"
DISPLAY=$display WAYLAND_DISPLAY='' MUI_RENDERER=cpu nice -n 19 \
    "$bin" --state "$out/click.state" --output Discard >"$out/click.log" 2>&1 &
editor=$!
trap 'kill $editor $xvfb 2>/dev/null || true' EXIT
export DISPLAY=$display
sleep 40
x=${at%,*} y=${at#*,}
xdotool mousemove "$x" "$y"; sleep 2
import -window root -resize 50% "$out/c-hover.png"
xdotool click 1; sleep 2
import -window root -resize 50% "$out/c-clicked.png"
box="$(( x / 2 - 70 )),$(( y / 2 - 14 )),$(( x / 2 + 70 )),$(( y / 2 + 14 ))"
if python3 -I -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB") for p in sys.argv[1:3])
box = tuple(map(int, sys.argv[3].split(",")))
sys.exit(0 if ImageChops.difference(a, b).crop(box).getbbox() else 1)' \
    "$out/c-hover.png" "$out/c-clicked.png" "$box"; then
    echo "PASS a click shows without a second input ($(basename "$instrument" .nki))"
else
    echo "FAIL a click shows without a second input ($(basename "$instrument" .nki))"; exit 1
fi
