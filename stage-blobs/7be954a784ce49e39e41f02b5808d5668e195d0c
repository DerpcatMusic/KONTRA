#!/usr/bin/env python3
"""matrix_report.py WAV [T0=0.5 T1=3.0]: the 2x2 matrix M with out = M [A, B] for the stereo noise sample (make_noise.py stereo).
Least-squares fit of the recorded L/R over T0..T1 s after onset on the known independent noise channels A, B (same seed).
Prints M (linear), per-element dB, the fit residual (dB re output) and M/S gains: mid = (M00+M01+M10+M11)/2-style summary."""
import struct, sys, numpy as np
def load(p):
    b = open(p, "rb").read(); i = 12
    while i + 8 <= len(b):
        t, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
        if t == b"data": return np.frombuffer(b[i+8:i+8+n//8*8], "<f4").reshape(-1, 2).astype(float)
        i += 8 + n
r = np.random.default_rng(1)
def nz(n): x = r.standard_normal(n); return x * 0.5 / np.abs(x).max()
fs = 48000; A, B = nz(fs * 20), nz(fs * 20)
x = load(sys.argv[1]); t0 = float(sys.argv[2]) if len(sys.argv) > 2 else 0.5; t1 = float(sys.argv[3]) if len(sys.argv) > 3 else 3.0
on = int(np.argmax(np.abs(x).max(axis=1) > 1e-4))
best = None
for lag in range(-300, 300):  # sample alignment: maximise the fit
    s = on + lag; a = int(t0 * fs); n = int((t1 - t0) * fs)
    if s < 0: continue
    X = np.stack([A[a:a+n], B[a:a+n]], 1); Y = x[s + a:s + a + n]
    M = np.linalg.lstsq(X, Y, rcond=None)[0].T; res = np.sum((Y - X @ M.T) ** 2) / np.sum(Y ** 2)
    if best is None or res < best[0]: best = (res, lag, M)
res, lag, M = best
db = lambda v: 20 * np.log10(abs(v) + 1e-12)
print(f"lag {lag}  residual {10*np.log10(res+1e-12):.1f} dB")
print(f"M = [[{M[0,0]:+.4f} {M[0,1]:+.4f}] [{M[1,0]:+.4f} {M[1,1]:+.4f}]]   dB [[{db(M[0,0]):.1f} {db(M[0,1]):.1f}] [{db(M[1,0]):.1f} {db(M[1,1]):.1f}]]")
