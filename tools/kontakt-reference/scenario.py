#!/usr/bin/env python3
"""Scenario text -> Standard MIDI File. Usage: scenario.py SCENARIO.txt OUT.mid

One event per line, `#` comments. Times in seconds (120 bpm, 480 ppq => 960 ticks/s).
  T on KEY VEL        T off KEY            T note KEY VEL LEN
  T cc N VALUE        T pedal VALUE        T bend -1..1        T pc N
Optional leading `ch=N` (0-15) on any line. A 1 s tail is not added; end the
scenario with `T end` to pad the file to T.
"""
import sys

TPS = 960


def vlq(n):
    out = [n & 0x7F]
    while n >> 7:
        n >>= 7
        out.append(n & 0x7F | 0x80)
    return bytes(reversed(out))


def parse(text):
    ev, end = [], 0.0  # (tick, order, bytes)
    for line in text.splitlines():
        line = line.split("#")[0].split()
        if not line:
            continue
        ch = 0
        if line[0].startswith("ch="):
            ch, line = int(line[0][3:]), line[1:]
        t, kind, a = float(line[0]), line[1], [float(x) for x in line[2:]]
        tick = round(t * TPS)
        if kind == "end":
            end = t
        elif kind == "on":
            ev.append((tick, 1, bytes([0x90 | ch, int(a[0]), int(a[1])])))
        elif kind == "off":
            ev.append((tick, 0, bytes([0x80 | ch, int(a[0]), 0])))
        elif kind == "note":
            ev.append((tick, 1, bytes([0x90 | ch, int(a[0]), int(a[1])])))
            ev.append((round((t + a[2]) * TPS), 0, bytes([0x80 | ch, int(a[0]), 0])))
        elif kind in ("cc", "pedal"):
            n, v = (64, a[0]) if kind == "pedal" else (int(a[0]), a[1])
            ev.append((tick, 0, bytes([0xB0 | ch, n, int(v)])))
        elif kind == "bend":
            v = max(0, min(16383, round(8192 + a[0] * 8192)))
            ev.append((tick, 0, bytes([0xE0 | ch, v & 127, v >> 7])))
        elif kind == "pc":
            ev.append((tick, 0, bytes([0xC0 | ch, int(a[0])])))
        else:
            raise SystemExit(f"unknown event: {line}")
    return sorted(ev, key=lambda e: e[:2]), end


def smf(text):
    ev, end = parse(text)
    body, last = b"", 0
    for tick, _, msg in ev:
        body += vlq(tick - last) + msg
        last = tick
    body += vlq(max(0, round(end * TPS) - last)) + b"\xff\x2f\x00"
    tempo = b"\x00\xff\x51\x03\x07\xa1\x20"  # 500000 us/beat
    body = tempo + body
    return b"MThd" + (6).to_bytes(4, "big") + (0).to_bytes(2, "big") + (1).to_bytes(2, "big") \
        + (480).to_bytes(2, "big") + b"MTrk" + len(body).to_bytes(4, "big") + body


if __name__ == "__main__":
    open(sys.argv[2], "wb").write(smf(open(sys.argv[1]).read()))
