#!/usr/bin/env bash
# The plugin's real editor under Xvfb, driven with real pointer events:
# the face drawn untouched, a part tab, the header's library art, the browser's library cards, the preset field, a library knob
# dragged and a switch clicked, a browser row clicked (selects), double-
# clicked (adds a part) and dragged onto a part (replaces it), the
# browser's edge dragged, then idle
# for a minute.
# Each step must change the window as it says; idle must cost ~0 CPU.
# click.sh beside it clicks one library control (Areia's Classic Mix) and
# wants it drawn with no further input.
#
#   tools/kontra-window/drive.sh <kontra-window binary> <instrument> <out dir>
#
# The points below are window pixels for Una Corda Cotton at the default
# 1180x780 and device scale 1.5, with the browser on Una Corda's three
# presets. Another instrument overrides them in the environment.
set -euo pipefail
bin=$1 instrument=$2 out=$3
tab=${TAB:-563,160}            # part tab Mapping
interface=${INTERFACE:-458,160}
preset=${PRESET:-874,114}      # the header's preset field
knob=${KNOB:-632,804}          # after five wheel steps down
switch=${SWITCH:-1484,872}
search=${SEARCH:-100,80}       # the browser's search field
name=${NAME:-Una Corda}        # typed there, so its card comes first
library=${LIBRARY:-200,260}    # that card
second=${SECOND:-90,811}       # Una Corda Felt
third=${THIRD:-94,847}         # Una Corda Pure
edge=${EDGE:-200,578}          # the edge between libraries and presets
header=${HEADER:-760,114}      # part 0's header, to drop on
fold=${FOLD:-426,114}          # part 0's fold, so a new part shows below it
mkdir -p "$out"
display=${XDISPLAY:-:97}
# A server still shutting down holds the display for a moment; one still
# there after that is another harness's: refuse rather than share it.
for _ in $(seq 1 25); do [[ -e /tmp/.X${display#:}-lock ]] || break; sleep 0.2; done
if [[ -e /tmp/.X${display#:}-lock ]]; then
    echo "FAIL display $display is taken (another harness?); set XDISPLAY"; exit 1
fi
Xvfb $display -screen 0 1920x1280x24 -nolisten tcp >"$out/xvfb.log" 2>&1 &
xvfb=$!
sleep 1
kill -0 $xvfb 2>/dev/null || { echo "FAIL Xvfb did not start: $(tail -3 "$out/xvfb.log")"; exit 1; }
"$bin" state "$instrument" "$out/part.state"
# The ALSA null device: an audio callback (the engine runs in it), no sound.
DISPLAY=$display WAYLAND_DISPLAY='' MUI_RENDERER=cpu nice -n 19 \
    "$bin" --state "$out/part.state" --output Discard >"$out/run.log" 2>&1 &
editor=$!
trap 'kill $editor $xvfb 2>/dev/null || true' EXIT
export DISPLAY=$display
sleep 25

at() { xdotool mousemove "${1%,*}" "${1#*,}"; }
shot() { import -window root -resize 50% "$out/$1.png"; }
# Whether shots $1 and $2 differ inside the half-size box $3 (x0,y0,x1,y1).
differ() { python3 -I -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB") for p in sys.argv[1:3])
box = tuple(map(int, sys.argv[3].split(","))) if len(sys.argv) > 3 else None
d = ImageChops.difference(a, b)
sys.exit(0 if (d.crop(box) if box else d).getbbox() else 1)' "$out/$1.png" "$out/$2.png" ${3:-}; }
fail=0
pass() { echo "PASS $1"; }
miss() { echo "FAIL $1"; fail=1; }
check() { if differ "$1" "$2" "${4:-}"; then pass "$3"; else miss "$3"; fi; }
still() { if differ "$1" "$2" "${4:-}"; then miss "$3"; else pass "$3"; fi; }

shot 0-open
# Nothing has been clicked yet: the face draws on its own once its
# pictures decode, not on the first input.
if python3 -I -c 'import sys; from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB").crop((210, 100, 875, 495))
sys.exit(0 if len(set(im.getdata())) > 64 else 1)' "$out/0-open.png"; then
    pass "face draws without any click"
else miss "face draws without any click"; fi
# The header's left end shows the library's art: many colours, not one fill.
if python3 -I -c 'import sys; from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB").crop((205, 46, 420, 92))
sys.exit(0 if len(set(im.getdata())) > 64 else 1)' "$out/0-open.png"; then
    pass "header: the library's art behind both rows"
else miss "header: the library's art behind both rows"; fi
# The browser's libraries are cards on their covers, as v2's were.
if python3 -I -c 'import sys; from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB").crop((5, 120, 192, 415))
sys.exit(0 if len(set(im.getdata())) > 256 else 1)' "$out/0-open.png"; then
    pass "browser: library cards on their covers"
else miss "browser: library cards on their covers"; fi
at "$tab"; xdotool click 1; sleep 1.5; shot 1-mapping
check 0-open 1-mapping "part tab: Mapping opens"
at "$interface"; xdotool click 1; sleep 1.5; shot 2-interface
at "$preset"; xdotool click 1; sleep 1.5; shot 3-presets
check 2-interface 3-presets "preset field: its menu opens"
xdotool key Escape; at "$preset"; xdotool click 1; sleep 1.5

at 1000,600; for _ in 1 2 3 4 5; do xdotool click 5; sleep 0.1; done
sleep 1.5; shot 4-face
at "$knob"; sleep 0.3; xdotool mousedown 1; sleep 0.2
for _ in $(seq 1 20); do xdotool mousemove_relative -- 0 -5; sleep 0.03; done
xdotool mouseup 1; sleep 1.5; shot 5-knob
check 4-face 5-knob "library knob: drag turns it"
at "$switch"; xdotool click 1; sleep 1.5; shot 6-switch
check 5-knob 6-switch "library switch: click flips it"
at 1000,600; for _ in $(seq 1 10); do xdotool click 4; sleep 0.1; done; sleep 1.5

at "$fold"; xdotool click 1; sleep 1.5
at "$search"; xdotool click 1; xdotool type --delay 60 "$name"; sleep 1.5
# The pointer rests on the card, then a click and no more input: what the
# click opened must show by itself, not on the next move.
at "$library"; sleep 1.5; shot 7a-hover
xdotool click 1; sleep 1.5; shot 7b-clicked
check 7a-hover 7b-clicked "a click shows without a second input" 0,110,198,500
at 1000,600; sleep 1.5; shot 7-library
at "$second"; xdotool click 1; sleep 3; shot 8-selected
# A click selects the row; the rack (right of the browser) stays as it was.
still 7-library 8-selected "browser: a click only selects" 205,30,880,500
at "$second"; xdotool click --repeat 2 --delay 80 1; sleep 0.4; shot 9-loading
sleep 8; shot 10-added
check 8-selected 10-added "browser: a double-click adds a part" 205,30,880,500
at "$third"; xdotool mousedown 1; sleep 0.2
for k in $(seq 1 12); do xdotool mousemove_relative -- 55 -70; sleep 0.04; done
at "$header"; sleep 0.3; xdotool mouseup 1; sleep 8; shot 11-replaced
check 10-added 11-replaced "browser: a preset dropped on a part replaces it" 205,40,880,70
# The edge between the libraries and the presets drags them taller.
at 1000,600; sleep 1; shot 12-edge
at "$edge"; xdotool mousedown 1; sleep 0.2
for _ in $(seq 1 15); do xdotool mousemove_relative -- 0 10; sleep 0.04; done
xdotool mouseup 1; at 1000,600; sleep 1.5; shot 13-edged
check 12-edge 13-edged "browser: its edge drags the lists taller" 0,280,198,500

# Idle: the pointer parked, nothing playing, a Kontakt face open. CPU over
# 60 s (IDLE_S), in % of a core,
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
at 1900,1270; sleep 2
span=${IDLE_S:-60}
t0=$(ticks); sleep "$span"
t1=$(ticks)
idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / span ))
echo "idle CPU: ${idle}% of a core over ${span} s"
if (( idle > 5 )); then miss "idle CPU"; else pass "idle CPU"; fi
exit $fail
