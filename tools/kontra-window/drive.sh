#!/usr/bin/env bash
# The plugin's real editor under Xvfb, driven with real pointer events:
# a part tab, a library knob dragged, a library switch clicked, then idle.
# Each step must change the window; idle must cost ~0 CPU.
#
#   tools/kontra-window/drive.sh <kontra-window binary> <instrument> <out dir> \
#       <knob x,y> <switch x,y>
#
# Points are in window pixels after the rack is scrolled down by five wheel
# steps. Una Corda Cotton at the default 1180x780, device scale 1.5:
#   drive.sh "$BIN" ".../Una Corda Cotton.nki" out 634,864 1484,932
set -euo pipefail
bin=$1 instrument=$2 out=$3 knob=$4 switch=$5
mkdir -p "$out"
display=:97
Xvfb $display -screen 0 1920x1280x24 -nolisten tcp >"$out/xvfb.log" 2>&1 &
xvfb=$!
"$bin" state "$instrument" "$out/part.state"
# The ALSA null device: an audio callback (the engine runs in it), no sound.
DISPLAY=$display WAYLAND_DISPLAY='' MUI_RENDERER=cpu nice -n 19 \
    "$bin" --state "$out/part.state" --output Discard >"$out/run.log" 2>&1 &
editor=$!
trap 'kill $editor $xvfb 2>/dev/null || true' EXIT
export DISPLAY=$display
sleep 25

shot() { import -window root -resize 50% "$out/$1.png"; }
same() { python3 -I -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB") for p in sys.argv[1:3])
sys.exit(0 if ImageChops.difference(a, b).getbbox() is None else 1)' "$out/$1.png" "$out/$2.png"; }
fail=0
check() { if same "$1" "$2"; then echo "FAIL $3"; fail=1; else echo "PASS $3"; fi; }

shot 0-open
xdotool mousemove 563 160 click 1; sleep 1.5; shot 1-mapping
check 0-open 1-mapping "part tab: Mapping opens"
xdotool mousemove 458 160 click 1; sleep 1.5
xdotool mousemove 1000 600; for _ in 1 2 3 4 5; do xdotool click 5; sleep 0.1; done
sleep 1.5; shot 2-face
xdotool mousemove "${knob%,*}" "${knob#*,}"; sleep 0.3; xdotool mousedown 1; sleep 0.2
for _ in $(seq 1 20); do xdotool mousemove_relative -- 0 -5; sleep 0.03; done
xdotool mouseup 1; sleep 1.5; shot 3-knob
check 2-face 3-knob "library knob: drag turns it"
xdotool mousemove "${switch%,*}" "${switch#*,}" click 1; sleep 1.5; shot 4-switch
check 3-knob 4-switch "library switch: click flips it"

# Idle: the pointer parked, nothing playing. CPU over 10 s, in % of a core,
# of every thread but the audio callback's: the null device has no clock, so
# cpal's thread spins there, where a real device would pace it.
ticks() {
    local sum=0 t
    for t in /proc/"$editor"/task/*; do
        [[ $(<"$t/comm") == cpal* ]] && continue
        sum=$(( sum + $(awk '{print $14 + $15}' "$t/stat") ))
    done
    echo "$sum"
}
xdotool mousemove 1900 1270; sleep 2
t0=$(ticks); sleep 10
t1=$(ticks)
idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / 10 ))
echo "idle CPU: ${idle}% of a core"
if (( idle > 5 )); then echo "FAIL idle CPU"; fail=1; else echo "PASS idle CPU"; fi
exit $fail
