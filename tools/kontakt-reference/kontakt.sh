#!/usr/bin/env bash
# Kontakt 8 standalone under Wine on a private Xvfb, audio to a null sink,
# MIDI from ALSA "Midi Through". Does not touch the Wine prefix or yabridge;
# Kontakt's own prefs (Portapotty/UserData/Settings.cfg) hold the audio device.
# Usage: kontakt.sh start INSTRUMENT.nki | stop
# One-time: in Kontakt Options > Audio pick device "kontra_ref" (WASAPI shared,
# 48000 Hz); back up Settings.cfg first. GUI coordinates assume the 1600x1000 desktop.
set -euo pipefail
export DISPLAY=:77 WINEDLLOVERRIDES="d3d11,d3d10core,dxgi,d3d9=b"  # wined3d: DXVK crashes on Xvfb
K="${KONTAKT_EXE:-/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe}"
W="${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}"
mkdir -p "$W/log" "$W/wav"
click() { xdotool mousemove "$1" "$2"; sleep 0.4; xdotool click 1; sleep "${3:-1.2}"; }

case ${1:-} in
start)
  pgrep -x Xvfb -a | grep -q ':77' || { setsid nohup Xvfb :77 -screen 0 1600x1000x24 >/dev/null 2>&1 & sleep 1; }
  pactl list short sinks | grep -q kontra_ref || pactl load-module module-null-sink sink_name=kontra_ref \
    sink_properties=device.description=kontra_ref format=float32le rate=48000 channels=2 >/dev/null
  # explorer /desktop: mouse input only works inside a Wine virtual desktop.
  setsid nohup wine explorer /desktop=k8,1600x1000 "$K" "$(winepath -w "$2")" >"$W/log/kontakt.log" 2>&1 </dev/null &
  sleep "${KONTAKT_LOAD_SECONDS:-45}"
  click 1089 440            # "What's new" popup
  click 20 57; click 60 86  # File > Options
  click 388 553            # Audio tab: the device choice does not persist across launches
  click 1088 318 0.5; xdotool mousemove 1060 480; for _ in 1 2 3 4 5 6; do xdotool click 5; done; sleep 0.5
  click 1010 485 2          # "kontra_ref" output (list position as of 6 scroll clicks)
  click 385 520             # MIDI tab
  click 1075 414; click 1128 476  # Midi Through Port-0 -> Port A
  click 1178 781            # Close
  import -window root "$W/log/ready.png"
  ;;
stop)
  click 20 57 || true; click 42 123 || true   # File > Exit
  sleep 3
  pkill -f '[K]ontakt 8.exe' || true
  pkill -f 'explorer.exe /desktop=k8' || true
  ;;
*) sed -n 2,7p "$0"; exit 2 ;;
esac
