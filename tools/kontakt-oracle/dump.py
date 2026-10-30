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
sys.path.insert(0, str(HERE.parent / "oracle"))
import slot  # noqa: E402  (oracle-slot: prefix, display and work dir of this session; legacy when unset)

EXE = os.environ.get("KONTAKT_EXE") or slot.exe_default()
SETTINGS = Path(EXE).parents[2] / "UserData/Settings.cfg"
WORK = slot.work(Path.home() / ".cache/kontra-native-oracle") / "run" / str(os.getpid())
DISPLAY = slot.display(":78")  # not :77, which the audio reference harness owns


def winpath(p: Path) -> str:
    return subprocess.check_output(["winepath", "-w", str(p)], text=True).strip()


# Modal startup dialogs that stop the Lua script from running: (window title, key).
# Mouse input only reaches windows inside a Wine virtual desktop, so each dialog is
# dismissed by clicking its button, at a position taken from the dialog's own size
# rather than a fixed point: the Audio/MIDI dialog is 594x302 here, and the fixed
# x=823 this used to click was outside it, so the click never landed.
#   (title regex, max dialog width, button offsets from the bottom-right corner)
DIALOGS = [
    ("^Audio and MIDI Settings$", 2000, [(-45, -22), (-45, -45), (-90, -22)]),  # OK
    ("^Kontakt 8$", 700, [(-45, -22), (-90, -22), (0, -125)]),                  # centred OK
]


def window_geometry(env: dict, window: str) -> tuple[int, int]:
    geo = subprocess.run(["xdotool", "getwindowgeometry", window], env=env,
                         capture_output=True, text=True).stdout
    size = next((l.split()[1] for l in geo.splitlines() if "Geometry" in l), "0x0")
    width, _, height = size.partition("x")
    return int(width or 0), int(height or 0)


def dismiss_dialogs(env: dict) -> None:
    for title, max_width, offsets in DIALOGS:
        found = subprocess.run(["xdotool", "search", "--name", title], env=env,
                               capture_output=True, text=True).stdout.split()
        for window in found:
            width, height = window_geometry(env, window)
            if width >= max_width or width == 0:
                continue
            for dx, dy in offsets:
                x = width + dx if dx < 0 else width // 2 + dx
                y = height + dy
                subprocess.run(["xdotool", "mousemove", "--window", window,
                                str(max(x, 1)), str(max(y, 1)), "click", "1"],
                               env=env, check=False, capture_output=True)
                # The dialog is gone once it no longer answers to its title.
                gone = not subprocess.run(["xdotool", "search", "--name", title], env=env,
                                          capture_output=True, text=True).stdout.split()
                if gone:
                    break



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
    if slot.kontakt_pids():
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
    env = slot.wine_env(dict(os.environ, DISPLAY=DISPLAY, WINEDEBUG=os.environ.get("ORACLE_WINEDEBUG", "-all")))
    # Forcing wine's own d3d makes Kontakt exit silently on a headless Xvfb: it
    # starts, touches GL, and dies with no window and a 2-line log (measured
    # 2026-10-29, wine 11.16). ORACLE_DLL_OVERRIDES="" leaves the prefix's
    # DXVK in place, which is what works there.
    overrides = os.environ.get("ORACLE_DLL_OVERRIDES", "d3d11,d3d10core,dxgi,d3d9=b")
    if overrides:
        env["WINEDLLOVERRIDES"] = overrides
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
        slot.stop_kontakt()
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
