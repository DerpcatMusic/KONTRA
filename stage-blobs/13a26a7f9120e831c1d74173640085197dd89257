#!/usr/bin/env python3
"""Per-note peak and sustained RMS of two renders. Usage: loudness.py KONTAKT.wav KONTRA.wav NOTE_SPACING_S VEL...
Notes are spaced NOTE_SPACING_S apart starting at the first onset; RMS is over 0.5..2.0 s after each onset.
Prints dB values and kontra-kontakt offsets (raw, and +6 dB for Kontakt's default -6 dB instrument volume)."""
import sys, os, numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import compare as c
ref, me, sp, vels = sys.argv[1], sys.argv[2], float(sys.argv[3]), sys.argv[4:]
def stats(path):
    x, sr = c.wav(path); o = c.onset(x, sr); out = []
    for i in range(len(vels)):
        s = o + int(i * sp * sr); seg = x[s:s + int(sp * sr)]; sus = x[s + int(.5 * sr): s + int(2.0 * sr)]
        out.append((float(c.db(np.abs(seg).max())), float(c.db(np.sqrt((sus ** 2).mean())))))
    return out
a, b = stats(ref), stats(me)
print(f"{'vel':>4} {'kontakt pk':>10} {'kontra pk':>10} {'d pk':>6} {'kontakt rms':>11} {'kontra rms':>10} {'d rms':>6} {'d rms +6':>9}")
for v, (kp, kr), (mp, mr) in zip(vels, a, b):
    print(f"{v:>4} {kp:10.1f} {mp:10.1f} {mp-kp:6.1f} {kr:11.1f} {mr:10.1f} {mr-kr:6.1f} {mr-kr-6:9.1f}")
