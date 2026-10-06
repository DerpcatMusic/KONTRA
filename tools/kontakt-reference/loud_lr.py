#!/usr/bin/env python3
"""Full-note levels per channel: loud_lr.py WAV SPACING LABEL1,LABEL2,... Onset = first sample > 1e-4.
Per note: L/R peak (0.3-3 s after onset), max(L,R) peak, RMS sqrt((L^2+R^2)/2) over 0.5-2 s, and the mono-mix (L+R)/2 peak for comparison."""
import sys, struct, numpy as np
b = open(sys.argv[1], "rb").read(); i = 12
while i + 8 <= len(b):
    tag, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
    if tag == b"data": x = np.frombuffer(b[i+8:i+8+n//8*8], "<f4").reshape(-1, 2).astype(float); break
    i += 8 + n
sr = 48000; sp = float(sys.argv[2]); on0 = np.argmax(np.abs(x).max(1) > 1e-4) / sr
db = lambda v: 20*np.log10(v + 1e-9)
for k, lab in enumerate(sys.argv[3].split(",")):
    t = on0 + k*sp; s = x[int(t*sr):int((t+sp-0.2)*sr)]; w = x[int((t+.5)*sr):int((t+2)*sr)]
    pl, pr = abs(s[:, 0]).max(), abs(s[:, 1]).max(); pm = abs(s.mean(1)).max()
    rms = np.sqrt((w**2).mean())
    print(f"{lab}: Lpk {db(pl):.1f} Rpk {db(pr):.1f} maxpk {db(max(pl,pr)):.1f} monopk {db(pm):.1f} rms(L,R) {db(rms):.1f}")
