#!/usr/bin/env python3
"""state.py OUT.state NKI -- a standalone KONTRA state whose rack holds one instrument, for
`<standalone> --state OUT.state` (the real window loads it at start)."""
import json
import struct
import sys

# KONTRA's plugin id in the state header (a state for another plugin is refused).
PLUGIN = bytes.fromhex("3c0598bed56cecaf")


def sized(b):
    return struct.pack("<I", len(b)) + b


def state(nki):
    rack = json.dumps({"version": 1, "parts": [{"part": 0, "path": nki, "channel": 0, "output": 0,
                       "gain": 1.0, "pan": 0.0, "solo": False, "mute": False, "scripts": []}]},
                      separators=(",", ":")).encode()
    body = struct.pack("<I", 2) + sized(b"rack") + sized(sized(rack)) + sized(b"editor") + sized(bytes(4))
    head = b"OAST" + struct.pack("<I", 1) + PLUGIN + struct.pack("<I", 2) + bytes(12) \
        + struct.pack("<I", 1) + bytes(16)
    return head + struct.pack("<Q", len(body)) + body


if __name__ == "__main__":
    open(sys.argv[1], "wb").write(state(sys.argv[2]))
