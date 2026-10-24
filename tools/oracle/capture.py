#!/usr/bin/env python3
"""Headless audio capture of real Kontakt 8 instruments (Wine, PipeWire null sink, no speakers).

    tools/oracle/capture.py NKI OUT.wav [--window S]               one instrument
    tools/oracle/capture.py --batch LIST.tsv OUTDIR [--window S]   many instruments

LIST.tsv: `name<TAB>nki_path` per line. Instruments load 16 at a time, one Kontakt session per
group. Instrument k of a group gets MIDI channel k+1 and time slot k of WINDOW seconds: the
scenario notes play on its channel, then All Sound Off (CC 120) on that channel closes the slot,
so no tail leaks into the next slot.

Audio: Kontakt's outputs are linked to the PipeWire null sink `kontra_ref` only (re-checked
throughout the session, because PipeWire re-links new ports to the default output = speakers).
The recording is that sink's monitor, 48 kHz float stereo, sliced per slot into OUTDIR/<name>.wav:
PRE seconds before the slot start, then WINDOW seconds.

Kontakt's audio device is chosen once per session through the Options dialog (GUI clicks, same
coordinates as tools/kontakt-oracle/render.py); the choice does not persist across launches.

Accuracy: slot starts come from the monotonic clock at MIDI start; slices are good to a few ms.
Absolute latency is not calibrated here: compare.py aligns each capture to the KONTRA render.
"""
import argparse
import math
import array
import importlib.util
import json
import os
import signal
import struct
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import slot  # noqa: E402  (oracle-slot environment; legacy single session when unset)

KONTAKT_ORACLE = Path(__file__).resolve().parent.parent / "kontakt-oracle"
CACHE = slot.work(Path.home() / ".cache/kontra-native-oracle")


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


dump = _load("dump", KONTAKT_ORACLE / "dump.py")  # EXE, winpath, dismiss_dialogs
wavutil = _load("wavutil", KONTAKT_ORACLE / "wavutil.py")

SINK = slot.sink("kontra_ref")
PER_SESSION = slot.channel_block()[1]  # MIDI channels this session may use (16 in legacy mode, 5 per slot of 3)
PRE = 0.5  # seconds kept before each slot
DISPLAY = slot.display(":79")  # dump.py uses :78, the reference harness :77
DESKTOP = "1600x1000"
SCENARIO = [(0.2, 1.0, 60, 100), (1.6, 1.0, 64, 90), (3.0, 1.5, 67, 110)]  # start, length, key, velocity
# Causes separated by scenario (all notes on one MIDI channel): round robin = same key and velocity
# repeated (a counter RR shows as different samples per hit); velsweep = one key, velocity sweep
# (layer boundaries); legato = each note-on lands 0.3 s before the previous note-off (overlap).
SCENARIOS = {
    "base": SCENARIO,
    "rr8": [(0.2 + 0.7 * i, 0.5, 60, 100) for i in range(8)],
    "velsweep": [(0.2 + 0.7 * i, 0.5, 60, v) for i, v in enumerate(range(16, 128, 16))],
    "rr32": [(0.2 + 0.7 * i, 0.5, 60, 100) for i in range(32)],  # distribution of per-hit start offsets
    "release": [(0.2 + 1.0 * i, 0.15, 60, 100) for i in range(4)],
    "velsplit": [(0.2 + 0.7 * i, 0.5, 60, 127 if i % 2 == 0 else 40) for i in range(16)],  # velocity scaling of start offsets  # short notes: each note-off fires release triggers
    "legato": [(0.2 + 0.8 * i, 1.1, k, 100) for i, k in enumerate((60, 62, 64, 65))],
}


