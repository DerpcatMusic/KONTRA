#!/usr/bin/env bash
# shoot.sh OUT.png PAGE... -- inside oracle-slot (tools/oracle/oracle-slot): builds each PROBES/PAGE.ksp
# (PROBES defaults to probes/) into lib/Instruments/PAGE.nki (a PAGE that is a path to an .nki is
# loaded as it is) with lib/Resources as its resource
# container, loads them all into one multi,
# hides browser and keyboard and shoots the root window; with one PAGE, drives it (drive.py) and keeps
# its trace beside the shot. DPI=96 shoots at 100 %, the default 144 at 150 %.
set -u
[ -n "${ORACLE_PREFIX:-}" ] || { echo "run under oracle-slot" >&2; exit 2; }
out=$1; shift; here=$(cd "$(dirname "$0")" && pwd) probes=${PROBES:-$here/probes}
export DISPLAY=$ORACLE_DISPLAY WINEDLLOVERRIDES="d3d11,d3d10core,dxgi,d3d9=b"
K="$ORACLE_PREFIX/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe"
W=${ORACLE_WORK:?run under oracle-slot}/ksp-ui-$$; mkdir -p "$W"
raw=$W/out.jsonl; lua=$W/probe.lua
for page in "$@"; do rm -f /data/dev/workspace/scratch/ksp-ui-trace/$page.nka; done
{ echo "ORACLE_PAGES = {"
  for page in "$@"; do
    # An existing instrument (a path ending .nki) is only loaded, not built.
    case $page in
      *.nki) echo "  {\"\", [[$(winepath -w "$page")]]}," ;;
      *) echo "  {[[$(winepath -w "$probes/$page.ksp")]], [[$(winepath -w "$here")\\lib\\Instruments\\$page.nki]]}," ;;
    esac
  done
  echo "}"
  echo "ORACLE_OUT = [[$(winepath -w "$raw")]]"
  echo "ORACLE_NKR = [[$(winepath -w "$here")\\lib\\probes.nkr]]"
  cat "$here/build.lua"; } > "$lua"
Xvfb "$DISPLAY" -screen 0 1600x1200x24 -nolisten tcp >/dev/null 2>&1 &
for _ in $(seq 40); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.5; done; sleep 1
# DPI=96 shoots Kontakt at 100 % (the slot's own 144 is put back on exit).
if [ -n "${DPI:-}" ]; then
  # Kontakt first: `wineserver -w` waits for every Wine process, so a running Kontakt held the
  # slot until the job's time limit.
  trap 'wineserver -k; wine reg add "HKCU\\Control Panel\\Desktop" /v LogPixels /t REG_DWORD /d 144 /f >/dev/null 2>&1; wineserver -w' EXIT
  wine reg add "HKCU\\Control Panel\\Desktop" /v LogPixels /t REG_DWORD /d "$DPI" /f >/dev/null 2>&1
fi
nice -n 19 wine explorer /desktop=k8,1600x1200 "$K" "$(winepath -w "$lua")" >"$out.log" 2>&1 </dev/null &
for _ in $(seq 120); do grep -q done "$raw" 2>/dev/null && break; sleep 2; done
cat "$raw" >> "$out.log" 2>/dev/null
sleep 6; pgrep -f "Kontakt 8.exe" >/dev/null && echo kontakt-alive >> "$out.log"; import -window root "$out.done.png"
for k in Escape Return Escape; do xdotool key $k; sleep 1.5; done
# Hide the browser and the keyboard (the bottom bar's toggles sit elsewhere at 100 %).
if [ "${DPI:-144}" = 96 ]; then bar="14,711 1180,711"; else bar="20,1065 1252,1065"; fi
for at in $bar; do xdotool mousemove ${at%,*} ${at#*,} click 1; sleep 3; done
xdotool mousemove 1590 1190; sleep 2
import -window root "$out"
# An existing instrument: Kontakt keeps its window 728 px tall (neither maximise nor Wine's
# keyboard resize changes it), so drag the rack's scrollbar down and shoot the page's lower part too.
case ${1:-} in *.nki)
  # The window's right edge: the last column on row 300 that is not the desktop's blue.
  right=$(python3 -c "
from PIL import Image; im = Image.open('$out').convert('RGB')
print(max(x for x in range(im.width) if im.getpixel((x, 300)) != (37, 111, 149)))")
  xdotool mousemove $((right - 17)) 250 mousedown 1; sleep 0.5
  xdotool mousemove $((right - 17)) 750; sleep 0.5; xdotool mouseup 1
  xdotool mousemove 1590 1190; sleep 3
  import -window root "${out%.png}-lower.png" ;;
esac
# One page: drive its controls (probes/PAGE.drive), shoot again and keep its trace.
if [ $# = 1 ] && [ -z "${NODRIVE:-}" ] && [ -f "$probes/$1.drive" ]; then
  # Kontakt's page corner with the browser and keyboard hidden (measured on the widgets page).
  if [ "${DPI:-144}" = 96 ]; then origin=; else origin=11,281,1.5; fi
  ORIGIN=$origin python3 "$here/drive.py" "$probes/$1.drive" "$out" 2>>"$out.log"
  xdotool mousemove 1590 1190; sleep 4
  import -window root "${out%.png}-driven.png"
  cp /data/dev/workspace/scratch/ksp-ui-trace/$1.nka "${out%.png}.nka" 2>/dev/null
fi
