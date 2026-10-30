#!/usr/bin/env python3
"""oracle-mcp: a stdio MCP server that drives REAL Kontakt 8 headless and measures it.

One long-lived Kontakt per server process: `oracle_up` launches it on a private
Xvfb (DXVK + lavapipe, openbox), every other tool reuses it, `oracle_down`
stops exactly the processes this process started.

Protocol: JSON-RPC 2.0, newline-delimited, on stdin/stdout (initialize ->
notifications/initialized -> tools/list / tools/call), same shape as the other
MCP servers on this machine. Nothing is written to stdout except responses;
diagnostics go to the log file announced on stderr at startup.

Safety rules, enforced in code (see docstring of oracle-mcp/README.md):
  * only displays :77 and :9x, and only ones this process created;
  * never DISPLAY=:0, never the user's desktop or windows;
  * screenshots are of the private display's root only, cropped to the
    Kontakt window;
  * processes are stopped by PID / process group of the PID we started,
    never by pattern (`pkill -f` has killed the caller's shell before);
  * every returned measurement is measured; a tool that cannot measure
    fails instead of returning a plausible default.
"""
from __future__ import annotations

import array
import hashlib
import json
import math
import os
import re
import shutil
import signal
import struct
import subprocess
import sys
import time
import traceback
from pathlib import Path

VERSION = "0.1.0"

# --------------------------------------------------------------------------- config
HOME = Path.home()
WINE = Path(os.environ.get("ORACLE_MCP_WINE", "/opt/wine-staging/bin/wine"))
PREFIX = Path(os.environ.get("ORACLE_MCP_PREFIX", HOME / "Armory/.oracle-spike/prefix-dxvk"))
PREFIX_SOURCE = Path(os.environ.get("ORACLE_MCP_PREFIX_SOURCE", HOME / "Armory/WINE"))
LIVE_PREFIX = HOME / "Armory/WINE"  # the user's live prefix (REAPER runs in it) - never ours
PORTABLE = Path(os.environ.get("ORACLE_MCP_PORTABLE", HOME / "Armory/VSTs/Kontakt 8.14.1 Portable"))
EXEDIR = PORTABLE / "Kontakt 8/x64"
EXE = EXEDIR / "Kontakt 8.exe"
SETTINGS = PORTABLE / "UserData/Settings.cfg"
ICD = Path("/usr/share/vulkan/icd.d/lvp_icd.x86_64.json")
DLL_OVERRIDES = "d3d11,d3d10core,dxgi,d3d9=n"
SINK = os.environ.get("ORACLE_MCP_SINK", "kontra_ref")
MIDI_DEVICE_NAME = "Midi Through Port-0"  # what Kontakt's Settings.cfg must name
XVFB_ARGS = ["-screen", "0", "1600x1000x24", "+extension", "GLX", "+extension", "RANDR",
             "+render", "-noreset", "-nolisten", "tcp"]
WINDOW_NAME = "^Kontakt 8$"

MAIN_DISPLAYS = [":77", ":94", ":95", ":96", ":97"]
PROBE_DISPLAYS = [":78", ":98", ":99"]
ALLOWED_DISPLAYS = set(MAIN_DISPLAYS) | set(PROBE_DISPLAYS)

WORK = Path(os.environ.get("ORACLE_MCP_WORK", "/tmp/opencode/oracle-mcp"))
STATE = WORK / "session.json"  # what this server started, so a dead server's leftovers can be reaped
LOG_PATH = Path(os.environ.get("ORACLE_MCP_LOG", WORK / "server.log"))
VERBOSE = os.environ.get("ORACLE_MCP_VERBOSE", "") not in ("", "0", "false")
LOG_MAX = 4 * 1024 * 1024

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
DUMP_LUA = REPO / "tools/kontakt-oracle/dump.lua"
KSP_PROBE = HERE / "ksp_text.lua"

# The instrument page's on-screen keyboard strip is flat in the browser and richly
# coloured once an instrument page is up (measured: browser sd 0.023 / 2 colours,
# instrument sd 0.318 / 199 colours).
KEYBOARD_STRIP = (380, 620, 600, 60)  # x, y, w, h in window-relative pixels
PAGE_SD_MIN = 0.10
PAGE_COLOURS_MIN = 40
UI_SD_MIN = 0.05
UI_COLOURS_MIN = 200


# --------------------------------------------------------------------------- logging
_log_file = None


def log(msg: str, level: str = "info") -> None:
    """Append one line, flushed immediately (a log saved only at exit is useless)."""
    global _log_file
    line = f"{time.strftime('%Y-%m-%d %H:%M:%S')} [{level}] {msg}\n"
    try:
        if _log_file is None:
            LOG_PATH.parent.mkdir(parents=True, exist_ok=True)
            _log_file = open(LOG_PATH, "a", buffering=1, encoding="utf-8", errors="replace")
        _log_file.write(line)
        _log_file.flush()
    except OSError:
        pass
    if level in ("error", "warn") or VERBOSE:
        sys.stderr.write(f"oracle-mcp: {msg}\n")
        sys.stderr.flush()


def rotate_log() -> None:
    try:
        if LOG_PATH.exists() and LOG_PATH.stat().st_size > LOG_MAX:
            LOG_PATH.replace(LOG_PATH.with_suffix(".log.1"))
    except OSError:
        pass


# --------------------------------------------------------------------------- errors
class ToolError(Exception):
    """A tool could not do or measure what it was asked. Reported as isError."""


