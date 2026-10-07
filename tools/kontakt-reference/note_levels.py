#!/usr/bin/env python3
"""Per-note L/R peak and RMS (dBFS): note_levels.py WAV [SPACING=6] [N=1]. Note k starts at first-onset + k*SPACING;
peak over the whole note (to SPACING-0.2 s), RMS over 0.5-2.0 s after its onset. Onset = first sample > 1e-4."""
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import compare, numpy as np
x = np.asarray(compare.wav(sys.argv[1])[0]).reshape(-1, 2); sr = 48000
sp = float(sys.argv[2]) if len(sys.argv) > 2 else 6; n = int(sys.argv[3]) if len(sys.argv) > 3 else 1
t0 = np.argmax(abs(x).max(1) > 1e-4) / sr; f = lambda a: 20*np.log10(a + 1e-12)
for k in range(n):
    t = t0 + k*sp; s = x[int(t*sr):int((t+sp-.2)*sr)]; r = x[int((t+.5)*sr):int((t+2)*sr)]
    print(f"note{k}: " + " ".join(f"{'LR'[c]} pk {f(abs(s[:,c]).max()):.1f} rms {f(np.sqrt((r[:,c]**2).mean())):.1f}" for c in (0, 1)))
