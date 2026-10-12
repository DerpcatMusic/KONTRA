#!/usr/bin/env python3
"""Attack envelope shape. env_report.py WAV N [SPACING=9]: note 0 is an attack-0 reference note at 0.1 s that fixes the
recording lead; note k (1..N) starts at SPACING*k + 0.1. Prints the time from note-on at which the 10 ms RMS reaches
1/5/10/25/50/75/90/95 % of the sustain level (median 3.5-5 s)."""
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import compare, numpy as np
x = np.asarray(compare.wav(sys.argv[1])[0]); sr = 48000; n = int(.01*sr); sp = float(sys.argv[3]) if len(sys.argv) > 3 else 9
e = np.sqrt(np.convolve(x**2, np.ones(n)/n, 'same')); lead = np.argmax(e > 1e-3)/sr - 0.1 - 0.005
for k in range(1, int(sys.argv[2])+1):
    on = int((lead + sp*k + .1)*sr); s = e[on:on+int(8*sr)]; sus = np.median(s[int(3.5*sr):int(5*sr)])
    print(k-1, 'sus %.1f dB' % (20*np.log10(sus+1e-12)), 'T@1/5/10/25/50/75/90/95%:', ' '.join('%.3f' % (np.argmax(s >= f*sus)/sr) for f in (.01, .05, .1, .25, .5, .75, .9, .95)))