class RpcError(Exception):
    def __init__(self, code: int, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


# --------------------------------------------------------------------------- shell
def base_env(display: str | None = None) -> dict:
    env = {k: v for k, v in os.environ.items()
           if k not in ("DISPLAY", "WINEPREFIX", "VK_ICD_FILENAMES", "WINEDLLOVERRIDES")}
    env["PATH"] = os.environ.get("PATH", "/usr/local/bin:/usr/bin:/bin")
    if display is not None:
        env["DISPLAY"] = display
    return env


def guard_display(display: str) -> None:
    if display not in ALLOWED_DISPLAYS:
        raise ToolError(
            f"refusing display {display!r}: this server only ever uses {sorted(ALLOWED_DISPLAYS)} "
            f"and never :0 (the user is working there)")
    if display in (":0",) or display.startswith(":0."):
        raise ToolError("refusing the user's desktop (:0)")


def sh(cmd, env=None, timeout=120, check=False, stdin_text=None) -> subprocess.CompletedProcess:
    p = subprocess.run([str(c) for c in cmd], capture_output=True, text=True,
                       env=env, timeout=timeout, input=stdin_text)
    if check and p.returncode != 0:
        raise ToolError(f"{' '.join(str(c) for c in cmd[:3])} failed rc={p.returncode}: "
                        f"{(p.stderr or p.stdout).strip()[:400]}")
    return p


def xd(display: str, *args, timeout=30) -> subprocess.CompletedProcess:
    guard_display(display)
    return sh(["xdotool", *args], env=base_env(display), timeout=timeout)


def display_up(display: str) -> bool:
    guard_display(display)
    return sh(["xdpyinfo", "-display", display, ], env=base_env(), timeout=15).returncode == 0


def clear_stale_lock(display: str) -> bool:
    """A stale /tmp/.X<n>-lock silently breaks a run: check the server, not the file."""
    num = display[1:]
    lock = Path(f"/tmp/.X{num}-lock")
    sock = Path(f"/tmp/.X11-unix/X{num}")
    if display_up(display):
        return False
    if not lock.exists() and not sock.exists():
        return False
    pid = None
    try:
        pid = int(lock.read_text().strip().split()[0])
    except (OSError, ValueError, IndexError):
        pass
    if pid is not None:
        try:
            os.kill(pid, 0)
            log(f"{lock} names live pid {pid} but {display} does not answer; leaving it alone", "warn")
            return False
        except OSError:
            pass
    for p in (lock, sock):
        try:
            p.unlink()
            log(f"removed stale {p} (no server answered {display})", "warn")
        except OSError:
            pass
    return True


def free_display(candidates) -> str:
    for d in candidates:
        guard_display(d)
        clear_stale_lock(d)
        if not display_up(d):
            return d
    raise ToolError(f"no free private display among {list(candidates)} — another oracle session "
                    f"is probably running; call oracle_down first")


# --------------------------------------------------------------------------- session
class Session:
    def __init__(self) -> None:
        self.display: str | None = None
        self.probe_display: str | None = None
        self.xvfb: subprocess.Popen | None = None
        self.openbox: subprocess.Popen | None = None
        self.wine: subprocess.Popen | None = None
        self.wine_log: Path | None = None
        self.window: int | None = None
        self.kontakt_pids: list[int] = []
        self.settings_changed: list[str] = []
        self.started_at: float | None = None
        self.instrument: str | None = None
        self.run_dir: Path | None = None
        self.counters = {"shot": 0, "click": 0, "render": 0}

    # -- lifecycle ---------------------------------------------------------
    @property
    def up(self) -> bool:
        return self.wine is not None and self.window is not None and self.wine.poll() is None

    def require_up(self, tool: str) -> None:
        if not self.up:
            raise ToolError(
                f"{tool}: no Kontakt session. Call oracle_up first "
                f"(state: launched={self.wine is not None} window={self.window} "
                f"wine_exit={None if self.wine is None else self.wine.poll()})")

    def env(self) -> dict:
        if self.display is None:
            raise ToolError("no display: the session is not up")
        guard_display(self.display)
        env = base_env(self.display)
        env.update({"WINEPREFIX": str(PREFIX), "VK_ICD_FILENAMES": str(ICD),
                    "WINEDLLOVERRIDES": DLL_OVERRIDES, "WINEDEBUG": "-all"})
        return env

    def addr(self) -> Path:
        if self.run_dir is None:
            raise ToolError("session work dir missing")
        return self.run_dir


SESSION = Session()


# --------------------------------------------------------------------------- helpers
def preflight() -> None:
    """Things that must exist before anything can work. Fails loudly, names each gap."""
    missing = [str(p) for p in (WINE, EXE, ICD) if not p.exists()]
    for tool in ("Xvfb", "openbox", "xdotool", "import", "magick", "xdpyinfo", "winepath"):
        if shutil.which(tool) is None:
            missing.append(tool)
    if missing:
        raise ToolError("missing prerequisites: " + ", ".join(missing))
    if PREFIX.resolve() == LIVE_PREFIX.resolve():
        raise ToolError(f"refusing to use the user's live prefix {LIVE_PREFIX} "
                        f"(REAPER runs there); set ORACLE_MCP_PREFIX to a reflinked copy")


def ensure_prefix() -> list[str]:
    """The private prefix. Created as a reflink copy of the source once; recorded."""
    notes = []
    if PREFIX.exists():
        return [f"prefix present: {PREFIX}"]
    preflight()
    if PREFIX_SOURCE.resolve() == LIVE_PREFIX.resolve():
        PREFIX.parent.mkdir(parents=True, exist_ok=True)
        t0 = time.monotonic()
        p = sh(["cp", "-a", "--reflink=auto", str(PREFIX_SOURCE), str(PREFIX)], timeout=1800)
        if p.returncode != 0:
            raise ToolError(f"could not copy the prefix: {p.stderr.strip()[:400]}")
        notes.append(f"prefix created by reflink copy of {PREFIX_SOURCE} in "
                     f"{time.monotonic() - t0:.1f}s")
    else:
        raise ToolError(f"prefix {PREFIX} is missing and its source {PREFIX_SOURCE} is not the "
                        f"live prefix; refusing to copy from an unknown source")
    return notes


def ensure_settings() -> list[str]:
    """Kontakt's own config must name our sink and the ALSA Midi Through port, or a
    launch would render to the user's speakers or hear no MIDI. Written once, backed up."""
    changed = []
    if not SETTINGS.exists():
        raise ToolError(f"{SETTINGS} does not exist: is the portable Kontakt complete?")
    text = SETTINGS.read_text(errors="surrogateescape")
    orig = text
    want_audio = f"AB2 AudioDevice=sz:WASAPI (Shared Mode) {SINK}"
    if want_audio not in text:
        if re.search(r"(?m)^AB2 AudioDevice=", text):
            text = re.sub(r"(?m)^AB2 AudioDevice=.*$", want_audio, text)
        else:
            # a key outside its section is not read: put it right under [Kontakt Application]
            head = "[Kontakt Application]\n"
            text = (text.replace(head, head + want_audio + "\n", 1)
                    if head in text else want_audio + "\n" + text)
        text = re.sub(r"(?m)^AB2 OutputChannel [^\n]* 0=.*$",
                      f"AB2 OutputChannel WASAPI (Shared Mode) {SINK} 0=dw:0", text)
        text = re.sub(r"(?m)^AB2 OutputChannel [^\n]* 1=.*$",
                      f"AB2 OutputChannel WASAPI (Shared Mode) {SINK} 1=dw:1", text)
        changed.append(f"AB2 AudioDevice -> WASAPI (Shared Mode) {SINK}")
    if f"AB2 MidiDevice0 Name=sz:{MIDI_DEVICE_NAME}" not in text:
        text = re.sub(r"(?m)^AB2 MidiDevice0 .*$\n?", "", text)
        text = text.rstrip("\n") + (
            f"\nAB2 MidiDevice0 Name=sz:{MIDI_DEVICE_NAME}"
            f"\nAB2 MidiDevice0 UID=sz:WinMidi Input {MIDI_DEVICE_NAME}"
            f"\nAB2 MidiDevice0 PortID=dw:0\n")
        changed.append(f"AB2 MidiDevice0 -> {MIDI_DEVICE_NAME} (Port A)")
    if text != orig:
        backup = SETTINGS.with_suffix(".cfg.pre-oracle-mcp")
        if not backup.exists():
            backup.write_bytes(orig.encode("utf-8", "surrogateescape"))
        SETTINGS.write_bytes(text.encode("utf-8", "surrogateescape"))
        log(f"settings written: {changed} (backup {backup.name})")
    return changed


def sink_state() -> str | None:
    p = sh(["pactl", "list", "short", "sinks"], env=base_env())
    for line in p.stdout.splitlines():
        fields = line.split("\t")
        if len(fields) > 1 and fields[1] == SINK:
            return fields[-1].split()[-1] if len(fields) > 4 else "UNKNOWN"
    return None


def ensure_sink() -> dict:
    """capture.py's recipe: a suspended null sink records silence even with audio linked."""
    state = sink_state()
    reloaded = False
    if state in (None, "SUSPENDED"):
        for line in sh(["pactl", "list", "short", "modules"], env=base_env()).stdout.splitlines():
            if f"sink_name={SINK}" in line:
                sh(["pactl", "unload-module", line.split("\t")[0]], env=base_env())
        p = sh(["pactl", "load-module", "module-null-sink", f"sink_name={SINK}",
                f"sink_properties=device.description={SINK} node.always-process=true "
                f"session.suspend-timeout-seconds=0", "format=float32le", "rate=48000",
                "channels=2"], env=base_env())
        if p.returncode != 0:
            raise ToolError(f"could not create the null sink {SINK}: {p.stderr.strip()[:300]}")
        reloaded = True
        state = sink_state()
    if state in (None, "SUSPENDED"):
        raise ToolError(f"null sink {SINK} is {state} after reload; check `pactl list sinks`")
    return {"sink": SINK, "state": state, "reloaded": reloaded}


def midi_port() -> int:
    """The ALSA sequencer port Kontakt's Settings.cfg listens on (Midi Through Port-0)."""
    p = sh(["aplaymidi", "-l"], env=base_env())
    for line in p.stdout.splitlines():
        if MIDI_DEVICE_NAME in line:
            m = re.match(r"\s*(\d+):(\d+)", line)
            if m:
                return int(m.group(1))
    raise ToolError(f"ALSA sequencer has no {MIDI_DEVICE_NAME!r} port; `aplaymidi -l` said:\n"
                    f"{p.stdout.strip()[:400]}")


def winpath(p: Path) -> str:
    """Kontakt wants a DOS path. Every path we handle is under / (drive Z:)."""
    s = str(Path(p).resolve())
    return "Z:" + s.replace("/", "\\")


def find_window() -> int | None:
    assert SESSION.display
    p = xd(SESSION.display, "search", "--onlyvisible", "--name", WINDOW_NAME)
    ids = [int(x) for x in p.stdout.split() if x.strip()]
    return ids[0] if ids else None


def geometry(window: int) -> dict:
    p = xd(SESSION.display, "getwindowgeometry", "--shell", str(window))
    d = {}
    for line in p.stdout.splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            d[k] = v
    return {"window": window, "x": int(d["X"]), "y": int(d["Y"]),
            "w": int(d["WIDTH"]), "h": int(d["HEIGHT"])}


def capture_root(path: Path) -> Path:
    """The ONLY screenshot this server ever takes: the private display's root."""
    assert SESSION.display
    guard_display(SESSION.display)
    path.parent.mkdir(parents=True, exist_ok=True)
    p = sh(["import", "-window", "root", str(path)], env=base_env(SESSION.display), timeout=60)
    if p.returncode != 0 or not path.exists():
        raise ToolError(f"screenshot failed: {p.stderr.strip()[:300]}")
    return path


def crop(src: Path, dst: Path, g: dict) -> Path:
    p = sh(["magick", str(src), "-crop", f"{g['w']}x{g['h']}+{g['x']}+{g['y']}", "+repage", str(dst)],
           env=base_env(), timeout=60)
    if p.returncode != 0 or not dst.exists():
        raise ToolError(f"crop failed: {p.stderr.strip()[:300]}")
    return dst


def stats(path: Path, region: tuple[int, int, int, int] | None = None) -> dict:
    args = ["magick", str(path)]
    if region:
        x, y, w, h = region
        args += ["-crop", f"{w}x{h}+{x}+{y}", "+repage"]
    args += ["-format", "%[fx:standard_deviation] %k %w %h", "info:"]
    p = sh(args, env=base_env(), timeout=60)
    if p.returncode != 0:
        raise ToolError(f"image stats failed: {p.stderr.strip()[:300]}")
    sd, colours, w, h = p.stdout.split()
    return {"sd": round(float(sd), 6), "colours": int(colours), "w": int(w), "h": int(h)}


def changed_pixels(before: Path, after: Path) -> int:
    """Mean-absolute-difference x pixel count between two crops (0 == identical)."""
    p = sh(["magick",
            "(", str(before), "-alpha", "off", "-colorspace", "sRGB", "-depth", "8", ")",
            "(", str(after), "-alpha", "off", "-colorspace", "sRGB", "-depth", "8", ")",
            "-compose", "difference", "-composite",
            "-format", "%[fx:int(mean*w*h+0.5)]", "info:"], env=base_env(), timeout=120)
    if p.returncode != 0:
        raise ToolError(f"pixel diff failed: {p.stderr.strip()[:300]}")
    return int(p.stdout.strip())


def strip_region(g: dict) -> tuple[int, int, int, int]:
    """The keyboard strip, clamped to the window, so the metric is always the region used."""
    x, y, w, h = KEYBOARD_STRIP
    w = max(1, min(w, g["w"] - x))
    h = max(1, min(h, g["h"] - y))
    return x, y, w, h


def page_verdict(client_stats: dict, keyboard_stats: dict) -> dict:
    ui = client_stats["sd"] >= UI_SD_MIN and client_stats["colours"] >= UI_COLOURS_MIN
    page = keyboard_stats["sd"] > PAGE_SD_MIN and keyboard_stats["colours"] > PAGE_COLOURS_MIN
    return {"ui_rendered": ui, "instrument_page": page,
            "mode": "instrument" if page else ("browser" if ui else "blank")}


def read_log_tail(path: Path, n: int = 12) -> str:
    try:
        lines = [l for l in path.read_text(errors="replace").splitlines() if l.strip()]
    except OSError:
        return ""
    keep = [l for l in lines if re.search(r"err:|XIO|c0000|Failed|error", l, re.I)] or lines[-n:]
    return "\n".join(keep[-n:])


def renderer_lines(log_path: Path) -> dict:
    text = ""
    try:
        text = log_path.read_text(errors="replace")
    except OSError:
        pass
    dxvk = re.search(r"DXVK: v[^\s|]+", text)
    device = re.search(r"Found device: [^)\n]*\)?", text)
    return {"dxvk": dxvk.group(0) if dxvk else None,
            "device": device.group(0).strip() if device else None}


# --------------------------------------------------------------------------- wav / midi
def read_wav(path: Path) -> tuple[int, int, list]:
    """Float WAV reader (same layout pw-record writes): (rate, channels, per-channel arrays)."""
    data = path.read_bytes()
    if data[:4] != b"RIFF":
        raise ToolError(f"{path}: not a RIFF file")
    pos, ch, rate, fmt = 12, 2, 48000, 3
    while pos + 8 <= len(data):
        tag, size = data[pos:pos + 4], int.from_bytes(data[pos + 4:pos + 8], "little")
        body = data[pos + 8:pos + 8 + size]
        if tag == b"fmt ":
            fmt, ch, rate = struct.unpack("<HHI", body[:8])
        if tag == b"data":
            size = min(size, len(data) - pos - 8)
            a = array.array("f")
            a.frombytes(data[pos + 8:pos + 8 + size // 4 * 4])
            return rate, ch, [a[i::ch] for i in range(ch)]
        pos += 8 + size + (size & 1)
    raise ToolError(f"{path}: no data chunk")


def write_wav_f32(path: Path, channels: list, rate: int) -> None:
    """Interleaved float32 stereo WAV (the layout pw-record writes)."""
    n = len(channels[0])
    ch = len(channels)
    frames = array.array("f")
    for i in range(n):
        for c in channels:
            frames.append(c[i])
    payload = frames.tobytes()
    hdr = (b"RIFF" + (36 + len(payload)).to_bytes(4, "little") + b"WAVEfmt "
           + (16).to_bytes(4, "little") + (3).to_bytes(2, "little")
           + ch.to_bytes(2, "little") + rate.to_bytes(4, "little")
           + (rate * ch * 4).to_bytes(4, "little") + (ch * 4).to_bytes(2, "little")
           + (32).to_bytes(2, "little") + b"data" + len(payload).to_bytes(4, "little"))
    path.write_bytes(hdr + payload)


def trim_wav(src: Path, dst: Path, start_s: float, length_s: float | None) -> dict:
    """Cut [start, start+length) out of a float WAV; length None = to the end."""
    rate, ch, chans = read_wav(src)
    n = len(chans[0])
    a = max(0, min(n, int(round(start_s * rate))))
    b = n if length_s is None else max(a, min(n, int(round((start_s + length_s) * rate))))
    if b <= a:
        raise ToolError(f"trim_wav: empty window {start_s}..{start_s}+{length_s} of {n / rate:.3f}s")
    write_wav_f32(dst, [c[a:b] for c in chans], rate)
    return {"start_frame": a, "frames": b - a, "rate": rate}


def wav_stats(path: Path) -> dict:
    rate, ch, chans = read_wav(path)
    n = len(chans[0])
    if n == 0:
        raise ToolError(f"{path}: empty audio")
    peak = 0.0
    energy = 0.0
    onset = None
    threshold = 10 ** (-60 / 20)
    for frame in range(n):
        v = 0.0
        for c in chans:
            s = c[frame]
            if abs(s) > abs(v):
                v = s
        av = abs(v)
        energy += v * v
        if av > peak:
            peak = av
        if onset is None and av > threshold:
            onset = frame
    rms = math.sqrt(energy / n)
    db = lambda x: round(20 * math.log10(x), 3) if x > 0 else None
    return {"sample_rate": rate, "channels": ch, "frames": n,
            "duration_s": round(n / rate, 6),
            "peak": round(peak, 6), "peak_dbfs": db(peak), "rms_dbfs": db(rms),
            "onset_s": None if onset is None else round(onset / rate, 6),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "bytes": path.stat().st_size}


def midi_info(path: Path) -> dict:
    """Length of a standard MIDI file in seconds and its note-on count."""
    data = path.read_bytes()
    if data[:4] != b"MThd":
        raise ToolError(f"{path}: not a standard MIDI file")
    hdr_len = int.from_bytes(data[4:8], "big")
    division = int.from_bytes(data[12:14], "big")
    if division & 0x8000:
        raise ToolError(f"{path}: SMPTE division is not supported")
    pos, ntracks = 8 + hdr_len, int.from_bytes(data[10:12], "big")
    tempo = 500000
    end_tick_total = 0
    notes = 0
    for _ in range(ntracks):
        if data[pos:pos + 4] != b"MTrk":
            raise ToolError(f"{path}: bad track header at {pos}")
        tlen = int.from_bytes(data[pos + 4:pos + 8], "big")
        trk = data[pos + 8:pos + 8 + tlen]
        pos += 8 + tlen
        i, tick, running = 0, 0, None
        while i < len(trk):
            delta = 0
            while True:
                b = trk[i]
                i += 1
                delta = (delta << 7) | (b & 0x7F)
                if not b & 0x80:
                    break
            tick += delta
            status = trk[i]
            if status & 0x80:
                i += 1
                if status < 0xF0:
                    running = status
            else:
                status = running
            if status == 0xFF:
                mtype, n = trk[i], 0
                i += 1
                while True:
                    b = trk[i]
                    i += 1
                    n = (n << 7) | (b & 0x7F)
                    if not b & 0x80:
                        break
                if mtype == 0x51 and n == 3:
                    tempo = (trk[i] << 16) | (trk[i + 1] << 8) | trk[i + 2]
                i += n
            elif status in (0xF0, 0xF7):
                n = 0
                while True:
                    b = trk[i]
                    i += 1
                    n = (n << 7) | (b & 0x7F)
                    if not b & 0x80:
                        break
                i += n
            else:
                if (status & 0xF0) == 0x90 and trk[i + 1] > 0:
                    notes += 1
                i += 1 if (status & 0xF0) in (0xC0, 0xD0) else 2
        end_tick_total = max(end_tick_total, tick)
    return {"duration_s": round(end_tick_total * (tempo / 1_000_000) / division, 6),
            "notes": notes}


def midi_duration(path: Path) -> float:
    return midi_info(path)["duration_s"]


def write_midi(path: Path, notes: list[dict], bpm: float = 120.0) -> dict:
    """notes: [{start_s, length_s, key, velocity}] -> a type-0/1 SMF at 480 ticks per beat."""
    if not notes:
        raise ToolError("notes: an empty note list cannot be rendered")
    division = 480
    seconds_per_tick = (60.0 / bpm) / division
    events = []
    for i, note in enumerate(notes):
        try:
            start = float(note.get("start_s", note.get("start", 0.0)))
            length = float(note.get("length_s", note.get("length", 0.5)))
            key = int(note.get("key", 60))
            vel = int(note.get("velocity", 100))
            chan = int(note.get("channel", 0))
        except (TypeError, ValueError) as e:
            raise ToolError(f"note {i}: {e}")
        if not (0 <= key <= 127 and 0 <= vel <= 127 and 0 <= chan <= 15):
            raise ToolError(f"note {i}: key/velocity must be 0..127 and channel 0..15")
        if length <= 0 or start < 0:
            raise ToolError(f"note {i}: start_s must be >= 0 and length_s > 0")
        events.append((round(start / seconds_per_tick), 1, bytes([0x90 | chan, key, vel])))
        events.append((round((start + length) / seconds_per_tick), 0, bytes([0x80 | chan, key, 0])))
    events.sort(key=lambda e: (e[0], e[1]))  # note-offs before note-ons on the same tick
    body = bytearray()
    tick = 0
    for at, _, ev in events:
        delta = at - tick
        tick = at
        vlq = [delta & 0x7F]
        delta >>= 7
        while delta:
            vlq.insert(0, (delta & 0x7F) | 0x80)
            delta >>= 7
        body += bytes(vlq) + ev
    body += b"\x00\xff\x2f\x00"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1, 0x01, 0xE0])
                     + b"MTrk" + len(body).to_bytes(4, "big") + bytes(body))
    return {"path": str(path), "notes": len(notes), "bpm": bpm,
            "duration_s": midi_duration(path)}


