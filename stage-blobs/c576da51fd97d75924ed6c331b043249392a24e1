#!/usr/bin/env python3
"""Summarise wav/fx_t_<type>_{0,50,80}.wav (cutoff 1000 Hz): -3 dB freq, slope, peak dB/freq, passband gain.
Usage: fx_types_report.py [WAVDIR]. Reference: fx_svlp2_ref.wav (filter bypassed, same note/velocity)."""
import sys, glob, os, numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fx_response as r
W = sys.argv[1] if len(sys.argv) > 1 else '/home/derpcat/.cache/kontra-reference/wav'
ref = f'{W}/fx_svlp2_ref.wav'
types = sorted({os.path.basename(p)[3:].rsplit('_', 1)[0] for p in glob.glob(f'{W}/fx_t_*_0.wav')})
print(f"{'type':12}{'reso':>5}{'pass dB':>9}{'fc3dB':>8}{'slope/oct':>10}{'peak dB':>9}{'peak Hz':>9}")
for t in types:
    for q in (0, 50, 80):
        p = f'{W}/fx_{t}_{q}.wav'
        if not os.path.exists(p): continue
        f, d = r.response(ref, p)
        low, fc, mn, mx, slope = r.summary(f, d)
        m = (f > 100) & (f < 10000); pk = f[m][np.argmax(d[m])]
        print(f"{t[2:]:12}{q:5d}{low:9.2f}{fc:8.0f}{slope:10.1f}{d[m].max()-low:9.1f}{pk:9.0f}")
