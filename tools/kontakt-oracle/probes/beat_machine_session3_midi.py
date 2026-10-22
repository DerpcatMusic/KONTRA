#!/usr/bin/env python3
"""Writes the MIDI file that drives beat_machine_session3.lua (format 0, 1000 ticks per quarter,
tempo 1000000 us per quarter, so one tick is exactly 1 ms).

    beat_machine_session3_midi.py OUT.mid

Notes (key, start ms, end ms): zone A key 48, zone B key 72, zone C key 96; each zone is retriggered
after its sequence ends, and A also gets a short-hold note. Velocity 100, channel 1.
"""
import struct
import sys
from pathlib import Path

NOTES = [
    (48, 0, 1200),      # A, first trigger, 1.2 s hold (sequence 0.5 s)
    (48, 2000, 3200),   # A, retrigger after the sequence ended
    (48, 3600, 3750),   # A, short hold (150 ms), sequence still running
    (72, 4500, 5500),   # B, first trigger, 1 s hold (sequence 0.667 s)
    (72, 6000, 6300),   # B, retrigger, short hold
    (96, 7000, 8600),   # C, first trigger, 1.6 s hold (sequence 1.2 s)
    (96, 9000, 9400),   # C, retrigger, short hold
]


def vlq(n: int) -> bytes:
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.append((n & 0x7F) | 0x80)
        n >>= 7
    return bytes(reversed(out))


def main() -> None:
    events = [(0, bytes([0xFF, 0x51, 0x03]) + (1000000).to_bytes(3, "big"))]
    for key, on, off in NOTES:
        events.append((on, bytes([0x90, key, 100])))
        events.append((off, bytes([0x80, key, 0])))
    events.sort(key=lambda e: e[0])  # no two events share a ms in NOTES, so the order is fixed
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