# --------------------------------------------------------------------------- sink routing
def route_to_sink(pids: list[int], wait: float = 20.0) -> dict:
    """Link this Kontakt's outputs to the null sink and drop every other link from them
    (PipeWire re-links new ports to the default output, i.e. the user's speakers)."""
    owned = {str(p) for p in pids}
    deadline = time.monotonic() + wait
    linked = 0
    while True:
        objs = json.loads(sh(["pw-dump"], env=base_env()).stdout or "[]")
        nodes = {o["id"]: (o.get("info") or {}).get("props") or {}
                 for o in objs if o.get("type") == "PipeWire:Interface:Node"}
        mine = {i for i, p in nodes.items() if str(p.get("application.process.id")) in owned}
        sink_node = next((i for i, p in nodes.items() if p.get("node.name") == SINK), None)
        ports = {o["id"]: o for o in objs if o.get("type") == "PipeWire:Interface:Port"}
        out = {i: o for i, o in ports.items()
               if (o.get("info") or {}).get("direction") == "output"
               and ((o["info"].get("props") or {}).get("node.id") in mine)}
        if (out and sink_node is not None) or time.monotonic() >= deadline:
            break
        time.sleep(0.5)
    if not out:
        raise ToolError(f"Kontakt (pids {sorted(owned)}) exposes no audio output port; "
                        f"sink present: {sink_node is not None}")
    if sink_node is None:
        raise ToolError(f"sink {SINK} vanished from PipeWire")
    sink_in = {((o.get("info") or {}).get("props") or {}).get("port.name"): i
               for i, o in ports.items()
               if (o.get("info") or {}).get("direction") == "input"
               and ((o["info"].get("props") or {}).get("node.id") == sink_node)}
    already = set()
    for link in (o for o in objs if o.get("type") == "PipeWire:Interface:Link"):
        li = link.get("info") or {}
        if li.get("output-port-id") not in out:
            continue
        if li.get("input-node-id") == sink_node:
            already.add(li.get("output-port-id"))
        else:
            sh(["pw-cli", "destroy", str(link["id"])], env=base_env())
    for i, o in out.items():
        if i in already:
            continue
        name = (((o.get("info") or {}).get("props") or {}).get("port.name") or "")
        target = sink_in.get("playback_" + name.split("_")[-1])
        if target is not None:
            sh(["pw-link", str(i), str(target)], env=base_env())
            linked += 1
    return {"ports": len(out), "sink_node": sink_node, "new_links": linked,
            "already_linked": sorted(already)}


