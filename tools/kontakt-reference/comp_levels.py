#!/usr/bin/env python3
"""Compressor-calibration levels: comp_levels.py WAV [NOTE_LEN=3] [N=1] [SPACING=6]. Note k starts at first-onset + k*SPACING.
Per channel (and L+R power sum): peak over the held note, RMS over 1.0 s .. NOTE_LEN, plus 0.5 s RMS windows (dBFS)."""
import sys
import numpy as np, struct
def stereo(path):
    b = open(path, "rb").read(); i = 12
    while i + 8 <= len(b):
        tag, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
        if tag == b"data": return np.frombuffer(b[i+8:i+8+n//8*8], "<f4").reshape(-1, 2).astype(float)
        i += 8 + n + (n & 1)
    raise SystemExit("no data chunk")
x = stereo(sys.argv[1]); sr = 48000; L = float(sys.argv[2]) if len(sys.argv) > 2 else 3; n = int(sys.argv[3]) if len(sys.argv) > 3 else 1
sp = float(sys.argv[4]) if len(sys.argv) > 4 else 6; db = lambda a: 20*np.log10(a + 1e-12)
t0 = np.argmax(abs(x).max(1) > 1e-4) / sr
for k in range(n):
    t = t0 + k*sp; s = x[int(t*sr):int((t+L)*sr)]; r = x[int((t+1)*sr):int((t+L)*sr)]
    w = [db(np.sqrt((x[int((t+a)*sr):int((t+a+.5)*sr)]**2).mean())) for a in np.arange(0, L, .5)]
    print(f"note{k}: pk L {db(abs(s[:,0]).max()):.2f} R {db(abs(s[:,1]).max()):.2f} | rms1-{L:g}s L {db(np.sqrt((r[:,0]**2).mean())):.2f} R {db(np.sqrt((r[:,1]**2).mean())):.2f} sum {db(np.sqrt((r**2).sum(1).mean())):.2f} | 0.5s rms(L+R): " + " ".join(f"{v:.1f}" for v in w))