def sh(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout


def lua_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def vlq(n):
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.insert(0, (n & 0x7F) | 0x80)
        n >>= 7
    return bytes(out)


def make_midi(path, slots, window=None, notes=SCENARIO, per_slot=None):
    """Format 0, 120 bpm, 480 ppq (960 ticks/s). Slot k: `notes` (or `per_slot[k]`) on channel k+1 from k*window.
    With `window`, All Sound Off (CC 120) on channel k+1 closes each slot; without it (the single
    scenario file) only the notes are written."""
    events = []
    first = slot.channel_block()[0]  # this session's MIDI channel block (see slot.py)
    for k in range(slots):
        ch, base = first + k, (k * window if window else 0.0)
        for start, length, key, vel in (per_slot[k] if per_slot else notes):
            on, off = round((base + start) * 960), round((base + start + length) * 960)
            events.append((on, 1, bytes([0x90 | ch, key, vel])))
            events.append((off, 0, bytes([0x80 | ch, key, 0])))
        if window:
            events.append((round((base + window) * 960) - 1, 0, bytes([0xB0 | ch, 120, 0])))
    events.sort(key=lambda e: (e[0], e[1]))
    track = bytearray(b"\x00\xff\x51\x03\x07\xa1\x20")  # 500000 us per quarter note
    last = 0
    for tick, _, data in events:
        track += vlq(tick - last) + data
        last = tick
    track += b"\x00\xff\x2f\x00"
    Path(path).write_bytes(b"MThd" + struct.pack(">IHHH", 6, 0, 1, 480)
                           + b"MTrk" + struct.pack(">I", len(track)) + bytes(track))


def probe_lua(nkis):
    """Loads each NKI on MIDI channel k+1 and reports it. Returns at once: the audio device is
    selected afterwards through the GUI, which needs Kontakt's Lua thread to be idle."""
    rows = ",".join(lua_str(p) for p in nkis)
    return f"""
ORACLE_ITEMS = {{{rows}}}
local out = assert(io.open(ORACLE_OUT, "w"))
local function say(t) out:write(t, "\\n") out:flush() end
for i, path in ipairs(ORACLE_ITEMS) do
  local ok, idx = pcall(Kontakt.load_instrument, path)
  local good = ok and math.type(idx) == "integer"
  if good then pcall(Kontakt.set_instrument_midi_channel, idx, i + {slot.channel_block()[0]}) end
  say(string.format('{{"ready":%d,"ok":%s,"error":%q}}', i, tostring(good), good and "" or tostring(idx)))
end
say('{{"done":true}}')
out:close()
"""


def sink_state():
    for line in sh("pactl", "list", "short", "sinks").splitlines():
        if SINK in line.split("\t")[1:2]:
            return line.split("\t")[-1].strip()
    return None


def ensure_sink():
    """The capture sink must never suspend: a suspended null sink records silence even with Kontakt
    linked to it. A sink loaded without node.always-process (or found SUSPENDED) is reloaded."""
    if sink_state() in ("IDLE", "RUNNING"):
        return
    for line in sh("pactl", "list", "short", "modules").splitlines():
        if f"sink_name={SINK}" in line:
            subprocess.run(["pactl", "unload-module", line.split("\t")[0]], capture_output=True)
    subprocess.run(["pactl", "load-module", "module-null-sink", f"sink_name={SINK}",
                    f"sink_properties=device.description={SINK} node.always-process=true session.suspend-timeout-seconds=0",
                    "format=float32le", "rate=48000", "channels=2"], capture_output=True)
    time.sleep(1)
    if sink_state() in (None, "SUSPENDED"):
        sys.exit(f"{SINK} is missing or suspended after reload; check pactl list sinks")


def log(work, msg):
    """Session diagnostics: appended to WORK/session.log (sink, links, peaks)."""
    with open(work / "session.log", "a") as f:
        f.write(time.strftime("%H:%M:%S ") + msg + "\n")


def kontakt_links():
    return [l.strip() for l in sh("pw-link", "-l").splitlines() if "Kontakt" in l or "kontra_ref" in l][:8]


def kontakt_ports():
    return [p.strip() for p in sh("pw-link", "-o").splitlines() if "Kontakt" in p and "output_" in p]


def route_kontakt_to_sink(wait=0):
    """Link Kontakt's outputs to kontra_ref and remove every other link from them. Idempotent:
    PipeWire re-creates the ports when the audio device opens and auto-links them to the default
    output (the speakers). Returns the number of Kontakt output ports (0 = none yet)."""
    if slot.active():  # this session's Kontakt nodes only (see slot.route_slot)
        return slot.route_slot(SINK, slot.kontakt_pids(), wait)
    deadline = time.monotonic() + wait
    ports = kontakt_ports()
    while not ports and time.monotonic() < deadline:
        time.sleep(0.5)
        ports = kontakt_ports()
    if not ports:
        return 0
    # `pw-link -l`: an unindented port line, then indented "|-> destination" lines
    links, port = {}, None
    for line in sh("pw-link", "-l").splitlines():
        if line and not line[0].isspace() and "|" not in line:
            port = line.strip()
        elif "|->" in line and port in ports:
            links.setdefault(port, []).append(line.split("|->")[1].strip())
    for port in ports:
        dsts = links.get(port, [])
        for dst in dsts:
            if not dst.startswith(f"{SINK}:"):
                subprocess.run(["pw-link", "-d", port, dst], capture_output=True)
        want = f"{SINK}:playback_{port[-2:]}"
        if want not in dsts:
            subprocess.run(["pw-link", port, want], capture_output=True)
    return len(ports)


def click(env, x, y, wait=1.2):
    subprocess.run(["xdotool", "mousemove", str(x), str(y)], env=env)
    time.sleep(0.4)
    subprocess.run(["xdotool", "click", "1"], env=env)
    time.sleep(wait)


def select_device(env):
    """File > Options: Audio device = kontra_ref; MIDI: Midi Through Port-0 -> Port A.
    Coordinates from tools/kontakt-oracle/render.py (1600x1000 desktop); no screenshots."""
    time.sleep(5)
    click(env, 1089, 440)
    click(env, 20, 57)
    click(env, 60, 86)
    click(env, 388, 553)
    click(env, 1088, 318, 0.5)
    subprocess.run(["xdotool", "mousemove", "1060", "480"], env=env)
    for _ in range(6):
        subprocess.run(["xdotool", "click", "5"], env=env)
    time.sleep(0.5)
    click(env, 1010, 485, 2)
    click(env, 385, 520)
    click(env, 1075, 414)
    click(env, 1128, 476)
    click(env, 1178, 781)


def midi_port():
    return next((l.split()[0] for l in sh("aplaymidi", "-l").splitlines() if "Midi Through" in l), "14:0")


def kontakt_running():
    return bool(slot.kontakt_pids())


def stop_kontakt(timeout=20):
    """Ends this session's Kontakt (Wine takes a few seconds to tear down); SIGKILL if it lingers."""
    slot.stop_kontakt(timeout)


def run_session(nkis, window, work, midi=None, notes=SCENARIO, per_slot=None):
    """Loads up to PER_SESSION NKIs, plays the slotted MIDI, records. Returns (wav, t_midi, loaded)
    where t_midi is the MIDI start on the recording clock and loaded[k] = (ok, error)."""
    if kontakt_running():
        stop_kontakt()  # left over from the previous group of this batch, or a crashed session
    if kontakt_running():
        sys.exit("another Kontakt is running; one oracle session at a time")
    work.mkdir(parents=True, exist_ok=True)
    raw = work / "probe.jsonl"
    raw.unlink(missing_ok=True)
    script = work / "probe.lua"
    script.write_text("ORACLE_OUT = " + lua_str(dump.winpath(raw)) + "\n"
                      + probe_lua([dump.winpath(Path(n).resolve()) for n in nkis]))
    if midi is None:  # batch: one slot per instrument
        midi = work / "slots.mid"
        make_midi(midi, len(nkis), window, notes, per_slot)
    ensure_sink()
    session = work / "session.wav"
    xvfb = subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", f"{DESKTOP}x24"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    env = slot.wine_env(dict(os.environ, DISPLAY=DISPLAY, WINEDLLOVERRIDES="d3d11,d3d10core,dxgi,d3d9=b",
                             WINEDEBUG="-all", PULSE_SINK=SINK))
    kontakt = subprocess.Popen(["wine", "explorer", f"/desktop=oracle,{DESKTOP}", dump.EXE, dump.winpath(script)],
                               env=env, stdout=open(work / "kontakt.log", "w"), stderr=subprocess.STDOUT,
                               start_new_session=True)
    keep = rec = player = None
    loaded, t_midi = {}, None
    try:
        deadline = time.monotonic() + 240 + 30 * len(nkis)
        seen, done = 0, False
        while time.monotonic() < deadline and not done:  # phase 1: loads
            dump.dismiss_dialogs(env)
            lines = raw.read_text(errors="replace").splitlines() if raw.exists() else []
            for line in lines[seen:]:
                record = json.loads(line) if line.strip() else {}
                if record.get("done"):
                    done = True
                elif "ready" in record:
                    loaded[record["ready"] - 1] = (record["ok"], record["error"])
            seen = len(lines)
            time.sleep(1)
        if not done:
            sys.exit(f"probe did not finish; see {work}")
        if not slot.active() or os.environ.get("ORACLE_GUI_SETUP"):  # a slot's device/MIDI come from its Settings.cfg
            select_device(env)
        if not route_kontakt_to_sink(wait=60):
            sys.exit(f"Kontakt has no audio output ports; see {work / 'kontakt.log'}")
        log(work, f"after route: sink={sink_state()} links={kontakt_links()}")
        keep = subprocess.Popen(["pw-cat", "-p", "--raw", "--rate", "48000", "--channels", "2", "--format", "f32",
                                 "--target", SINK, "/dev/zero"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        t_rec = time.monotonic()  # the WAV's sample 0 is at recorder start, so the clock starts here
        rec = subprocess.Popen(["pw-record", "--target", SINK, "-P", "{ stream.capture.sink=true }", "--rate", "48000",
                                "--format", "f32", "--channels", "2", str(session)],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1.5)  # recorder start-up; no MIDI yet
        route_kontakt_to_sink()
        player = subprocess.Popen(["aplaymidi", "-p", midi_port(), str(midi)],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        t_midi = time.monotonic() - t_rec
        end = t_rec + t_midi + len(nkis) * window + 1
        while time.monotonic() < end:  # phase 2: keep links pinned while the slots play
            route_kontakt_to_sink()
            time.sleep(0.1)
        time.sleep(0.5)
        log(work, f"session end: sink={sink_state()} links={kontakt_links()}")
    finally:
        if player and player.poll() is None:
            player.kill()
        if rec:
            rec.send_signal(signal.SIGINT)
            rec.wait(15)
        if keep:
            keep.terminate()
        stop_kontakt()
        if kontakt.poll() is None:
            os.killpg(kontakt.pid, signal.SIGKILL)
        xvfb.terminate()
    return session, t_midi, loaded


def slice_session(session, t_midi, names, loaded, out_dir, window):
    """OUTDIR/<name>.wav per slot: PRE seconds, then the slot's WINDOW seconds."""
    ch, rate, chans = wavutil.read(session)
    frames = len(chans[0])
    manifest = {}
    for k, name in enumerate(names):
        ok, error = loaded.get(k, (False, "no report from probe"))
        if not ok:
            manifest[name] = {"loaded": False, "error": error, "wav": None}
            continue
        slot = t_midi + k * window
        start = max(0, round((slot - PRE) * rate))
        end = min(frames, round((slot + window) * rate))
        inter = array.array("f", [0.0]) * ((end - start) * ch)
        for c in range(ch):
            inter[c::ch] = array.array("f", chans[c][start:end])
        data = inter.tobytes()
        path = out_dir / f"{name}.wav"
        path.write_bytes(b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt "
                         + struct.pack("<IHHIIHH", 16, 3, ch, rate, rate * ch * 4, ch * 4, 32)
                         + b"data" + struct.pack("<I", len(data)) + data)
        peak = max(max(inter), -min(inter), 0.0)
        manifest[name] = {"loaded": True, "error": "", "wav": str(path), "frames": end - start,
                          "slot_start_in_wav_s": PRE, "peak_db": 20 * math.log10(peak + 1e-12)}
    return manifest


CANARY_NAME = "__canary__"
CANARY_MIN_DB = -50.0  # the canary is known audible: below this the session is broken


def capture(items, out_dir, window, midi=None, notes=SCENARIO, canary=None):
    """items: [(name, nki)] or [(name, nki, notes)] (per-instrument notes; default `notes`).
    `canary` = (name, nki, notes) of a known audible instrument: it takes slot 0 of every session,
    and the session aborts loudly when it records silence (the audio or MIDI path is broken).
    `midi` (single instrument only) is played as given instead of the slotted batch scenario.
    Returns the manifest {name: {...}}; WAVs land in out_dir."""
    out_dir.mkdir(parents=True, exist_ok=True)
    items = [(it[0], it[1], it[2] if len(it) > 2 else notes) for it in items]
    if canary:
        items = [(CANARY_NAME, canary[1], canary[2])] + items
    manifest = {}
    per = PER_SESSION
    for g in range(0, len(items), per):
        group = items[g:g + per]
        work = CACHE / f"capture-{os.getpid()}-{g}"
        names = [n for n, _, _ in group]
        nkis = [k for _, k, _ in group]
        per_slot = [ns for _, _, ns in group]
        session, t_midi, loaded = run_session(nkis, window, work, midi, per_slot=per_slot)
        if session.exists():
            _, _, chans = wavutil.read(session)
            peak = max((max(max(c), -min(c)) for c in chans if len(c)), default=0.0)
            log(work, f"recording peak {20 * math.log10(peak + 1e-12):.1f} dBFS, {len(chans[0])} frames")
        part = slice_session(session, t_midi, names, loaded, out_dir, window)
        for name, nki, _ in group:
            part[name]["nki"] = nki
        if canary and g == 0:
            c = part[CANARY_NAME]
            if not c.get("loaded") or c.get("peak_db", -240) < CANARY_MIN_DB:
                sys.exit(f"canary silent in session {work} ({c.get('peak_db')} dBFS, load {c.get('error')}): "
                         f"audio or MIDI path broken, see {work / 'session.log'}")
        manifest.update(part)
        session.unlink(missing_ok=True)
    return manifest


def scenario_notes(name="base", keyswitch=None):
    """SCENARIOS[name]; with `keyswitch`, a 0.1 s keyswitch note at 0 s and the scenario 0.3 s later."""
    notes = SCENARIOS[name]
    if keyswitch is None:
        return notes
    return [(0.0, 0.1, keyswitch, 100)] + [(start + 0.3, length, key, vel) for start, length, key, vel in notes]


def write_scenario(name="base", keyswitch=None):
    tag = f"{name}-ks{keyswitch}" if keyswitch is not None else name
    path = CACHE / f"scenario-{tag}.mid"
    CACHE.mkdir(parents=True, exist_ok=True)
    make_midi(path, 1, notes=scenario_notes(name, keyswitch))
    return path


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("inputs", nargs="+", help="NKI OUT.wav, or OUTDIR with --batch")
    ap.add_argument("--batch", metavar="LIST.tsv")
    ap.add_argument("--window", type=float, default=6.0, help="seconds per instrument slot (default 6)")
    ap.add_argument("--midi", help="single mode: MIDI file to play (default: the built-in scenario)")
    ap.add_argument("--scenario", choices=sorted(SCENARIOS), default="base",
                    help="note pattern for the slots or the single file (see SCENARIOS)")
    ap.add_argument("--keyswitch", type=int, metavar="KEY",
                    help="play a keyswitch note KEY at 0 s, the scenario from 0.3 s (single-file mode)")
    args = ap.parse_args()
    if args.batch:
        if len(args.inputs) != 1:
            ap.error("--batch LIST.tsv OUTDIR")
        rows = [l.split("\t") for l in Path(args.batch).read_text().splitlines()
                if l.strip() and not l.startswith("#")]
        items = [(r[0], r[1]) for r in rows]
        out_dir = Path(args.inputs[0]).resolve()
    else:
        if len(args.inputs) != 2 or not args.inputs[1].endswith(".wav"):
            ap.error("NKI OUT.wav")
        out_wav = Path(args.inputs[1]).resolve()
        items = [(out_wav.stem, str(Path(args.inputs[0]).resolve()))]
        out_dir = out_wav.parent
    midi = None if args.batch else Path(args.midi).resolve() if args.midi else write_scenario(args.scenario, args.keyswitch)
    manifest = capture(items, out_dir, args.window, midi, scenario_notes(args.scenario, args.keyswitch))
    if args.batch:  # merge: a resumed batch keeps the earlier entries
        cj = out_dir / "capture.json"
        merged = json.loads(cj.read_text()) if cj.exists() else {}
        merged.update(manifest)
        cj.write_text(json.dumps(merged, indent=1, sort_keys=True))
    ok = sum(1 for m in manifest.values() if m.get("wav"))
    print(f"captured {ok}/{len(items)} -> {out_dir}")
    return 0 if ok == len(items) else 1


if __name__ == "__main__":
    sys.exit(main())
