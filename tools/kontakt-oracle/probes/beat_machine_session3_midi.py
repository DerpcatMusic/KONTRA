#!/usr/bin/env python3
"""Writes the MIDI file that drives the Beat Machine oracle probes (format 0, 1000 ticks per quarter).

    beat_machine_session3_midi.py OUT.mid [BPM] [SCENARIO]

BPM (default 60) sets the tempo meta only; note times stay in ms (one tick = 60/BPM ms).
SCENARIO selects the note list (channel 1):
  session3  (default) zone A key 48, zone B key 72, zone C key 96, velocity 100. Used by
            beat_machine_session3.lua and beat_machine_session4.lua.
  single    key 48 twice (0 ms and 2000 ms, 1.2 s holds), velocity 100. Single-zone probe A.
  velocity  velocity sweep 32, 64, 100, 127 on key 48 (sampler zone, 0 to 6 s) and on key 96
            (Beat zone, 8 to 14 s); holds 1.2 s. Used by beat_machine_velocity.lua.
"""
import struct
import sys
from pathlib import Path

# (key, start ms, end ms, velocity)
SCENARIOS = {
    "session3": [
        (48, 0, 1200, 100),      # A, first trigger, 1.2 s hold (sequence 0.5 s)
        (48, 2000, 3200, 100),   # A, retrigger after the sequence ended
        (48, 3600, 3750, 100),   # A, short hold (150 ms), sequence still running
        (72, 4500, 5500, 100),   # B, first trigger, 1 s hold (sequence 0.667 s)
        (72, 6000, 6300, 100),   # B, retrigger, short hold
        (96, 7000, 8600, 100),   # C, first trigger, 1.6 s hold (sequence 1.2 s)
        (96, 9000, 9400, 100),   # C, retrigger, short hold
    ],
    "single": [
        (48, 0, 1200, 100),
        (48, 2000, 3200, 100),
    ],
    "velocity": [
        *[(48, 2000 * i, 2000 * i + 1200, v) for i, v in enumerate([32, 64, 100, 127])],
        *[(96, 8000 + 2000 * i, 8000 + 2000 * i + 1200, v) for i, v in enumerate([32, 64, 100, 127])],
    ],
}


def vlq(n: int) -> bytes:
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.append((n & 0x7F) | 0x80)
        n >>= 7
    return bytes(reversed(out))


def main() -> None:
    bpm = float(sys.argv[2]) if len(sys.argv) > 2 else 60.0
    notes = SCENARIOS[sys.argv[3] if len(sys.argv) > 3 else "session3"]
    tick = lambda ms: round(ms * bpm / 60)  # 1000 ticks per quarter: one tick is 60000/(1000*bpm) ms
    events = [(0, bytes([0xFF, 0x51, 0x03]) + round(60_000_000 / bpm).to_bytes(3, "big"))]
    for key, on, off, vel in notes:
        events.append((tick(on), bytes([0x90, key, vel])))
        events.append((tick(off), bytes([0x80, key, 0])))
    events.sort(key=lambda e: e[0])  # the note lists have no two events at the same tick
    track = b""
    last = 0
    for t, data in events:
        track += vlq(t - last) + data
        last = t
    track += vlq(0) + bytes([0xFF, 0x2F, 0x00])
    header = b"MThd" + struct.pack(">IHHH", 6, 0, 1, 1000)
    Path(sys.argv[1]).write_bytes(header + b"MTrk" + struct.pack(">I", len(track)) + track)


if __name__ == "__main__":
    main()
