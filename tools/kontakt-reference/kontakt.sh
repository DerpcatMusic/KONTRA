#!/usr/bin/env bash
# Kontakt 8 standalone under Wine on a private Xvfb, audio to a null sink,
# MIDI from ALSA "Midi Through". Does not touch the Wine prefix or yabridge;
# Kontakt's own prefs (Portapotty/UserData/Settings.cfg) hold the audio device.
# Usage: kontakt.sh start INSTRUMENT.nki | setup | route | state | calibrate | stop
# Reference protocol (docs/architecture-v2/REFERENCE_PROTOCOL.md): `calibrate` (needs scenarios/calibration.nki loaded) must pass once per
# session; record.sh refuses to run without a valid stamp, a pinned `state` and a protocol-compliant MIDI file.
# One-time: in Kontakt Options > Audio pick device "kontra_ref" (WASAPI shared,
# 48000 Hz); back up Settings.cfg first. GUI coordinates assume the 1600x1000 desktop.
set -euo pipefail
RES="${KONTAKT_RES:-1600x1000}"  # GUI click coordinates in setup assume 1600x1000
export DISPLAY=:77 WINEDLLOVERRIDES="d3d11,d3d10core,dxgi,d3d9=b"  # wined3d: DXVK crashes on Xvfb
K="${KONTAKT_EXE:-/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe}"
W="${KONTRA_REFERENCE_DIR:-/home/derpcat/.cache/kontra-reference}"
mkdir -p "$W/log" "$W/wav"
click() { xdotool mousemove "$1" "$2"; sleep 0.4; xdotool click 1; sleep "${3:-1.2}"; }

case ${1:-} in
start)
  pgrep -x Xvfb -a | grep -q ':77' || { setsid nohup Xvfb :77 -screen 0 ${RES}x24 >/dev/null 2>&1 & sleep 1; }
  pactl list short sinks | grep -q kontra_ref || pactl load-module module-null-sink sink_name=kontra_ref \
    sink_properties=device.description=kontra_ref format=float32le rate=48000 channels=2 >/dev/null
  # explorer /desktop: mouse input only works inside a Wine virtual desktop.
  setsid nohup wine explorer /desktop=k8,$RES "$K" "$(winepath -w "$2")" >"$W/log/kontakt.log" 2>&1 </dev/null &
  sleep 15
  for _ in $(seq 150); do   # wait until the loading Progress dialog is gone
    xdotool search --name '^Progress$' >/dev/null 2>&1 || break; sleep 2
  done
  sleep 5
  # KONTAKT_NOAUDIO=1: read the GUI only (larger desktop); no audio device setup, never send MIDI
  [ -n "${KONTAKT_NOAUDIO:-}" ] || { "$0" setup; sleep 3; "$0" setup; "$0" route || { "$0" stop; exit 1; }; }
  # a killed/restarted Kontakt may come up in the new view; the GUI coordinates need Classic View (menu > Switch to Classic View)
  "$0" state 2>&1 | grep -q 'MISMATCH master' && { click 325 98; click 128 579 3; }   # only when the master editor itself is not found (instrument-header mismatches mean "still loading")
  import -window root "$W/log/ready.png"
  ;;
setup)  # GUI-script the audio device and MIDI port once the instrument has loaded
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
route)  # routing guard: Kontakt's output ports must be linked ONLY to kontra_ref (never the user's default sink)
  src=$(pw-link -o | grep -E '^Native Instruments Kontakt:output_(FL|FR)$' || true)
  [ -n "$src" ] || { echo "route: Kontakt has no output ports yet" >&2; exit 1; }
  for _ in 1 2 3; do
    while IFS= read -r port; do ch=${port##*_}
      pw-link -l | awk -v P="$port" '$0==P{f=1;next} /^[^ |]/{f=0} f&&/\|->/{sub(/^ *\|-> /,"");print}' |
        while IFS= read -r dst; do [ "$dst" = "kontra_ref:playback_$ch" ] || pw-link -d "$port" "$dst"; done
      pw-link "$port" "kontra_ref:playback_$ch" 2>/dev/null || true
    done <<<"$src"
    bad=$(pw-link -l | awk '/^Native Instruments Kontakt:output_/{f=1;next} /^[^ |]/{f=0} f&&/\|->/' | grep -vc 'kontra_ref:playback_' || true)
    ok=$(pw-link -l | awk '/^Native Instruments Kontakt:output_/{f=1;next} /^[^ |]/{f=0} f&&/\|->/' | grep -c 'kontra_ref:playback_' || true)
    [ "$bad" = 0 ] && [ "$ok" -ge 2 ] && { echo "route: ok (Kontakt -> kontra_ref only)"; exit 0; }
    sleep 1
  done
  echo "route: FAILED, Kontakt is not isolated on kontra_ref" >&2; exit 1 ;;
state)  # pinned-state check: master 0.00 dB / 440 Hz, instrument volume 0 dB, pan C, tune 0 (golden screenshots); prints the values it verified
  here=$(cd "$(dirname "$0")" && pwd); xdotool mousemove 1300 420; sleep 0.5; import -window root "$W/log/state_closed.png"
  click 1224 98 1.5; xdotool mousemove 1300 420; sleep 0.5; import -window root "$W/log/state_master.png"; click 1224 98 1.0
  python3 "$here/state_check.py" "$W/log/state_closed.png" "$W/log/state_master.png" ;;
calibrate)  # gain through the whole chain: bare noise instrument at unity, recorded level must be the file level x the master law
  here=$(cd "$(dirname "$0")" && pwd)
  [ -f /tmp/noise.wav ] || python3 "$here/make_noise.py" /tmp/noise.wav noise
  "$0" state || { echo "calibrate: state not pinned, abort" >&2; exit 1; }
  python3 "$here/scenario.py" "$here/scenarios/calibration.txt" "$W/cal.mid"
  KONTRA_NO_STAMP_CHECK=1 "$here/record.sh" "$W/cal.mid" "$W/wav/cal.wav" 3 || exit 1
  if python3 "$here/calibrate.py" "$W/wav/cal.wav" /tmp/noise.wav 0.0 | tee "$W/log/calibration.txt"; then
    echo "$(pactl list short modules | awk '/kontra_ref/{print $1}') $("$(dirname "$0")/wsid.sh") $(date +%s)" >"$W/calibrated"; rm -f "$W/wav/cal.wav"
  else rm -f "$W/calibrated"; echo "calibrate: FAILED, session aborted (no recordings allowed)" >&2; exit 1; fi ;;
stop)
  click 20 57 || true; click 42 123 || true   # File > Exit
  sleep 3
  pkill -f '[K]ontakt 8.exe' || true
  pkill -f 'explorer.exe /desktop=k8' || true
  ;;
*) sed -n 2,7p "$0"; exit 2 ;;
esac
