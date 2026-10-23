"""Oracle slot pool: several real-Kontakt sessions at once, one per slot.

`tools/oracle/oracle-slot CMD...` runs CMD in a free slot and exports (see that script):
  ORACLE_SLOT (1..ORACLE_SLOTS)   ORACLE_PREFIX (this slot's Wine prefix)   ORACLE_DISPLAY (:7N)
  ORACLE_SINK (PipeWire null sink kontra_ref_N)   ORACLE_WORK (scratch dir for this slot)

Unset ORACLE_SLOT is the single-session setup every tool had before: default Wine prefix, the
tool's own display, sink kontra_ref, one Kontakt per machine (the caller holds the legacy lock).

All slots share ALSA "Midi Through", so every slot owns a block of MIDI channels (channel_block):
an instrument in slot N only hears notes on its own channels, never another slot's.
"""
import json
import os
import signal
import subprocess
import time
from pathlib import Path

MIDI_CHANNELS = 16
KONTAKT_NAME = "Kontakt 8.exe"
KONTAKT_REL = Path("drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe")
DEFAULT_PREFIX = Path.home() / ".wine"


def active():
    """True inside oracle-slot (a slot, not the legacy single session)."""
    return bool(os.environ.get("ORACLE_SLOT"))


def display(default):
    return os.environ.get("ORACLE_DISPLAY") or default


def sink(default):
    return os.environ.get("ORACLE_SINK") or default


def work(default):
    return Path(os.environ["ORACLE_WORK"]) if os.environ.get("ORACLE_WORK") else Path(default)


def prefix():
    """This slot's Wine prefix, or the default prefix in legacy mode."""
    return Path(os.environ["ORACLE_PREFIX"]) if active() else DEFAULT_PREFIX


def exe_default():
    return str(prefix() / KONTAKT_REL)


def wine_env(env):
    """Adds the prefix this session runs in (Kontakt's own WINEPREFIX is what kontakt_pids() matches)."""
    env["WINEPREFIX"] = str(prefix())
    return env


def channel_block():
    """(first, count) of 0-based MIDI channels this session may use. Legacy: all 16 from 0."""
    if not active():
        return 0, MIDI_CHANNELS
    per = MIDI_CHANNELS // int(os.environ.get("ORACLE_SLOTS", "3"))
    return (int(os.environ["ORACLE_SLOT"]) - 1) * per, per


def kontakt_pids():
    """Kontakt processes of this session: this slot's prefix only. Legacy: every Kontakt."""
    if not active():
        out = subprocess.run(["pgrep", "-x", KONTAKT_NAME], capture_output=True, text=True).stdout
        return [int(p) for p in out.split()]
    want = f"WINEPREFIX={prefix()}".encode()
    pids = []
    for d in Path("/proc").iterdir():
        if not d.name.isdigit():
            continue
        try:
            if (d / "comm").read_text().strip() == KONTAKT_NAME and want in (d / "environ").read_bytes().split(b"\0"):
                pids.append(int(d.name))
        except OSError:  # process ended while we looked
            pass
    return pids


def stop_kontakt(timeout=20):
    """Ends this session's Kontakt (SIGTERM, then SIGKILL). In a slot, also stops the slot's wineserver,
    which kills what is left of that prefix only (never another slot or the default prefix)."""
    for pid in kontakt_pids():
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass
    end = time.monotonic() + timeout
    while kontakt_pids() and time.monotonic() < end:
        time.sleep(0.5)
    for pid in kontakt_pids():
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass
    if active():
        subprocess.run(["wineserver", "-k"], env=wine_env(dict(os.environ)), capture_output=True)


def ensure_sink(name):
    """Creates the PipeWire null sink `name` when missing. Exact name match (kontra_ref is a prefix of kontra_ref_1)."""
    rows = subprocess.run(["pactl", "list", "short", "sinks"], capture_output=True, text=True).stdout.splitlines()
    if any(r.split()[1:2] == [name] for r in rows):
        return
    subprocess.run(["pactl", "load-module", "module-null-sink", f"sink_name={name}",
                    f"sink_properties=device.description={name}", "format=float32le", "rate=48000", "channels=2"],
                   capture_output=True)


def _pw_dump():
    return json.loads(subprocess.run(["pw-dump"], capture_output=True, text=True).stdout or "[]")


def route_slot(sink_name, pids, wait=0):
    """Links the outputs of the Kontakt nodes owned by `pids` to `sink_name` only: any other link
    from them is removed (PipeWire re-links new ports to the default output). Ownership comes from
    pw-dump (application.process.id), so another slot's Kontakt ports are never touched. Returns the
    number of this session's output ports (0 = not there yet; waits up to `wait` seconds for them)."""
    deadline = time.monotonic() + wait
    owned = {str(p) for p in pids}
    while True:
        objs = _pw_dump()
        nodes = {o["id"]: o["info"]["props"] for o in objs if o.get("type") == "PipeWire:Interface:Node"}
        mine = {i for i, p in nodes.items() if str(p.get("application.process.id")) in owned}
        sink_node = next((i for i, p in nodes.items() if p.get("node.name") == sink_name), None)
        ports = {o["id"]: o for o in objs if o.get("type") == "PipeWire:Interface:Port"}
        out = {i: o for i, o in ports.items()
               if o["info"].get("direction") == "output" and o["info"]["props"].get("node.id") in mine}
        if out or time.monotonic() >= deadline or sink_node is None:
            break
        time.sleep(0.5)
    if not out or sink_node is None:
        return len(out)
    sink_in = {o["info"]["props"].get("port.name"): i for i, o in ports.items()
               if o["info"].get("direction") == "input" and o["info"]["props"].get("node.id") == sink_node}
    linked = set()
    for link in (o for o in objs if o.get("type") == "PipeWire:Interface:Link"):
        li = link["info"]
        if li.get("output-port-id") not in out:
            continue
        if li.get("input-node-id") == sink_node:
            linked.add(li["output-port-id"])
        else:
            subprocess.run(["pw-cli", "destroy", str(link["id"])], capture_output=True)
    for i, o in out.items():
        if i in linked:
            continue
        target = sink_in.get("playback_" + o["info"]["props"].get("port.name", "").split("_")[-1])
        if target is not None:
            subprocess.run(["pw-link", str(i), str(target)], capture_output=True)
    return len(out)
