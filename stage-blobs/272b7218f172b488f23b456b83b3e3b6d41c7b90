#!/usr/bin/env python3
"""Per-note peak/RMS (dBFS) of a solo_group scenario: solo_report.py WAV G1,G2,... [SPACING=6] [ONSET=0.1]
RMS window 0.5-2.0 s after onset, peak 0.3-3.0 s after onset."""
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import compare, numpy as np
x = np.asarray(compare.wav(sys.argv[1])[0]); sr = 48000
sp = float(sys.argv[3]) if len(sys.argv) > 3 else 6; on = float(sys.argv[4]) if len(sys.argv) > 4 else 0.1
for i, g in enumerate(sys.argv[2].split(',')):
    t = i * sp + on; s = x[int((t+.3)*sr):int((t+3.0)*sr)]; r = x[int((t+.5)*sr):int((t+2)*sr)]
    print(f'g{g}: peak {20*np.log10(abs(s).max()+1e-9):.1f} rms {20*np.log10(np.sqrt((r**2).mean())+1e-9):.1f}')
