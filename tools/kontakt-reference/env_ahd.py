#!/usr/bin/env python3
"""AHD volume-envelope timing on the noise instrument: env_ahd.py WAV [LABEL]
Times are measured from the moment the 10 ms RMS first reaches the plateau (0.5 dB), i.e. after the attack. Prints the time after onset at which the 10 ms RMS first falls
1/3/6/10/20/30/40/50 dB below the plateau level (the median of the first 20 ms after onset)."""
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import compare, numpy as np
x = np.asarray(compare.wav(sys.argv[1])[0]); sr = 48000; n = int(.01*sr)
e = np.sqrt(np.convolve(x**2, np.ones(n)/n, 'same')); on = int(np.argmax(e > 1e-3)); ref = np.median(e[on+n:on+3*n])
d = 20*np.log10(e[on:on+int(8*sr)]/ref + 1e-12); pk = int(np.argmax(d > -0.5)); d, on = d[pk:], on + pk; t = np.arange(len(d))/sr  # measure from the first sample at the plateau
print(sys.argv[2] if len(sys.argv) > 2 else '', 'ref %.1f dB' % (20*np.log10(ref)), 'T below plateau by 1/3/6/10/20/30/40/50 dB (s):',
      ' '.join('%.3f' % t[np.argmax(d < -k)] if (d < -k).any() else 'n/a' for k in (1, 3, 6, 10, 20, 30, 40, 50)))
