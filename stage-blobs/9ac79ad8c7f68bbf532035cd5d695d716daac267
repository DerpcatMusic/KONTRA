#!/usr/bin/env bash
# Una Cotton (just loaded): open Script Editor, bypass scripts 1-4, apply solo_group.ksp in slot 5. Coordinates: see kontakt.sh header.
export DISPLAY=:77; here=$(cd "$(dirname "$0")" && pwd)
c() { xdotool mousemove "$1" "$2"; sleep 0.5; xdotool click 1; sleep 1; }
c 565 203; c 1370 307
for x in 620 800 1000 1190; do c $x 343; c 563 371; done
c 1375 343; c 578 422
TY=650 AY=454 "$here/ksp_apply.sh" "$here/scenarios/solo_group.ksp"
c 1427 454
