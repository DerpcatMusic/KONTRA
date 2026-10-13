#!/usr/bin/env python3
"""Run a probe Lua inside Kontakt (see dump.py) and record the audio it plays.

    render.py PROBE.lua OUT.wav SECONDS

The probe builds an instrument, attaches a KSP script that plays notes on a timer, and writes
{"ready":true} to ORACLE_OUT when playback has started (then {"done":true} is not needed here).
Kontakt's output is forced onto a private PipeWire null sink (`kontra_ref`, created if missing, so
the user's speakers/DAC are never used); the recording is that sink's monitor, 48 kHz float stereo.
Kontakt is killed afterwards. One Kontakt session at a time (same guard as dump.py).
"""
import importlib.util
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("dump", HERE / "dump.py")
dump = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dump)

SINK = "kontra_ref"


def sh(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout


def click(env, x, y, wait=1.2):
    subprocess.run(["xdotool", "mousemove", str(x), str(y)], env=env)
    time.sleep(0.4)
    subprocess.run(["xdotool", "click", "1"], env=env)
    time.sleep(wait)


def shot(env, name):
    subprocess.run(["import", "-window", "root", f"/tmp/w3/audit/{name}.png"], env=env, check=False)


def select_device(env):
    """The audio device choice does not persist across launches (tools/kontakt-reference/kontakt.sh
    setup): open File > Options > Audio and pick the kontra_ref output with the harness's coordinates."""
    time.sleep(5)
    shot(env, "r0")
    click(env, 1089, 440)
    click(env, 20, 57)
    click(env, 60, 86)
    click(env, 388, 553)
    shot(env, "r1")
    click(env, 1088, 318, 0.5)
    shot(env, "r2")
    subprocess.run(["xdotool", "mousemove", "1060", "480"], env=env)
    for _ in range(6):
        subprocess.run(["xdotool", "click", "5"], env=env)
    time.sleep(0.5)
    shot(env, "r3")
    click(env, 1010, 485, 2)
    shot(env, "r4")
    click(env, 385, 520)                      # MIDI tab
    click(env, 1075, 414)
    click(env, 1128, 476)                     # Midi Through Port-0 -> Port A
    shot(env, "r5")
    click(env, 1178, 781)


def trigger(midi=None):
    """One MIDI note into Kontakt (ALSA Midi Through 14:0) starts the probe's KSP sequence."""
    mid = Path(midi) if midi else Path("/tmp/w3/audit/trigger.mid")
    trk = bytes([0x00, 0x90, 36, 100, 0x60, 0x80, 36, 0, 0x00, 0xFF, 0x2F, 0x00])
    if not midi:
        mid.write_bytes(b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1, 0x01, 0xE0]) + b"MTrk" + len(trk).to_bytes(4, "big") + trk)
    port = next((l.split()[0] for l in sh("aplaymidi", "-l").splitlines() if "Midi Through" in l), "14:0")
    subprocess.run(["aplaymidi", "-p", port, str(mid)], check=False)


def main() -> int:
    lua, out_wav, seconds = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), float(sys.argv[3])
    midi = sys.argv[4] if len(sys.argv) > 4 else None
    if subprocess.run(["pgrep", "-f", "[K]ontakt 8.exe"], capture_output=True).returncode == 0:
        sys.exit("another Kontakt is running; one oracle session at a time")
    if SINK not in sh("pactl", "list", "short", "sinks"):
        sh("pactl", "load-module", "module-null-sink", f"sink_name={SINK}",
           f"sink_properties=device.description={SINK}", "format=float32le", "rate=48000", "channels=2")
    dump.WORK.mkdir(parents=True, exist_ok=True)
    raw = dump.WORK / "oracle.jsonl"
    raw.unlink(missing_ok=True)
    script = dump.WORK / "oracle.lua"
    script.write_text("ORACLE_INPUTS = {}\nORACLE_OUT = " + dump.lua_str(dump.winpath(raw)) + "\n" + lua.read_text())
    xvfb = subprocess.Popen(["Xvfb", dump.DISPLAY, "-screen", "0", "1600x1000x24"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    env = dict(os.environ, DISPLAY=dump.DISPLAY, WINEDLLOVERRIDES="d3d11,d3d10core,dxgi,d3d9=b",
               WINEDEBUG="-all", PULSE_SINK=SINK)
    kontakt = subprocess.Popen(["wine", "explorer", "/desktop=oracle,1600x1000", dump.EXE, dump.winpath(script)],
                               env=env, stdout=open(dump.WORK / "kontakt.log", "w"), stderr=subprocess.STDOUT,
                               start_new_session=True)
    rec = keep = None
    try:
        deadline = time.time() + 240
        while time.time() < deadline and not (raw.exists() and '"ready"' in raw.read_text(errors="replace")):
            dump.dismiss_dialogs(env)
            time.sleep(2)
        else:
            if not raw.exists() or '"ready"' not in raw.read_text(errors="replace"):
                sys.exit(f"probe never became ready; see {dump.WORK}")
        # routing guard: Kontakt must feed only the null sink
        select_device(env)
        time.sleep(6)
        print(sh("pactl", "list", "short", "sink-inputs"))
        print([l for l in sh("pw-link", "-o").splitlines() if "kontra" not in l][:20])
        ports = [l for l in sh("pw-link", "-o").splitlines() if "Kontakt:output_" in l]
        print("kontakt output ports:", len(ports))
        for p in ports:
            sh("pw-link", p, f"{SINK}:playback_{p[-2:]}")
        keep = subprocess.Popen(["pw-cat", "-p", "--raw", "--rate", "48000", "--channels", "2", "--format", "f32",
                                 "--target", SINK, "/dev/zero"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        rec = subprocess.Popen(["pw-record", "--target", SINK, "-P", "{ stream.capture.sink=true }", "--rate", "48000",
                                "--format", "f32", "--channels", "2", str(out_wav)])
        time.sleep(1)
        trigger(midi)
        time.sleep(seconds)
        rec.send_signal(signal.SIGINT)
        rec.wait(10)
    finally:
        if keep:
            keep.terminate()
        subprocess.run(["pkill", "-f", "[K]ontakt 8.exe"], check=False)
        time.sleep(2)
        os.killpg(kontakt.pid, signal.SIGKILL) if kontakt.poll() is None else None
        xvfb.terminate()
    print(f"{out_wav}: {out_wav.stat().st_size} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
