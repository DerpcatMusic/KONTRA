#!/usr/bin/env bash
# The plugin's real editor under Xvfb, driven with real pointer events:
# the face drawn untouched, a part tab, the header's library art, the browser's library cards, the preset field, a library knob
# dragged and a switch clicked, the part's look switched, a browser row clicked (selects), double-
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
knob=${KNOB:-746,834}          # after five wheel steps down
switch=${SWITCH:-1418,886}
search=${SEARCH:-100,80}       # the browser's search field
name=${NAME:-Una Corda}        # typed there, so its card comes first
library=${LIBRARY:-200,260}    # that card
second=${SECOND:-90,811}       # Una Corda Felt
third=${THIRD:-94,847}         # Una Corda Pure
edge=${EDGE:-200,578}          # the edge between libraries and presets
header=${HEADER:-760,114}      # part 0's header, to drop on
fold=${FOLD:-426,114}          # part 0's fold, so a new part shows below it
look=${LOOK:-900,164}          # the part's look: Original, Vector, KONTRA
vector=${VECTOR:-805,242}      # its menu's Vector
kontra=${KONTRA:-812,280}      # and KONTRA
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
# The Afflatus keyswitch tab has a six-frame picture. Its held frame is
# visible before the button's release opens the keyswitch page.
if [[ ${PRESSED_ONLY:-0} == 1 ]]; then
    at "${MULTILINE_TAB:-1652,260}"; sleep 1; shot 1-hover
    xdotool mousedown 1; sleep 2; shot 2-held
    check 1-hover 2-held "Kontakt button: held picture frame" 800,110,850,155
    xdotool mouseup 1; sleep 2; shot 3-released
    ticks_pressed() { local sum=0 t; for t in /proc/"$editor"/task/*; do
        [[ $(<"$t/comm") == cpal* ]] && continue
        sum=$((sum + $(awk '{print $14 + $15}' "$t/stat")))
    done; echo "$sum"; }
    at 1900,1270; sleep 2
    span=${IDLE_S:-60}; t0=$(ticks_pressed); sleep "$span"; t1=$(ticks_pressed)
    idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / span ))
    echo "idle CPU: ${idle}% of a core over ${span} s"
    if (( idle > 5 )); then miss "idle CPU"; else pass "idle CPU"; fi
    exit $fail
fi
# Compare a tabbed label to its pre-change real-window capture.
if [[ ${KSP_TABS_ONLY:-0} == 1 ]]; then
    if [[ -n ${KSP_TABS_BEFORE:-} ]]; then
        if python3 -I -c 'import sys; from PIL import Image
box = tuple(map(int, sys.argv[3].split(",")))
a, b = (Image.open(p).convert("RGB").crop(box) for p in sys.argv[1:3])
bright = lambda image: sum(min(pixel) > 65 for pixel in image.getdata())
sys.exit(0 if bright(a) >= 8 and bright(b) <= 2 else 1)' "$KSP_TABS_BEFORE" "$out/0-open.png" "${KSP_TABS_BOX:-443,204,448,211}"; then
            pass "Areia tab missing-glyph box disappears"
        else miss "Areia tab missing-glyph box disappears"; fi
    fi
    ticks_tabs() { local sum=0 t; for t in /proc/"$editor"/task/*; do
        [[ $(<"$t/comm") == cpal* ]] && continue
        sum=$((sum + $(awk '{print $14 + $15}' "$t/stat")))
    done; echo "$sum"; }
    at 1900,1270; sleep 2
    span=${IDLE_S:-60}; t0=$(ticks_tabs); sleep "$span"; t1=$(ticks_tabs)
    idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / span ))
    echo "idle CPU: ${idle}% of a core over ${span} s"
    if (( idle > 5 )); then miss "idle CPU"; else pass "idle CPU"; fi
    exit $fail
fi
# Compare a scaled library picture against its pre-change editor capture.
if [[ ${PICTURE_FILTER_ONLY:-0} == 1 ]]; then
    if [[ -n ${PICTURE_FILTER_BEFORE:-} ]]; then
        if python3 -I -c 'import sys; from PIL import Image, ImageChops
box = tuple(map(int, sys.argv[3].split(",")))
a, b = (Image.open(p).convert("RGB").crop(box) for p in sys.argv[1:3])
difference = ImageChops.difference(a, b)
pixels = list(difference.getdata())
changed = sum(max(pixel) > 8 for pixel in pixels)
mean = sum(map(sum, pixels)) / (3 * len(pixels)) if pixels else float("inf")
sys.exit(0 if changed >= 80 and mean < 3 else 1)' "$PICTURE_FILTER_BEFORE" "$out/0-open.png" "${PICTURE_FILTER_BOX:-440,185,600,260}"; then
            pass "scaled picture changes in the editor"
        else miss "scaled picture changes in the editor"; fi
    fi
    ticks_picture() { local sum=0 t; for t in /proc/"$editor"/task/*; do
        [[ $(<"$t/comm") == cpal* ]] && continue
        sum=$((sum + $(awk '{print $14 + $15}' "$t/stat")))
    done; echo "$sum"; }
    at 1900,1270; sleep 2
    span=${IDLE_S:-60}; t0=$(ticks_picture); sleep "$span"; t1=$(ticks_picture)
    idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / span ))
    echo "idle CPU: ${idle}% of a core over ${span} s"
    if (( idle > 5 )); then miss "idle CPU"; else pass "idle CPU"; fi
    exit $fail
fi
# Afflatus' keyswitch page uses add_text_line for its note-name list.
if [[ ${MULTILINE_ONLY:-0} == 1 ]]; then
    at "${MULTILINE_TAB:-1652,260}"; xdotool click 1; sleep 3; at 1900,1270; shot 1-keyswitches
    if [[ -n ${MULTILINE_BEFORE:-} ]]; then
        if python3 -I -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB").crop((805,175,845,408)) for p in sys.argv[1:])
sys.exit(0 if ImageChops.difference(a,b).getbbox() else 1)' "$MULTILINE_BEFORE" "$out/1-keyswitches.png"; then
            pass "Afflatus keyswitch names: multiple lines draw"
        else miss "Afflatus keyswitch names: multiple lines draw"; fi
    fi
    ticks_multiline() { local sum=0 t; for t in /proc/"$editor"/task/*; do
        [[ $(<"$t/comm") == cpal* ]] && continue
        sum=$((sum + $(awk '{print $14 + $15}' "$t/stat")))
    done; echo "$sum"; }
    at 1900,1270; sleep 2
    span=${IDLE_S:-60}; t0=$(ticks_multiline); sleep "$span"; t1=$(ticks_multiline)
    idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / span ))
    echo "idle CPU: ${idle}% of a core over ${span} s"
    if (( idle > 5 )); then miss "idle CPU"; else pass "idle CPU"; fi
    exit $fail
fi
# Open VWinds' preferences page, where the authored tables live.
if [[ ${UVI_TABLE_ONLY:-0} == 1 ]]; then
    at "${UVI_PREFS:-1310,944}"; xdotool click 1; sleep 3; shot 1-prefs
    if [[ -n ${UVI_TABLE_TAB:-} ]]; then
        at "$UVI_TABLE_TAB"; xdotool click 1; sleep 3; shot 2-table
        if [[ -n ${UVI_TABLE_BEFORE:-} ]]; then
            if python3 -I -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB").crop((385,160,697,305)) for p in sys.argv[1:])
d = ImageChops.difference(a,b)
sys.exit(0 if sum(1 for p in d.getdata() if max(p) > 8) > 100 else 1)' "$UVI_TABLE_BEFORE" "$out/2-table.png"; then
                pass "UVI auto-vibrato table draws its bars"
            else miss "UVI auto-vibrato table draws its bars"; fi
        fi
    fi
    at 1900,1270; sleep 2
    ticks_table() { local sum=0 t; for t in /proc/"$editor"/task/*; do
        [[ $(<"$t/comm") == cpal* ]] && continue
        sum=$((sum + $(awk '{print $14 + $15}' "$t/stat")))
    done; echo "$sum"; }
    span=${IDLE_S:-60}; t0=$(ticks_table); sleep "$span"; t1=$(ticks_table)
    idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / span ))
    echo "idle CPU: ${idle}% of a core over ${span} s"
    if (( idle > 5 )); then miss "idle CPU"; else pass "idle CPU"; fi
    exit $fail
fi
# UVI's script page font styling; UVI_FONT_BEFORE can name a pre-change
# capture to check that the page actually changed in the real window.
if [[ ${UVI_FONT_ONLY:-0} == 1 ]]; then
    if [[ -n ${UVI_FONT_BEFORE:-} ]]; then
        if python3 -I -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB").crop((790,478,840,495)) for p in sys.argv[1:])
sys.exit(0 if ImageChops.difference(a,b).getbbox() else 1)' "$UVI_FONT_BEFORE" "$out/0-open.png"; then
            pass "UVI script font styling changes the page"
        else miss "UVI script font styling changes the page"; fi
    fi
    ticks_uvi() {
        local sum=0 t
        for t in /proc/"$editor"/task/*; do
            [[ $(<"$t/comm") == cpal* ]] && continue
            sum=$((sum + $(awk '{print $14 + $15}' "$t/stat")))
        done
        echo "$sum"
    }
    at 1900,1270; sleep 2
    span=${IDLE_S:-60}; t0=$(ticks_uvi); sleep "$span"; t1=$(ticks_uvi)
    idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / span ))
    echo "idle CPU: ${idle}% of a core over ${span} s"
    if (( idle > 5 )); then miss "idle CPU"; else pass "idle CPU"; fi
    exit $fail
fi
# Pacific's unpictured knob: the top ring is lit and the bottom is shadowed.
# STOCK_ONLY is a short, real-window check for that stock skin.
if [[ ${STOCK_ONLY:-0} == 1 ]]; then
    # Compare with a capture made without KONTAKT_STOCK_EXECUTABLE to prove
    # the installed artwork reached the real editor.
    if [[ -n ${STOCK_BASELINE:-} ]]; then
        if python3 -I -c 'import sys; from PIL import Image, ImageChops
box = tuple(map(int, sys.argv[3].split(",")))
a = Image.open(sys.argv[1]).convert("RGB").crop(box)
b = Image.open(sys.argv[2]).convert("RGB").crop(box)
sys.exit(0 if ImageChops.difference(a, b).getbbox() else 1)' "$STOCK_BASELINE" "$out/0-open.png" "${STOCK_COMPARE_BOX:-180,120,400,220}"; then
            pass "installed Kontakt stock artwork"
        else miss "installed Kontakt stock artwork"; fi
    fi
    if python3 -I -c 'import sys; from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
top, bottom = im.getpixel((235, 150))[0], im.getpixel((235, 181))[0]
sys.exit(0 if top > bottom + 5 else 1)' "$out/0-open.png"; then
        pass "stock knob: upper ring is lit"
    else miss "stock knob: upper ring is lit"; fi
    ticks_stock() {
        local sum=0 t
        for t in /proc/"$editor"/task/*; do
            [[ $(<"$t/comm") == cpal* ]] && continue
            sum=$((sum + $(awk '{print $14 + $15}' "$t/stat")))
        done
        echo "$sum"
    }
    at 1900,1270; sleep 2
    span=${IDLE_S:-60}; t0=$(ticks_stock); sleep "$span"; t1=$(ticks_stock)
    idle=$(( (t1 - t0) * 100 / $(getconf CLK_TCK) / span ))
    echo "idle CPU: ${idle}% of a core over ${span} s"
    if (( idle > 5 )); then miss "idle CPU"; else pass "idle CPU"; fi
    exit $fail
fi
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
# The part's look: Vector drops the pictures, KONTRA lists the controls.
at 1900,1270; sleep 1; shot 6a-original
at "$look"; xdotool click 1; sleep 1; at "$vector"; xdotool click 1; sleep 2
at 1900,1270; sleep 1; shot 6b-vector
check 6a-original 6b-vector "part look: Vector draws without pictures" 205,100,880,500
# Fit must show all three Una Corda sliders before any scrolling in Vector.
if [[ ${name} == "Una Corda" ]]; then
    if python3 -I -c 'import sys; from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
boxes = [(360, 430, 386, 466), (528, 430, 554, 466), (696, 430, 722, 466)]
sys.exit(0 if all(sum(1 for p in im.crop(box).getdata() if min(p) > 100) >= 10 for box in boxes) else 1)' "$out/6b-vector.png"; then
        pass "Vector: all three sliders visible without scrolling"
    else miss "Vector: all three sliders visible without scrolling"; fi
fi
at "$look"; xdotool click 1; sleep 1; at "$kontra"; xdotool click 1; sleep 2
at 1900,1270; sleep 1; shot 6c-kontra
check 6b-vector 6c-kontra "part look: KONTRA lists the controls" 205,100,880,500
# When comparing an earlier build, check the generated control captions too.
if [[ -n ${LABEL_BEFORE:-} ]]; then
    if python3 -I -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB").crop((205, 115, 760, 130)) for p in sys.argv[1:])
sys.exit(0 if ImageChops.difference(a, b).getbbox() else 1)' "$LABEL_BEFORE" "$out/6c-kontra.png"; then
        pass "KONTRA control captions changed"
    else miss "KONTRA control captions changed"; fi
fi
at "$look"; xdotool click 1; sleep 1; at "$look"; sleep 0.2
xdotool mousemove_relative -- -90 40; xdotool click 1; sleep 2
at 1900,1270; sleep 1; shot 6d-back
# The pictures decode again: the same, but for a little resampling.
if python3 -I -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(p).convert("RGB").crop((205, 100, 880, 500)) for p in sys.argv[1:3])
sys.exit(0 if max(map(max, ImageChops.difference(a, b).getextrema())) <= 40 else 1)' \
    "$out/6a-original.png" "$out/6d-back.png"; then
    pass "part look: Original comes back"
else miss "part look: Original comes back"; fi

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
