#!/usr/bin/env python3
"""zone_rms.py WAV N [SPACING=3]: first note onset defines the lead; prints RMS dB (0.5-1.4 s after each onset)."""
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import compare, numpy as np
x = np.asarray(compare.wav(sys.argv[1])[0]); sr = 48000; sp = float(sys.argv[3]) if len(sys.argv) > 3 else 3
n = int(.01*sr); e = np.sqrt(np.convolve(x**2, np.ones(n)/n, 'same')); t0 = np.argmax(e > 1e-4)/sr - 0.005
for k in range(int(sys.argv[2])):
    s = x[int((t0+sp*k+.5)*sr):int((t0+sp*k+1.4)*sr)]; print(k, '%.2f' % (20*np.log10(np.sqrt((s**2).mean())+1e-12)))
