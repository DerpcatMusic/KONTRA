#!/usr/bin/env bash
# Una Cotton: scripts 1-4 LEFT ON, solo_group.ksp applied in slot 5 only.
export DISPLAY=:77; here=$(cd "$(dirname "$0")" && pwd)
c() { xdotool mousemove "$1" "$2"; sleep 0.5; xdotool click 1; sleep 1; }
c 565 203; c 1370 307; c 1375 343; c 578 422
TY=650 AY=454 "$here/ksp_apply.sh" "$here/scenarios/solo_group.ksp"
c 1427 454
