#!/usr/bin/env python3
"""Which samples a library's own scripts play in Kontakt, for one short phrase.

    oracle-slot python3 tools/kontakt-oracle/script_trace.py LIB.nki OUT_DIR KEY [KS,KS,...]

Loads LIB.nki, puts a tracer KSP in the last free script slot and plays a phrase over MIDI:
legato line, a repeated note (round robin), a pedalled pair, soft and loud notes, each
keyswitch followed by KEY, then a mod-wheel sweep over a held KEY. Every note event that
reaches the tracer (MIDI or script-made) logs time ms, key, velocity, source slot, zone,
group and sample name to OUT_DIR/trace.nka. A zone of -1 is retried after 10 and 50 ms;
an event with no voice may still report an earlier event's zone (Kontakt 8). The key
colours and names go to OUT_DIR/keys.nka.
OUT_DIR/phrase.json is the same phrase for `kontra-inspect play --scenario`.
"""
import importlib.util
import json
import os
import struct
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("dump", HERE / "dump.py")
dump = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dump)
sys.path.insert(0, str(HERE.parent / "oracle"))
import slot  # noqa: E402

RATE = 48000


def phrase(k: int, switches: list[int]) -> list[tuple]:
    """(ms, kind, a, b): kind on/off (key, velocity) or cc (number, value)."""
    ev = [(0, "cc", 1, 90), (0, "cc", 11, 127)]

    def note(at, key, length, vel=100):
        ev.extend([(at, "on", key, vel), (at + length, "off", key, 0)])

    for i, step in enumerate([0, 2, 4, 5]):  # overlapping: legato transitions
        note(500 + 600 * i, k + step, 700)
    for i in range(4):  # repetition / round robin
        note(3500 + 400 * i, k + 7, 250)
    ev.append((5300, "cc", 64, 127))  # pedal: detached notes under sustain
    note(5400, k, 200)
    note(5700, k + 4, 200)
    ev.append((6400, "cc", 64, 0))
    note(7000, k, 400, 40)
    note(7600, k, 400, 120)
    at = 8400
    for s in switches:
        note(at, s, 100)
        note(at + 200, k, 500)
        at += 1000
    # Mod-wheel sweep over a held note, then a note at full wheel.
    note(at, k, 2600)
    for i in range(9):
        ev.append((at + 200 + 250 * i, "cc", 1, min(127, 16 * i)))
    note(at + 3000, k + 2, 400)
    ev.append((at + 3600, "cc", 1, 90))
    return sorted(ev, key=lambda e: (e[0], e[1] != "off"))


def midi(ev, channel: int) -> bytes:
    def vlq(n):
        out = [n & 0x7F]
        n >>= 7
        while n:
            out.append((n & 0x7F) | 0x80)
            n >>= 7
        return bytes(reversed(out))

    trk, last = b"\x00\xff\x51\x03\x0f\x42\x40", 0  # 60 BPM, 1000 ticks/beat: 1 tick = 1 ms
    for ms, kind, a, b in ev:
        status = {"on": 0x90, "off": 0x80, "cc": 0xB0}[kind] | channel
        trk += vlq(ms - last) + bytes([status, a, b])
        last = ms
    trk += b"\x00\xff\x2f\x00"
    return b"MThd" + struct.pack(">IHHH", 6, 0, 1, 1000) + b"MTrk" + struct.pack(">I", len(trk)) + trk


TRACER = """on init
  declare !log[2000]
  declare $n := 0
  declare $z
  declare $id
  declare !keys[128]
  declare $k
  pgs_create_key(KTRACE, 1)
  $k := 0
  while ($k < 128)
    !keys[$k] := $k & "," & get_key_color($k) & "," & get_key_type($k) & "," & get_key_name($k)
    inc($k)
  end while
  save_array_str(!keys, "KEYS")
end on
on note
  $id := $EVENT_ID
  wait(1)
  $z := get_event_par($id, $EVENT_PAR_ZONE_ID)
  if ($z = -1)
    wait(10000)
    $z := get_event_par($id, $EVENT_PAR_ZONE_ID)
  end if
  if ($z = -1)
    wait(50000)
    $z := get_event_par($id, $EVENT_PAR_ZONE_ID)
  end if
  if ($n < 2000)
    !log[$n] := $ENGINE_UPTIME & "," & $EVENT_NOTE & "," & $EVENT_VELOCITY & "," ...
      & get_event_par($id, $EVENT_PAR_SOURCE) & "," & $z & "," ...
      & group_name(get_zone_par($z, $ZONE_PAR_GROUP)) & "," & get_sample($z, $NI_FILE_NAME)
    inc($n)
    pgs_set_key_val(KTRACE, 0, $n)
  end if
end on
on pgs_changed
  save_array_str(!log, "OUT")
end on
"""


def main() -> int:
    nki, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    key = int(sys.argv[3])
    switches = [int(s) for s in sys.argv[4].split(",")] if len(sys.argv) > 4 and sys.argv[4] else []
    out.mkdir(parents=True, exist_ok=True)
    ev = phrase(key, switches)
    channel = slot.channel_block()[0]
    # Kontakt loads samples after load_instrument returns: the MIDI phrase waits LEAD ms.
    lead = int(os.environ.get("TRACE_LEAD_MS", 45000))
    (out / "phrase.mid").write_bytes(midi([(ms + lead, *rest) for ms, *rest in ev], channel))
    kinds = {"on": "note_on", "off": "note_off", "cc": "cc"}
    (out / "phrase.json").write_text(json.dumps({"events": [
        {"frame": ms * RATE // 1000, "type": kinds[kind], **({"cc": a, "value": b} if kind == "cc" else {"key": a, "velocity": b})}
        for ms, kind, a, b in ev]}))
    nka = dump.winpath(out / "trace.nka").replace("\\", "/")
    tracer = TRACER.replace('"OUT"', '"' + nka + '"').replace('"KEYS"', '"' + nka.replace("trace.nka", "keys.nka") + '"')
    probe = out / "probe.lua"
    probe.write_text(f"""local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\\n") out:flush() end
log('{{"start":true}}')
kt.reset_multi()
local i = kt.load_instrument({dump.lua_str(dump.winpath(nki))})
log('{{"loaded":' .. tostring(i) .. '}}')
pcall(kt.set_instrument_midi_channel, i, {channel + 1})
-- A library may route to an output the standalone does not have: record, then use the first.
local okc, out_ch = pcall(kt.get_instrument_output_channel, i)
log('{{"output_channel":' .. tostring(out_ch) .. '}}')
pcall(kt.set_instrument_output_channel, i, 0)
local free = -1
for s = 4, 0, -1 do
  local ok, name = pcall(kt.get_instrument_script_name, i, s)
  local ok2, src = pcall(kt.get_instrument_script_source, i, s)
  if ok and (name == nil or name == "") and (not ok2 or src == nil or src == "") then free = s break end
end
local ok, err = pcall(kt.set_instrument_script_source, i, free, [==[{tracer}]==])
log(string.format('{{"slot":%d,"ok":%s,"err":"%s"}}', free, tostring(ok), tostring(err):gsub('"', "'"):sub(1, 200)))
log('{{"ready":true}}')
""")
    seconds = (ev[-1][0] + lead) / 1000 + 4
    return subprocess.run([sys.executable, str(HERE / "render.py"), str(probe), str(out / "kontakt.wav"),
                           str(seconds), str(out / "phrase.mid")]).returncode


if __name__ == "__main__":
    sys.exit(main())
