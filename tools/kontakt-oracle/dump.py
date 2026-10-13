#!/usr/bin/env python3
"""Dump Kontakt's own post-load state for .nki files (the native oracle).

    tools/kontakt-oracle/dump.py OUT.jsonl A.nki [B.nki ...]
    ORACLE_LUA=probe.lua tools/kontakt-oracle/dump.py OUT.jsonl [inputs...]   # custom probe

A probe gets the same ORACLE_INPUTS / ORACLE_OUT globals and must end its output
with the line {"done":true}.

Runs Kontakt 8 standalone under Wine on a private Xvfb with dump.lua (Kontakt Lua
API; needs DeveloperFeatures=dw:1 in Portapotty/UserData/Settings.cfg). No audio,
no MIDI. Kontakt keeps running after a script, so we stop it once the done line
appears. Output never contains script source or sample data.
"""
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
EXE = os.environ.get("KONTAKT_EXE", "/home/derpcat/.wine/drive_c/Program Files/Common Files/"
                     "VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe")
SETTINGS = Path(EXE).parents[2] / "UserData/Settings.cfg"
WORK = Path.home() / ".cache/kontra-native-oracle/run" / str(os.getpid())
DISPLAY = ":78"  # not :77, which the audio reference harness owns


def winpath(p: Path) -> str:
    return subprocess.check_output(["winepath", "-w", str(p)], text=True).strip()


# Modal startup dialogs that stop the Lua script from running: (window title, key).
# Mouse input only reaches windows inside a Wine virtual desktop, so each dialog is
# dismissed by clicking its button at a position relative to the dialog.
#   (title regex, max dialog width, x, y of the button relative to the dialog)
DIALOGS = [
    ("^Audio and MIDI Settings$", 2000, 823, 470),  # OK
    ("^Kontakt 8$", 700, None, 125),                # "library not installed" etc.: centred OK
]


def dismiss_dialogs(env: dict) -> None:
    for title, max_width, x, y in DIALOGS:
        found = subprocess.run(["xdotool", "search", "--name", title], env=env,
                               capture_output=True, text=True).stdout.split()
        for window in found:
            geo = subprocess.run(["xdotool", "getwindowgeometry", window], env=env,
                                 capture_output=True, text=True).stdout
            size = next((l.split()[1] for l in geo.splitlines() if "Geometry" in l), "0x0")
            width = int(size.split("x")[0])
            if width >= max_width or width == 0:
                continue
            subprocess.run(["xdotool", "mousemove", "--window", window, str(x if x is not None else width // 2),
                            str(y), "click", "1"], env=env, check=False, capture_output=True)


def lua_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main() -> int:
    if len(sys.argv) < 2 or (len(sys.argv) < 3 and "ORACLE_LUA" not in os.environ):
        print(__doc__, file=sys.stderr)
        return 2
    out = Path(sys.argv[1]).resolve()
    inputs = [Path(p).resolve() for p in sys.argv[2:]]
    if b"DeveloperFeatures=dw:1" not in SETTINGS.read_bytes():
        sys.exit(f"enable Options > Developer first: DeveloperFeatures=dw:1 in {SETTINGS}")
    if subprocess.run(["pgrep", "-f", "[K]ontakt 8.exe"], capture_output=True).returncode == 0:
        sys.exit("another Kontakt is running; one oracle session at a time")
    WORK.mkdir(parents=True, exist_ok=True)
    raw = WORK / "oracle.jsonl"
    raw.unlink(missing_ok=True)
    script = WORK / "oracle.lua"
    script.write_text(
        "ORACLE_INPUTS = {" + ",".join(lua_str(winpath(p)) for p in inputs) + "}\n"
        + "ORACLE_OUT = " + lua_str(winpath(raw)) + "\n"
        + Path(os.environ.get("ORACLE_LUA", HERE / "dump.lua")).read_text())

    xvfb = subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "1600x1000x24"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    env = dict(os.environ, DISPLAY=DISPLAY, WINEDLLOVERRIDES="d3d11,d3d10core,dxgi,d3d9=b",
               WINEDEBUG="-all")
    log = open(WORK / "kontakt.log", "w")
    kontakt = subprocess.Popen(["wine", "explorer", "/desktop=oracle,1600x1000", EXE, winpath(script)],
                               env=env, stdout=log, stderr=subprocess.STDOUT,
                               start_new_session=True)
    deadline = time.time() + float(os.environ.get("ORACLE_TIMEOUT", 120 + 60 * len(inputs)))
    done = False
    try:
        while time.time() < deadline:
            if raw.exists() and raw.read_text(errors="replace").rstrip().endswith('{"done":true}'):
                done = True
                break
            dismiss_dialogs(env)
            time.sleep(2)
        else:
            subprocess.run(["import", "-window", "root", str(WORK / "timeout.png")], env=env, check=False)
    finally:
        subprocess.run(["pkill", "-f", "[K]ontakt 8.exe"], check=False)
        time.sleep(2)
        os.killpg(kontakt.pid, signal.SIGKILL) if kontakt.poll() is None else None
        xvfb.terminate()
    if not raw.exists():
        sys.exit(f"no oracle output; see {WORK / 'kontakt.log'}")
    # Dialogs dismissed above leave Kontakt's own error in the record (load_error).
    records = [json.loads(line) for line in raw.read_text().splitlines() if line.strip()]
    records = [r for r in records if not r.get("done")]
    out.write_text("".join(json.dumps(r, sort_keys=True) + "\n" for r in records))
    print(f"{len(records)} records for {len(inputs)} inputs -> {out}" + ("" if done else " (TIMED OUT)"))
    return 0 if done else 1


if __name__ == "__main__":
    sys.exit(main())