def kontakt_pids() -> list[int]:
    """Kontakt processes in OUR prefix only (never another session's)."""
    want = f"WINEPREFIX={PREFIX}".encode()
    found = []
    for d in Path("/proc").iterdir():
        if not d.name.isdigit():
            continue
        try:
            if (d / "comm").read_text().strip() != "Kontakt 8.exe":
                continue
            if want in (d / "environ").read_bytes().split(b"\0"):
                found.append(int(d.name))
        except OSError:
            continue
    return sorted(found)


# --------------------------------------------------------------------------- tools
def tool_up(nki: str | None = None) -> dict:
    if SESSION.wine is not None:
        raise ToolError(f"a session is already up (display {SESSION.display}, window "
                        f"{SESSION.window}); call oracle_down first")
    preflight()
    notes = ensure_prefix()
    notes += ensure_settings()
    sink = ensure_sink()
    port = midi_port()

    display = free_display(MAIN_DISPLAYS)
    SESSION.display = display
    notes.append(f"display {display} free (xdpyinfo: no server answering)")

    log(f"starting Xvfb {display}")
    SESSION.xvfb = subprocess.Popen(["Xvfb", display, *XVFB_ARGS], start_new_session=True,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    t0 = time.monotonic()
    while time.monotonic() - t0 < 15:
        if display_up(display):
            break
        time.sleep(0.3)
    if not display_up(display):
        raise ToolError(f"Xvfb {display} did not answer xdpyinfo within 15s "
                        f"(pid {SESSION.xvfb.pid}); see `Xvfb {display}` output")

    SESSION.openbox = subprocess.Popen(["openbox", "--sm-disable"], env=base_env(display),
                                       start_new_session=True, stdout=subprocess.DEVNULL,
                                       stderr=subprocess.DEVNULL)
    t0 = time.monotonic()
    wm = False
    while time.monotonic() - t0 < 10:
        p = sh(["xprop", "-root", "_NET_SUPPORTING_WM_CHECK"], env=base_env(display))
        if "window id" in p.stdout:
            wm = True
            break
        time.sleep(0.3)
    if not wm:
        raise ToolError(f"openbox did not register as a window manager on {display} within 10s "
                        f"(pid {SESSION.openbox.pid})")

    SESSION.run_dir = WORK / time.strftime("run-%Y%m%d-%H%M%S")
    SESSION.run_dir.mkdir(parents=True, exist_ok=True)
    SESSION.wine_log = SESSION.run_dir / "kontakt.log"

    env = SESSION.env()
    launch_t0 = time.monotonic()
    log(f"launching {EXE} on {display} with WINEPREFIX={PREFIX}")
    with open(SESSION.wine_log, "wb") as logf:
        SESSION.wine = subprocess.Popen([str(WINE), str(EXE)], cwd=str(EXEDIR), env=env,
                                        stdout=logf, stderr=subprocess.STDOUT,
                                        start_new_session=True)
    window = None
    t0 = time.monotonic()
    while time.monotonic() - t0 < 90:
        if SESSION.wine.poll() is not None:
            raise ToolError(f"wine exited rc={SESSION.wine.returncode} before a window appeared; "
                            f"log: {read_log_tail(SESSION.wine_log)}")
        window = find_window()
        if window is not None:
            break
        time.sleep(1.0)
    if window is None:
        raise ToolError(f"no 'Kontakt 8' window within 90s (wine pid "
                        f"{SESSION.wine.pid}); log: {read_log_tail(SESSION.wine_log)}")
    launch_s = round(time.monotonic() - launch_t0, 2)
    SESSION.window = window
    SESSION.kontakt_pids = kontakt_pids()
    write_state()
    SESSION.started_at = time.monotonic()

    # focus the app so keyboard input reaches it (measured: activate + click the body)
    focus = "activate"
    xd(display, "search", "--name", "Kontakt 8", "windowactivate", "--sync")
    time.sleep(1.0)
    g = geometry(window)
    xd(display, "mousemove", str(g["x"] + g["w"] // 2), str(g["y"] + g["h"] // 3))
    xd(display, "click", "1")
    time.sleep(1.0)
    xd(display, "key", "Escape")  # dismiss the first-run "What's new" modal
    time.sleep(2.0)

    shot_raw = SESSION.addr() / "up-root.png"
    capture_root(shot_raw)
    client = crop(shot_raw, SESSION.addr() / "up.png", geometry(window))
    cs = stats(client)
    region = strip_region(geometry(window))
    ks = stats(client, region)
    verdict = page_verdict(cs, ks)

    loaded = None
    if nki:
        loaded = tool_load(nki)

    return {
        "state": "up",
        "display": display,
        "prefix": str(PREFIX),
        "exe": str(EXE),
        "wine": {"path": str(WINE), "version": sh([str(WINE), "--version"], env=base_env()).stdout.strip()},
        "config": {"icd": str(ICD), "dll_overrides": DLL_OVERRIDES, "wm": "openbox",
                   "xvfb_args": " ".join(XVFB_ARGS), "sink": SINK, "midi_port": port,
                   "midi_device": MIDI_DEVICE_NAME},
        "renderer": renderer_lines(SESSION.wine_log),
        "pids": {"xvfb": SESSION.xvfb.pid, "openbox": SESSION.openbox.pid,
                 "wine": SESSION.wine.pid, "kontakt": SESSION.kontakt_pids},
        "window": geometry(window),
        "launch_s": launch_s,
        "sink": sink,
        "settings_changed": SESSION.settings_changed,
        "notes": notes,
        "page": {"client": cs, "keyboard_strip": {**ks, "region": region}, **verdict},
        "instrument": SESSION.instrument,
        "loaded": loaded,
        "shot": str(client),
        "work_dir": str(SESSION.addr()),
        "log": str(LOG_PATH),
    }


def write_state() -> None:
    """Record what we started: PIDs only, so a crashed server's session can be reaped by PID."""
    state = {"server_pid": os.getpid(), "display": SESSION.display,
             "prefix": str(PREFIX), "started": time.strftime("%Y-%m-%dT%H:%M:%S"),
             "pids": {name: p.pid for name, p in
                      (("xvfb", SESSION.xvfb), ("openbox", SESSION.openbox),
                       ("wine", SESSION.wine)) if p is not None}}
    try:
        STATE.parent.mkdir(parents=True, exist_ok=True)
        STATE.write_text(json.dumps(state, indent=2))
    except OSError as e:
        log(f"could not write {STATE}: {e}", "warn")


def clear_state() -> None:
    try:
        STATE.unlink()
    except OSError:
        pass


def reap_stale() -> list[str]:
    """Kill what a previous (now dead) server of ours left behind, by recorded PID.

    Only processes whose own environment proves they belong to that session are touched
    (same DISPLAY for Xvfb/openbox, same WINEPREFIX for wine), so a PID reused by another
    program is never killed."""
    notes = []
    if not STATE.exists():
        return notes
    try:
        state = json.loads(STATE.read_text())
    except (OSError, json.JSONDecodeError) as e:
        log(f"unreadable {STATE}: {e}; ignoring", "warn")
        clear_state()
        return notes
    try:
        owner = int(state.get("server_pid") or 0)
    except (TypeError, ValueError):
        owner = 0
    if owner and owner != os.getpid():
        try:
            os.kill(owner, 0)
            return [f"session file belongs to live server {owner}; leaving it alone"]
        except OSError:
            pass
    display = state.get("display")
    prefix = state.get("prefix")
    for name, pid in (state.get("pids") or {}).items():
        try:
            pid = int(pid)
        except (TypeError, ValueError):
            continue
        env = {}
        cmdline = ""
        try:
            for item in Path(f"/proc/{pid}/environ").read_bytes().split(b"\0"):
                if b"=" in item:
                    k, v = item.split(b"=", 1)
                    env[k.decode()] = v.decode(errors="replace")
            cmdline = Path(f"/proc/{pid}/cmdline").read_bytes().replace(b"\0", b" ").decode(
                errors="replace")
        except OSError:
            notes.append(f"{name} {pid}: already gone")
            continue
        # Xvfb carries the display in its argv (it has no DISPLAY), openbox in the
        # environment, wine in WINEPREFIX. A PID reused by something else matches none.
        mine = ((name == "xvfb" and display in cmdline.split())
                or (name != "xvfb" and env.get("DISPLAY") == display)
                or (prefix is not None and env.get("WINEPREFIX") == prefix))
        if not mine:
            notes.append(f"{name} {pid}: not from session {display} (not killed)")
            continue
        try:
            os.killpg(os.getpgid(pid), signal.SIGKILL)
        except OSError:
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass
        notes.append(f"reaped stale {name} {pid} ({display})")
    if display:
        try:
            guard_display(display)
            t0 = time.monotonic()
            while time.monotonic() - t0 < 5 and display_up(display):
                time.sleep(0.3)
            clear_stale_lock(display)
        except ToolError:
            pass
    clear_state()
    for n in notes:
        log(n, "warn")
    return notes


def tool_down() -> dict:
    stopped = []
    if SESSION.wine is None and SESSION.xvfb is None and SESSION.openbox is None:
        return {"state": "down", "stopped": [], "note": "nothing was up"}
    procs = [("wine", SESSION.wine), ("openbox", SESSION.openbox), ("xvfb", SESSION.xvfb)]
    # the process group of each pid we started: wine's Kontakt inherits it, so a
    # group kill reaches exactly the processes launched for that pid (never by pattern)
    for name, p in procs:
        if p is None:
            continue
        try:
            pgid = os.getpgid(p.pid)
        except OSError:
            pgid = p.pid
        try:
            os.killpg(pgid, signal.SIGTERM)
            stopped.append({"name": name, "pid": p.pid, "pgid": pgid, "signal": "SIGTERM"})
        except OSError:
            pass
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline and any(p.poll() is None for _, p in procs if p):
        time.sleep(0.3)
    for name, p in procs:
        if p is None:
            continue
        try:
            pgid = os.getpgid(p.pid)
        except OSError:
            pgid = p.pid
        if p.poll() is None:
            try:
                os.killpg(pgid, signal.SIGKILL)
                stopped.append({"name": name, "pid": p.pid, "pgid": pgid, "signal": "SIGKILL"})
            except OSError:
                pass
    leftovers = kontakt_pids()
    if leftovers:
        # only our prefix, and only after the group kill failed to take them down
        for pid in leftovers:
            try:
                os.kill(pid, signal.SIGKILL)
                stopped.append({"name": "kontakt-leftover", "pid": pid, "signal": "SIGKILL"})
            except OSError:
                pass
        sh(["wineserver", "-k"], env={**base_env(), "WINEPREFIX": str(PREFIX)}, timeout=30)
    if SESSION.display:
        was = SESSION.display
        t0 = time.monotonic()
        while time.monotonic() - t0 < 10 and display_up(was):
            time.sleep(0.3)
        display_gone = not display_up(was)
        if not display_gone:
            clear_stale_lock(was)  # only removes a lock whose pid is dead
    else:
        was, display_gone = None, True
    clear_state()
    out = {"state": "down", "stopped": stopped, "display": was,
           "display_gone": display_gone, "kontakt_leftovers": leftovers}
    SESSION.__init__()  # forget the session; the next oracle_up starts clean
    log(f"down: {out}")
    return out


def tool_load(nki: str) -> dict:
    SESSION.require_up("oracle_load")
    assert SESSION.wine_log
    path = Path(nki).expanduser()
    if not path.is_absolute():
        raise ToolError(f"oracle_load: {nki!r} is not an absolute path")
    if not path.exists():
        raise ToolError(f"oracle_load: {path} does not exist")
    if path.suffix.lower() != ".nki":
        raise ToolError(f"oracle_load: {path} is not an .nki")
    display, window = SESSION.display, SESSION.window
    assert display and window

    xd(display, "search", "--name", "Kontakt 8", "windowactivate", "--sync")
    time.sleep(0.5)
    g = geometry(window)
    xd(display, "mousemove", str(g["x"] + g["w"] // 2), str(g["y"] + g["h"] // 3))
    xd(display, "click", "1")
    time.sleep(0.7)

    def open_dialog() -> int | None:
        xd(display, "key", "ctrl+o")
        t0 = time.monotonic()
        while time.monotonic() - t0 < 8:
            p = xd(display, "search", "--onlyvisible", "--name", "^Load$")
            ids = [int(x) for x in p.stdout.split() if x.strip()]
            if ids:
                return ids[0]
            time.sleep(0.5)
        return None

    dialog = open_dialog()
    if dialog is None:
        # one retry with an explicit focus click (measured: the first Ctrl+O can be lost)
        xd(display, "search", "--name", "Kontakt 8", "windowactivate", "--sync")
        time.sleep(0.5)
        xd(display, "key", "Escape")
        time.sleep(1.0)
        dialog = open_dialog()
    if dialog is None:
        raise ToolError("oracle_load: Ctrl+O produced no 'Load' dialog window — keyboard input "
                        f"is not reaching Kontakt (window {window}); "
                        f"log: {read_log_tail(SESSION.wine_log)}")

    xd(display, "type", "--delay", "8", winpath(path))
    time.sleep(1.5)
    xd(display, "key", "Return")

    # wait for the page to paint: the keyboard strip turns from flat to coloured
    t0 = time.monotonic()
    page = None
    shot = None
    while time.monotonic() - t0 < 60:
        time.sleep(2.0)
        shot = capture_root(SESSION.addr() / "load-root.png")
        g = geometry(window)
        client = crop(shot, SESSION.addr() / f"load-{int(time.monotonic() - t0)}.png", g)
        cs = stats(client)
        ks = stats(client, strip_region(g))
        page = page_verdict(cs, ks)
        if page["instrument_page"]:
            break
        # a stray dialog can swallow the load (e.g. a "missing samples" prompt)
        p = xd(display, "search", "--onlyvisible", "--name", "^Kontakt 8$")
        ids = [int(x) for x in p.stdout.split() if x.strip()]
        if len(ids) > 1:
            xd(display, "key", "Return")
    if page is None or not page["instrument_page"]:
        raise ToolError(
            f"oracle_load: {path.name} never painted an instrument page within 60s "
            f"(client sd={None if page is None else page.get('client')}); "
            f"last shot {shot}; log: {read_log_tail(SESSION.wine_log)}")
    if SESSION.wine and SESSION.wine.poll() is not None:
        raise ToolError(f"oracle_load: wine died (rc={SESSION.wine.returncode}): "
                        f"{read_log_tail(SESSION.wine_log)}")
    SESSION.instrument = str(path)
    return {"nki": str(path), "win_path": winpath(path), "window": g,
            "page": page, "shot": str(client), "strip_region": strip_region(g),
            "renderer": renderer_lines(SESSION.wine_log),
            "load_s": round(time.monotonic() - t0, 2)}


def tool_shot() -> dict:
    SESSION.require_up("oracle_shot")
    display, window = SESSION.display, SESSION.window
    assert display and window
    SESSION.counters["shot"] += 1
    n = SESSION.counters["shot"]
    raw = capture_root(SESSION.addr() / f"shot-{n:03d}-root.png")
    g = geometry(window)
    client = crop(raw, SESSION.addr() / f"shot-{n:03d}.png", g)
    cs = stats(client)
    region = strip_region(g)
    ks = stats(client, region)
    verdict = page_verdict(cs, ks)
    if not verdict["ui_rendered"]:
        raise ToolError(f"oracle_shot: the Kontakt window is blank (sd={cs['sd']}, "
                        f"colours={cs['colours']}); root capture kept at {raw}")
    return {"path": str(client), "root": str(raw), "window": g,
            "bytes": client.stat().st_size,
            "sd": cs["sd"], "colours": cs["colours"],
            "keyboard_strip": {"sd": ks["sd"], "colours": ks["colours"], "region": region},
            **verdict,
            "sha256": hashlib.sha256(client.read_bytes()).hexdigest()}


def tool_click(x, y, settle: float = 2.5) -> dict:
    SESSION.require_up("oracle_click")
    display, window = SESSION.display, SESSION.window
    assert display and window
    try:
        x, y = int(x), int(y)
        settle = float(settle)
    except (TypeError, ValueError):
        raise ToolError("oracle_click: x and y must be integers (window-relative pixels)")
    if settle < 0.5 or settle > 30:
        raise ToolError("oracle_click: settle must be between 0.5 and 30 seconds")
    g = geometry(window)
    if not (0 <= x < g["w"] and 0 <= y < g["h"]):
        # a click outside the window cannot change anything: report it as the measured
        # 0, not as a success
        raise ToolError(f"oracle_click: ({x},{y}) is outside the {g['w']}x{g['h']} Kontakt "
                        f"window — nothing was clicked, changed=0")
    SESSION.counters["click"] += 1
    n = SESSION.counters["click"]
    before = crop(capture_root(SESSION.addr() / f"click-{n:03d}-before-root.png"),
                  SESSION.addr() / f"click-{n:03d}-before.png", g)
    xd(display, "mousemove", str(g["x"] + x), str(g["y"] + y))
    time.sleep(0.4)
    xd(display, "click", "1")
    time.sleep(settle)
    after = crop(capture_root(SESSION.addr() / f"click-{n:03d}-after-root.png"),
                 SESSION.addr() / f"click-{n:03d}-after.png", g)
    changed = changed_pixels(before, after)
    out = {"x": x, "y": y, "abs": [g["x"] + x, g["y"] + y], "changed": changed,
           "window": g, "settle_s": settle, "before": str(before), "after": str(after)}
    if changed == 0:
        raise ToolError(f"oracle_click: the click at window ({x},{y}) changed 0 pixels — "
                        f"input did not land (idle difference is also 0, so 0 is a failure). "
                        f"before={before} after={after}")
    return out


def tool_render(midi: str | None = None, notes: list | None = None,
                seconds: float | None = None, pre_roll: float = 0.25,
                tail: float = 0.7, channel: int = 0) -> dict:
    SESSION.require_up("oracle_render")
    if midi and notes:
        raise ToolError("oracle_render: give either midi or notes, not both")
    if not midi and not notes:
        raise ToolError("oracle_render: give a midi path or a note list")
    if seconds is not None:
        try:
            seconds = float(seconds)
        except (TypeError, ValueError):
            raise ToolError("oracle_render: seconds must be a number")
        if not (0.1 <= seconds <= 120):
            raise ToolError("oracle_render: seconds must be between 0.1 and 120")
    SESSION.counters["render"] += 1
    n = SESSION.counters["render"]
    out_dir = SESSION.addr()
    if midi:
        src = Path(midi).expanduser()
        if not src.is_absolute() or not src.exists():
            raise ToolError(f"oracle_render: {midi!r} is not an existing absolute path")
        midi_path = src
        info = midi_info(midi_path)
        duration = info["duration_s"]
        note_count = info["notes"]
    else:
        midi_path = out_dir / f"render-{n:03d}.mid"
        meta = write_midi(midi_path, notes, bpm=float(os.environ.get("ORACLE_MCP_BPM", 120)))
        note_count = meta["notes"]
        info = midi_info(midi_path)
        duration = info["duration_s"]
    sink = ensure_sink()
    routed = route_to_sink(SESSION.kontakt_pids or kontakt_pids())
    port = midi_port()

    raw = out_dir / f"render-{n:03d}-raw.wav"
    wav = out_dir / f"render-{n:03d}.wav"
    raw.unlink(missing_ok=True)
    wav.unlink(missing_ok=True)
    env = base_env()
    t_rec = time.monotonic()
    rec = subprocess.Popen(["pw-record", "--target", SINK, "-P",
                            "{ stream.capture.sink=true }", "--rate", "48000",
                            "--channels", "2", "--format", "f32", str(raw)],
                           env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                           start_new_session=True)
    time.sleep(pre_roll)
    play_env = base_env()
    played = subprocess.Popen(["aplaymidi", "-p", str(port), str(midi_path)], env=play_env,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              start_new_session=True)
    started = time.monotonic()
    truncated = False
    if seconds is None or seconds >= duration:
        # play the whole file, then keep `tail` seconds of release
        try:
            played.wait(timeout=duration + 20)
        except subprocess.TimeoutExpired:
            played.kill()
            raise ToolError(f"oracle_render: aplaymidi did not finish a {duration:.2f}s file")
        time.sleep(tail)
    else:
        # a deliberate cut: `seconds` of programme, counted from playback start
        while time.monotonic() - started < seconds:
            if played.poll() is not None:
                break
            time.sleep(0.05)
        if played.poll() is None:
            truncated = True
            played.kill()
        time.sleep(min(tail, 0.5))
    for proc, name in ((rec, "pw-record"),):
        if proc.poll() is None:
            proc.send_signal(signal.SIGINT)
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
    if played.poll() is None:
        played.kill()
    t_stop = time.monotonic()
    err = (rec.stderr.read() or b"").decode("utf-8", "replace").strip() if rec.stderr else ""
    if not raw.exists() or raw.stat().st_size < 100:
        raise ToolError(f"oracle_render: pw-record wrote no audio ({raw}); {err[:300]}")

    raw_st = wav_stats(raw)
    if raw_st["peak"] == 0 or raw_st["peak_dbfs"] is None:
        raise ToolError(
            f"oracle_render: the render is silent (peak 0) — nothing reached the sink. "
            f"sink={sink} routed={routed} port={port} midi={midi_path} "
            f"instrument={SESSION.instrument} wav={raw}")
    # Cut the requested window relative to the MEASURED onset, not to the wall clock: the
    # capture's first sample is not the moment pw-record was spawned, so a time-based cut
    # would make `seconds` mean nothing.
    start = max(0.0, (raw_st["onset_s"] or 0.0) - 0.05)  # 50 ms of lead-in
    length = (seconds if seconds is not None else duration) + tail
    cut = trim_wav(raw, wav, start, length)
    st = wav_stats(wav)
    if st["peak"] == 0 or st["peak_dbfs"] is None:
        raise ToolError(f"oracle_render: the trimmed window is silent ({wav})")
    timing = {"record_spawn_to_stop_s": round(t_stop - t_rec, 3),
              "play_spawn_to_stop_s": round(t_stop - started, 3)}
    return {
        "raw": str(raw),
        "raw_duration_s": raw_st["duration_s"],
        "raw_onset_s": raw_st["onset_s"],
        "trim": cut,
        "timing": timing,
        "wav": str(wav),
        "midi": str(midi_path),
        "midi_duration_s": duration,
        "notes": note_count,
        "requested_seconds": seconds,
        "truncated": truncated,
        "sink": sink,
        "routed": routed,
        "midi_port": port,
        "instrument": SESSION.instrument,
        "latency_note": "the WAV starts 50 ms before the measured onset; raw_onset_s is "
                        "how far into the raw capture the note landed (MIDI + sink + Kontakt "
                        "latency, not calibrated)",
        **st,
    }


def tool_dump_script(nki: str, probe: str = "ksp", timeout: float = 240.0) -> dict:
    """Run a Lua probe inside a disposable Kontakt and return what it wrote.

    dump.py's contract is kept (ORACLE_INPUTS / ORACLE_OUT globals, one JSON record
    per line, last line {"done":true}), but not dump.py's launcher: it forces
    WINEDLLOVERRIDES=...=b and its default EXE (~/.wine/.../Portapotty/...) is not on
    this machine. The launch below is the same wine install our session runs.
    """
    preflight()
    path = Path(nki).expanduser()
    if not path.is_absolute() or not path.exists():
        raise ToolError(f"oracle_dump_script: {nki!r} is not an existing absolute path")
    if probe not in ("ksp", "dump"):
        raise ToolError("oracle_dump_script: probe must be 'ksp' (script source) or 'dump' (dump.lua)")
    source = KSP_PROBE if probe == "ksp" else DUMP_LUA
    if not source.exists():
        raise ToolError(f"oracle_dump_script: probe {source} is missing")

    ensure_settings()  # the probe Kontakt must not raise modal dialogs about audio/MIDI
    display = free_display(PROBE_DISPLAYS)
    run = WORK / time.strftime("probe-%Y%m%d-%H%M%S")
    run.mkdir(parents=True, exist_ok=True)
    out = run / "probe.jsonl"
    script = run / "probe.lua"
    script.write_text(
        "ORACLE_INPUTS = {" + lua_str(winpath(path)) + "}\n"
        + "ORACLE_OUT = " + lua_str(winpath(out)) + "\n"
        + source.read_text())
    log_path = run / "probe.log"

    guard_display(display)
    clear_stale_lock(display)
    if display_up(display):
        raise ToolError(f"oracle_dump_script: display {display} is already in use")
    xvfb = subprocess.Popen(["Xvfb", display, *XVFB_ARGS], start_new_session=True,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    t0 = time.monotonic()
    while time.monotonic() - t0 < 15 and not display_up(display):
        time.sleep(0.3)
    if not display_up(display):
        raise ToolError(f"oracle_dump_script: Xvfb {display} did not come up")

    env = base_env(display)
    env.update({"WINEPREFIX": str(PREFIX), "VK_ICD_FILENAMES": str(ICD),
                "WINEDLLOVERRIDES": DLL_OVERRIDES, "WINEDEBUG": "-all"})
    started = time.monotonic()
    with open(log_path, "wb") as logf:
        kontakt = subprocess.Popen([str(WINE), str(EXE), winpath(script)], cwd=str(EXEDIR),
                                   env=env, stdout=logf, stderr=subprocess.STDOUT,
                                   start_new_session=True)
    done = False
    try:
        while time.monotonic() - started < timeout:
            if kontakt.poll() is not None and not out.exists():
                raise ToolError(f"oracle_dump_script: Kontakt exited rc={kontakt.returncode} "
                                f"without writing {out.name}; log: {read_log_tail(log_path)}")
            if out.exists() and out.read_text(errors="replace").rstrip().endswith('{"done":true}'):
                done = True
                break
            time.sleep(0.5)
    finally:
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.killpg(os.getpgid(kontakt.pid), sig)
            except OSError:
                break
            t0 = time.monotonic()
            while time.monotonic() - t0 < 6 and kontakt.poll() is None:
                time.sleep(0.2)
        try:
            os.killpg(os.getpgid(xvfb.pid), signal.SIGKILL)
        except OSError:
            pass
    if not done:
        raise ToolError(f"oracle_dump_script: the probe did not finish within {timeout}s; "
                        f"out={out} log: {read_log_tail(log_path)}")
    records = [json.loads(line) for line in out.read_text(errors="replace").splitlines()
               if line.strip()]
    records = [r for r in records if not r.get("done")]
    if not records:
        raise ToolError(f"oracle_dump_script: probe wrote no records ({out})")
    out_rec = {"nki": str(path), "probe": probe, "probe_file": str(source),
               "records": records, "seconds": round(time.monotonic() - started, 2),
               "run_dir": str(run), "display": display}
    if probe == "ksp":
        # the point of this probe: the script text, so a caller can diff it against KONTRA
        text = []
        for rec in records:
            inst = rec.get("instrument") or {}
            for s in inst.get("scripts") or []:
                if s.get("source"):
                    text.append({"slot": s.get("slot"), "name": s.get("name"),
                                 "protected": s.get("protected"),
                                 "source": s["source"]})
        out_rec["scripts"] = text
        out_rec["ksp_chars"] = sum(len(t["source"]) for t in text)
        if not text:
            raise ToolError(
                "oracle_dump_script: the instrument reports no script source "
                f"(protected or scriptless); records={json.dumps(records)[:600]}")
    return out_rec


def lua_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def _module_base(pid: int) -> int:
    """Lowest mapping of the Kontakt executable: its PE image base under wine."""
    best = None
    try:
        for line in Path(f"/proc/{pid}/maps").read_text().splitlines():
            parts = line.split(None, 5)  # the path may contain spaces: keep it one field
            if len(parts) == 6 and parts[5].lower().endswith("kontakt 8.exe"):
                lo = int(parts[0].split("-")[0], 16)
                best = lo if best is None else min(best, lo)
    except OSError as e:
        raise ToolError(f"oracle_mem: cannot read /proc/{pid}/maps: {e}")
    if best is None:
        raise ToolError(f"oracle_mem: pid {pid} has no mapping of Kontakt 8.exe (is it the right "
                        f"process?)")
    return best


def tool_mem(addr, length: int = 64, pid: int | None = None) -> dict:
    SESSION.require_up("oracle_mem")
    try:
        length = int(length)
    except (TypeError, ValueError):
        raise ToolError("oracle_mem: length must be an integer")
    if not (1 <= length <= 4096):
        raise ToolError("oracle_mem: length must be between 1 and 4096 bytes")
    if isinstance(addr, str):
        name = addr.strip().lower()
        if name in ("exe", "kontakt.exe", "module"):
            address = _module_base(pid if pid is not None else kontakt_pids()[0])
        else:
            try:
                address = int(addr, 0)
            except ValueError:
                raise ToolError(f"oracle_mem: {addr!r} is not an address (use 0x..., decimal "
                                f"or 'exe' for the Kontakt module base)")
    else:
        try:
            address = int(addr)
        except (TypeError, ValueError):
            raise ToolError("oracle_mem: addr must be an integer or 0x... string")
    if address <= 0:
        raise ToolError("oracle_mem: addr must be > 0")
    pids = SESSION.kontakt_pids or kontakt_pids()
    if not pids:
        raise ToolError("oracle_mem: no Kontakt process in this session's prefix")
    target = pid if pid is not None else max(pids)
    if target not in pids:
        raise ToolError(f"oracle_mem: pid {target} is not a Kontakt of this session "
                        f"(session pids: {pids})")
    mapping = None
    try:
        for line in Path(f"/proc/{target}/maps").read_text().splitlines():
            parts = line.split(None, 5)  # the path may contain spaces: keep it one field
            if len(parts) >= 2 and "-" in parts[0]:
                lo, hi = (int(x, 16) for x in parts[0].split("-"))
                if lo <= address < hi:
                    mapping = {"range": parts[0], "perms": parts[1],
                               "path": parts[5] if len(parts) > 5 else None}
                    break
    except OSError as e:
        raise ToolError(f"oracle_mem: cannot read /proc/{target}/maps: {e}")
    if mapping is None:
        raise ToolError(f"oracle_mem: 0x{address:x} is not in any mapping of pid {target}")
    try:
        with open(f"/proc/{target}/mem", "rb", buffering=0) as mem:
            mem.seek(address)
            data = mem.read(length)
    except OSError as e:
        raise ToolError(f"oracle_mem: reading 0x{address:x} from pid {target} failed: {e}")
    if not data:
        raise ToolError(f"oracle_mem: 0x{address:x} returned no bytes (unmapped page?)")
    if len(data) < length:
        raise ToolError(f"oracle_mem: short read at 0x{address:x}: {len(data)} of {length} "
                        f"bytes (mapping ends at 0x{int(mapping['range'].split('-')[1], 16):x})")
    u32 = [struct.unpack_from("<I", data, i)[0] for i in range(0, len(data) - 3, 4)]
    f32 = [round(v, 8) for v in (struct.unpack_from("<f", data, i)[0]
                                 for i in range(0, len(data) - 3, 4))]
    return {"pid": target, "addr": f"0x{address:x}", "length": len(data),
            "hex": data.hex(), "u32": u32, "f32": f32, "mapping": mapping}


def tool_status() -> dict:
    state = {"up": SESSION.up, "display": SESSION.display, "window": SESSION.window,
             "instrument": SESSION.instrument,
             "pids": {'xvfb': getattr(SESSION.xvfb, 'pid', None),
                      'openbox': getattr(SESSION.openbox, 'pid', None),
                      'wine': getattr(SESSION.wine, 'pid', None),
                      'kontakt': SESSION.kontakt_pids},
             "work_dir": str(SESSION.addr()) if SESSION.run_dir else None,
             "log": str(LOG_PATH)}
    if SESSION.up and SESSION.window and SESSION.display:
        state["window_geometry"] = geometry(SESSION.window)
    state["kontakt_pids_in_prefix"] = kontakt_pids()
    return state


TOOLS = {
    "oracle_up": {
        "fn": tool_up,
        "description": "Start the private oracle rig: Xvfb (:77 or :9x, never :0) + openbox, "
                       "create the PipeWire null sink and check Kontakt's Settings.cfg, then "
                       "launch Kontakt 8 standalone with DXVK on lavapipe and wait for its "
                       "window. One long-lived Kontakt per server process. Optionally load an "
                       ".nki. Returns the config actually used and the renderer line.",
        "schema": {"type": "object", "properties": {
            "nki": {"type": "string", "description": "absolute path of an .nki to load after launch"}}},
    },
    "oracle_down": {
        "fn": tool_down,
        "description": "Stop exactly the processes this server started (Xvfb, openbox, the "
                       "wine process group incl. Kontakt), by PID/process group. Reports each "
                       "signal sent and whether the display went away.",
        "schema": {"type": "object", "properties": {}},
    },
    "oracle_load": {
        "fn": tool_load,
        "description": "Load an .nki into the running Kontakt (Ctrl+O, then the path) and wait "
                       "until the instrument page paints. Returns window geometry, page pulses "
                       "and whether the page is up.",
        "schema": {"type": "object", "properties": {
            "nki": {"type": "string", "description": "absolute path of the .nki"}},
            "required": ["nki"]},
    },
    "oracle_shot": {
        "fn": tool_shot,
        "description": "Capture the private display's root, crop the Kontakt window, save a "
                       "PNG. Returns path, size, texture sd, distinct colours and whether the "
                       "instrument page is up. Fails if the window is blank.",
        "schema": {"type": "object", "properties": {}},
    },
    "oracle_click": {
        "fn": tool_click,
        "description": "Click at window-relative coordinates and return how many pixels "
                       "changed (mean-absolute-difference x pixels). changed=0 is reported as "
                       "a failure, not a success; a coordinate outside the window is refused "
                       "with changed=0.",
        "schema": {"type": "object", "properties": {
            "x": {"type": "integer"}, "y": {"type": "integer"},
            "settle": {"type": "number", "description": "seconds to wait before the second "
                                                        "capture (default 2.5)"}},
            "required": ["x", "y"]},
    },
    "oracle_render": {
        "fn": tool_render,
        "description": "Play a MIDI file (or a note list) into Kontakt with aplaymidi and "
                       "record the null sink's monitor with pw-record. Returns WAV path, "
                       "duration, onset, peak/rms in dBFS and sha256. A silent render is an "
                       "error.",
        "schema": {"type": "object", "properties": {
            "midi": {"type": "string", "description": "absolute path of a standard MIDI file"},
            "notes": {"type": "array", "items": {"type": "object", "properties": {
                "start_s": {"type": "number"}, "length_s": {"type": "number"},
                "key": {"type": "integer"}, "velocity": {"type": "integer"},
                "channel": {"type": "integer"}},
                "required": ["key"]},
                "description": "note list instead of a MIDI file"},
            "seconds": {"type": "number", "description": "audio length to record (default: "
                                                       "the whole MIDI plus tail)"},
            "tail": {"type": "number", "description": "seconds of release tail to keep"}}},
    },
    "oracle_dump_script": {
        "fn": tool_dump_script,
        "description": "Run a Lua probe inside a disposable Kontakt (dump.py's contract: "
                       "ORACLE_INPUTS/ORACLE_OUT, JSON lines, {\"done\":true}) and return what "
                       "it wrote. probe=ksp (default) returns the KSP script text per slot; "
                       "probe=dump runs tools/kontakt-oracle/dump.lua (state, hashed sources).",
        "schema": {"type": "object", "properties": {
            "nki": {"type": "string", "description": "absolute path of the .nki"},
            "probe": {"type": "string", "enum": ["ksp", "dump"]},
            "timeout": {"type": "number"}},
            "required": ["nki"]},
    },
    "oracle_mem": {
        "fn": tool_mem,
        "description": "Read bytes from the Kontakt process (this session's prefix only) at an "
                       "address and return hex plus u32/f32 interpretations and the mapping.",
        "schema": {"type": "object", "properties": {
            "addr": {"type": ["integer", "string"], "description": "address (0x... or decimal)"},
            "length": {"type": "integer"}, "pid": {"type": "integer"}},
            "required": ["addr"]},
    },
    "oracle_status": {
        "fn": tool_status,
        "description": "The session's state: up/down, display, window geometry, PIDs, instrument.",
        "schema": {"type": "object", "properties": {}},
    },
}


# --------------------------------------------------------------------------- MCP
def tool_list() -> dict:
    return {"tools": [{"name": name, "description": spec["description"],
                       "inputSchema": spec["schema"]} for name, spec in TOOLS.items()]}


def tool_call(params: dict) -> dict:
    name = params.get("name")
    if name not in TOOLS:
        raise RpcError(-32602, f"unknown tool {name!r}")
    args = params.get("arguments") or {}
    if not isinstance(args, dict):
        raise RpcError(-32602, "arguments must be an object")
    log(f"tool {name} {json.dumps(args)[:400]}")
    t0 = time.monotonic()
    try:
        result = TOOLS[name]["fn"](**args)
    except ToolError as e:
        log(f"tool {name} FAILED in {time.monotonic() - t0:.1f}s: {e}", "error")
        return {"content": [{"type": "text", "text": json.dumps(
            {"error": str(e), "tool": name}, indent=2)}], "isError": True}
    except TypeError as e:
        log(f"tool {name} bad arguments: {e}", "error")
        return {"content": [{"type": "text", "text": json.dumps(
            {"error": f"bad arguments for {name}: {e}", "tool": name}, indent=2)}],
            "isError": True}
    except Exception as e:  # noqa: BLE001 - a crash must be reported, not swallowed
        log(f"tool {name} CRASHED: {e}\n{traceback.format_exc()}", "error")
        return {"content": [{"type": "text", "text": json.dumps(
            {"error": f"internal error in {name}: {e}", "tool": name,
             "traceback": traceback.format_exc()[-1200:]}, indent=2)}], "isError": True}
    log(f"tool {name} ok in {time.monotonic() - t0:.1f}s")
    return {"content": [{"type": "text", "text": json.dumps(result, indent=2, default=str)}],
            "isError": False}


def handle(method: str, params: dict):
    if method == "initialize":
        return {"protocolVersion": params.get("protocolVersion", "2024-11-05"),
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "kontakt-oracle", "version": VERSION}}
    if method == "ping":
        return {}
    if method == "tools/list":
        return tool_list()
    if method == "tools/call":
        return tool_call(params)
    if method in ("resources/list", "prompts/list"):
        key = method.split("/")[0]
        return {key: []}
    raise RpcError(-32601, f"method not found: {method}")


def send(msg: dict) -> None:
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def serve() -> int:
    rotate_log()
    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        def handler(signum, _frame, _sig=sig):
            log(f"signal {signum}: stopping the session before exit", "warn")
            try:
                if SESSION.wine is not None or SESSION.xvfb is not None:
                    tool_down()
            finally:
                os._exit(0)
        try:
            signal.signal(sig, handler)
        except (OSError, ValueError):
            pass
    try:
        notes = reap_stale()
        if notes:
            log("startup reap: " + "; ".join(notes))
    except Exception as e:  # noqa: BLE001 - a reaper bug must not stop the server
        log(f"startup reap failed: {e}", "error")
    log(f"oracle-mcp {VERSION} starting (pid {os.getpid()}); log {LOG_PATH}; "
        f"verbose={VERBOSE}", "info")
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError as e:
            send({"jsonrpc": "2.0", "id": None,
                  "error": {"code": -32700, "message": f"parse error: {e}"}})
            continue
        method = msg.get("method")
        mid = msg.get("id")
        if method is None:
            continue  # a response or unknown message; we never send requests
        if mid is None:
            # notification: notifications/initialized and friends need no answer
            if VERBOSE:
                log(f"notification {method}")
            continue
        try:
            result = handle(method, msg.get("params") or {})
            send({"jsonrpc": "2.0", "id": mid, "result": result})
        except RpcError as e:
            send({"jsonrpc": "2.0", "id": mid,
                  "error": {"code": e.code, "message": e.message}})
        except Exception as e:  # noqa: BLE001
            log(f"{method} crashed: {e}\n{traceback.format_exc()}", "error")
            send({"jsonrpc": "2.0", "id": mid,
                  "error": {"code": -32603, "message": f"internal error: {e}"}})
    log("stdin closed; shutting down")
    if SESSION.wine is not None:
        log("session still up at exit; stopping it", "warn")
        try:
            tool_down()
        except Exception as e:  # noqa: BLE001
            log(f"cleanup at exit failed: {e}", "error")
    return 0


def main() -> int:
    if "--selftest" in sys.argv:
        print(json.dumps({"tools": [t["name"] for t in tool_list()["tools"]],
                          "log": str(LOG_PATH)}, indent=2))
        return 0
    return serve()


if __name__ == "__main__":
    sys.exit(main())
