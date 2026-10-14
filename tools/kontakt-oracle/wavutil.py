"""Minimal float WAV reader for render.py recordings (no numpy needed for the peak scan)."""
import array
import struct


def read(path):
    d = open(path, "rb").read()
    pos = 12
    ch = rate = 2, 48000
    ch, rate, fmt = 2, 48000, 3
    while pos + 8 <= len(d):
        tag, size = d[pos:pos + 4], struct.unpack("<I", d[pos + 4:pos + 8])[0]
        body = d[pos + 8:pos + 8 + size]
        if tag == b"fmt ":
            fmt, ch, rate = struct.unpack("<HHI", body[:8])
        if tag == b"data":
            size = min(size, len(d) - pos - 8)
            a = array.array("f")
            a.frombytes(d[pos + 8:pos + 8 + size // 4 * 4])
            return ch, rate, [a[i::ch] for i in range(ch)]
        pos += 8 + size + (size & 1)
    raise ValueError("no data chunk")


if __name__ == "__main__":
    import sys

    ch, rate, chans = read(sys.argv[1])
    n = len(chans[0])
    print(ch, rate, n / rate, "s")
    thr = 0.01
    hits = [i for i, x in enumerate(chans[0]) if abs(x) > thr]
    last = -10**9
    for i in hits:
        if i - last > rate // 4:
            print(f"event at {i / rate:.3f}s peak {max(abs(x) for x in chans[0][i:i + 2000]):.4f}")
        last = i
