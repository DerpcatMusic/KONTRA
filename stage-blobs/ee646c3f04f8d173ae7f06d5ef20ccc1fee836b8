#!/usr/bin/env python3
"""Per-note L / R / combined RMS (dBFS) of a stereo f32 recording: lr_report.py WAV LABEL1,LABEL2,... [SPACING=6]
First note must be audible; the lead is taken from its onset (note at 0.6 s in the scenario). Window 0.5-2.0 s after each onset.
combined = sqrt((L^2+R^2)/2) (the agreed convention)."""
import sys, struct, numpy as np
b = open(sys.argv[1], "rb").read(); i = 12
while i + 8 <= len(b):
    tag, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
    if tag == b"data": x = np.frombuffer(b[i+8:i+8+n//8*8], "<f4").reshape(-1, 2).astype(float); break
    i += 8 + n
sr = 48000; sp = float(sys.argv[3]) if len(sys.argv) > 3 else 6
on0 = np.argmax(np.abs(x).max(1) > 1e-4) / sr
db = lambda v: 20*np.log10(v + 1e-9)
for k, lab in enumerate(sys.argv[2].split(",")):
    t = on0 + k*sp; w = x[int((t+.5)*sr):int((t+2)*sr)]
    l, r = np.sqrt((w[:, 0]**2).mean()), np.sqrt((w[:, 1]**2).mean())
    print(f"{lab}: L {db(l):.1f} R {db(r):.1f} L-R {db(l)-db(r):+.1f} comb {db(np.sqrt((l*l+r*r)/2)):.1f}")
