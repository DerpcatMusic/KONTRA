#!/usr/bin/env python3
"""Per-note gain vs the noise file: gain_notes.py REC.wav 'T1,T2,...' [NOISE.wav]. T = note start (s, relative to the first onset - 0.1).
Prints least-squares gain (dB) of each channel over 0.5-1.5 s into the note (silent -> -inf)."""
import sys, numpy as np; sys.argv += [None]*0
sys.path.insert(0, __file__.rsplit('/', 1)[0]); import importlib.util
sp = importlib.util.spec_from_file_location('cal', __file__.rsplit('/', 1)[0] + '/calibrate_lib.py'); cal = importlib.util.module_from_spec(sp); sp.loader.exec_module(cal)
rec, sr = cal.rd(sys.argv[1]); src, _ = cal.rd(sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] else '/tmp/noise.wav'); s = src[:, 0]
on = int(np.argmax(np.abs(rec).max(1) > 1e-5)); base = on - int(0.1*sr)
for t in sys.argv[2].split(','):
    a0 = base + int((float(t) + 0.1)*sr); ref = s[24000:72000]; out = []
    seg = rec[a0 + 24000 - 6000:a0 + 72000 + 6000]            # +-6000 samples of scheduling jitter
    for ch in (0, 1):
        x = seg[:, ch]; n = len(x) + len(ref); c = np.fft.irfft(np.fft.rfft(x, n) * np.conj(np.fft.rfft(ref, n)), n)[:len(x) - len(ref) + 1]
        lag = int(np.argmax(np.abs(c))); y = x[lag:lag + len(ref)]; g = (y @ ref) / (ref @ ref)
        out.append((20*np.log10(abs(g) + 1e-12) if np.abs(y).max() > 1e-7 else float('-inf'), lag - 6000))
    print(f"note@{t}: L {out[0][0]:+.3f} dB  R {out[1][0]:+.3f} dB  (lag {out[0][1]})")
