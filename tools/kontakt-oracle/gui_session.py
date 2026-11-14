#!/usr/bin/env python3
"""A Kontakt oracle session kept open for editor work no API reaches (adding a modulator).

    oracle-slot tools/kontakt-oracle/gui_session.py PROBE.lua

Starts Kontakt (render.py's setup: private slot prefix, null sink) with PROBE.lua, waits for its
{"ready":true}, then reads commands, one per line, from the FIFO it prints (ORACLE_WORK/gui.cmd):
  shot NAME              screenshot to ORACLE_WORK/NAME.png
  click X Y | rclick X Y | dclick X Y
  drag X1 Y1 X2 Y2
  key KEYS               xdotool key (e.g. ctrl+s, Return)
  type TEXT
  record SECONDS OUT.wav [MIDI]   record the null sink while MIDI (or one trigger note) plays
  quit
Each command's result is appended to ORACLE_WORK/gui.log.
"""
import importlib.util
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("render", HERE / "render.py")
render = importlib.util.module_from_spec(spec)
spec.loader.exec_module(render)
dump, slot = render.dump, render.slot


def xdo(env, *args):
    return subprocess.run(["xdotool", *map(str, args)], env=env, capture_output=True, text=True)


def main() -> int:
    lua = Path(sys.argv[1]).resolve()
    if not slot.active():
        sys.exit("run inside oracle-slot (a private Wine prefix)")
    if slot.kontakt_pids():
        sys.exit("another Kontakt is running in this slot")
    slot.ensure_sink(render.SINK)
    work = slot.work(Path.home() / ".cache/kontra-oracle")
    work.mkdir(parents=True, exist_ok=True)
    dump.WORK.mkdir(parents=True, exist_ok=True)
    raw = dump.WORK / "oracle.jsonl"
    raw.unlink(missing_ok=True)
    script = dump.WORK / "oracle.lua"
    # ORACLE_CHANNEL: the 1-based MIDI channel of this slot's block; the probe binds its instrument to it.
    script.write_text("ORACLE_INPUTS = {}\nORACLE_CHANNEL = " + str(slot.channel_block()[0] + 1) + "\nORACLE_OUT = "
                      + dump.lua_str(dump.winpath(raw)) + "\n" + lua.read_text())
    fifo, log = work / "gui.cmd", work / "gui.log"
    fifo.unlink(missing_ok=True)
    os.mkfifo(fifo)
    log.write_text("")
    xvfb = subprocess.Popen(["Xvfb", dump.DISPLAY, "-screen", "0", "1600x1000x24"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    env = slot.wine_env(dict(os.environ, DISPLAY=dump.DISPLAY, WINEDLLOVERRIDES="d3d11,d3d10core,dxgi,d3d9=b",
                             WINEDEBUG="-all", PULSE_SINK=render.SINK))
    kontakt = subprocess.Popen(["wine", "explorer", "/desktop=oracle,1600x1000", dump.EXE, dump.winpath(script)],
                               env=env, stdout=open(dump.WORK / "kontakt.log", "w"), stderr=subprocess.STDOUT,
                               start_new_session=True)

    def note(text):
        with open(log, "a") as f:
            f.write(text + "\n")

    try:
        deadline = time.time() + 240
        while time.time() < deadline and not (raw.exists() and '"ready"' in raw.read_text(errors="replace")):
            dump.dismiss_dialogs(env)
            time.sleep(2)
        if not raw.exists() or '"ready"' not in raw.read_text(errors="replace"):
            sys.exit(f"probe never became ready; see {dump.WORK}")
        render.select_device(env)
        time.sleep(4)
        slot.route_slot(render.SINK, slot.kontakt_pids(), wait=60)
        note("ready " + raw.read_text(errors="replace").replace("\n", " "))
        print(f"commands: {fifo}\nlog: {log}", flush=True)
        while True:
            with open(fifo) as f:
                lines = [l.split() for l in f.read().splitlines() if l.strip()]
            for cmd in lines:
                op, args = cmd[0], cmd[1:]
                if op == "quit":
                    note("bye")
                    return 0
                if op == "shot":
                    render.shot(env, args[0])
                elif op in ("click", "rclick", "dclick"):
                    xdo(env, "mousemove", args[0], args[1])
                    time.sleep(0.3)
                    button = "3" if op == "rclick" else "1"
                    xdo(env, "click", *(["--repeat", "2"] if op == "dclick" else []), button)
                    time.sleep(1.0)
                elif op == "drag":
                    xdo(env, "mousemove", args[0], args[1])
                    xdo(env, "mousedown", "1")
                    time.sleep(0.3)
                    xdo(env, "mousemove", args[2], args[3])
                    time.sleep(0.3)
                    xdo(env, "mouseup", "1")
                    time.sleep(1.0)
                elif op == "key":
                    xdo(env, "key", *args)
                    time.sleep(0.5)
                elif op == "type":
                    xdo(env, "type", " ".join(args))
                    time.sleep(0.5)
                elif op == "record":
                    seconds, out = float(args[0]), Path(args[1])
                    keep = subprocess.Popen(["pw-cat", "-p", "--raw", "--rate", "48000", "--channels", "2", "--format",
                                             "f32", "--target", render.SINK, "/dev/zero"],
                                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    rec = subprocess.Popen(["pw-record", "--target", render.SINK, "-P", "{ stream.capture.sink=true }",
                                            "--rate", "48000", "--format", "f32", "--channels", "2", str(out)])
                    time.sleep(1)
                    # A MIDI file is moved into this slot's channel block (the ALSA port is shared).
                    midi = slot.remap_midi(Path(args[2]), work / "block.mid", slot.channel_block()[0]) if len(args) > 2 else None
                    render.trigger(midi)
                    time.sleep(seconds)
                    rec.send_signal(signal.SIGINT)
                    rec.wait(10)
                    keep.terminate()
                note(f"done {' '.join(cmd)}")
    finally:
        slot.stop_kontakt()
        os.killpg(kontakt.pid, signal.SIGKILL) if kontakt.poll() is None else None
        xvfb.terminate()
        fifo.unlink(missing_ok=True)


if __name__ == "__main__":
    sys.exit(main())
