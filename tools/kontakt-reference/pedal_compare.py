#!/usr/bin/env python3
"""Per-scenario RMS envelope (dBFS, 0.5 s bins) of a pedal_suite render pair.
Usage: pedal_compare.py NAME   (reads ~/.cache/kontra-reference/wav/suite_NAME.{kontakt,kontra}.wav)"""
import sys, os, numpy as np
sys.path.insert(0, os.path.dirname(__file__)); import compare
W = os.path.expanduser('~/.cache/kontra-reference/wav/suite_%s.%s.wav')
names = ['sustain','retrigger','pedal_after_notes','pedal_up_while_held','half_pedal','sostenuto','sostenuto_after','soft_pedal']
d = {k: np.asarray(compare.wav(W % (sys.argv[1], k))[0]) for k in ('kontakt', 'kontra')}
sr = 48000
def env(x, i):
    s = x[i*14*sr:(i*14+13)*sr]; n = len(s)//(sr//2)
    return [20*np.log10(np.sqrt(np.mean(s[j*sr//2:(j+1)*sr//2]**2))+1e-9) for j in range(n)]
for i, n in enumerate(names):
    print(n)
    for k in d:
        e = env(d[k], i); print(f'  {k:8s}', ' '.join(f'{v:4.0f}' for v in e[:26]))
