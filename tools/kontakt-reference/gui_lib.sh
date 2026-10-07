# GUI helpers for sessions on the 1600x1400 desktop (source me after session_lib.sh). Needs DISPLAY=:77, W set.
# Kontakt swallows the first click after a menu action or view change, so: use clk_ok/neutral clicks and verify with screenshots.
clk() { xdotool mousemove "$1" "$2"; sleep .5; xdotool click 1; sleep "${3:-2}"; }
shot() { import -window root -crop 1000x800+540+180 "$W/log/$1.png"; }
classic_group_editor() { clk 325 98; clk 128 579 5; clk 1000 700 1; clk 565 203 2.5; clk 770 307 2.5
  xdotool mousemove 1000 500; for _ in $(seq 25); do xdotool click 5; sleep .2; done; sleep 4; }
# addfx PLUSX PLUSY UTILITIES_Y DOWNS : add a Utilities-submenu entry (DOWNS=1 first item, 2 second)
addfx() { clk 590 540 1; clk "$1" "$2" 3; xdotool mousemove 730 "$3"; sleep 1; xdotool key Right; sleep .5
  for _ in $(seq "$4"); do xdotool key Down; sleep .2; done; xdotool key Return; sleep 3; }
edit_open() { for _ in 1 2 3; do import -window root -crop 900x120+540+620 /tmp/eo0.png; clk 562 598 3; import -window root -crop 900x120+540+620 /tmp/eo1.png
  cmp -s /tmp/eo0.png /tmp/eo1.png || return 0; done; return 1; }
# typef X Y VALUE : type into a value field without committing; commitf = Return
typef() { xdotool mousemove "$1" "$2"; sleep .5; xdotool click --repeat 2 --delay 100 1; sleep .6; xdotool key ctrl+a; xdotool type --delay 40 -- "$3"; sleep .3; xdotool mousemove 1300 300; }
setf() { typef "$1" "$2" "$3"; xdotool key Return; sleep 1.2; }
