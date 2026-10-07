#!/usr/bin/env python3
"""cutoff_report.py WAV REF.wav SPACING LABELS [WIN=1:4 | step=0.25:0:6]
Effective low-pass cutoff of the filtered noise in WAV, against REF (same scenario with the filter bypassed, so the noise
structure and the amp velocity gain cancel). Per note k (onset of note k = first onset + k*SPACING) and window t0:t1 s:
ratio of 1/6-octave band powers, plateau = mean of the bands 150-300 Hz, fc = where the ratio is 3 dB under the plateau.
WIN `step=D:A:B` prints one fc per D-second window over A..B s after the note onset (for LFO / envelope time curves).
fc here is the -3 dB point of the measured filter, not the knob value; compare against an unmodulated run."""
import struct, sys, numpy as np
def load(p):
    b = open(p, "rb").read(); i = 12
    while i + 8 <= len(b):
        t, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
        if t == b"data": return np.frombuffer(b[i+8:i+8+n//8*8], "<f4").reshape(-1, 2)[:, 0]
        i += 8 + n
fs = 48000; N = 8192; C = 150 * 2 ** (np.arange(0, 6.8, 1 / 6))
def bands(seg):
    P = np.mean([np.abs(np.fft.rfft(seg[j:j+N] * np.hanning(N)))**2 for j in range(0, len(seg) - N, N // 4)], axis=0); f = np.fft.rfftfreq(N, 1 / fs)
    return np.array([10 * np.log10(P[(f >= q * 2 ** (-1/12)) & (f < q * 2 ** (1/12))].mean() + 1e-30) for q in C])
def fc(seg, ref):
    r = bands(seg) - bands(ref)
    if bands(seg).max() < -100: return "silent"
    pl = r[:6].mean(); r = np.convolve(np.pad(r, 1, mode="edge"), np.ones(3) / 3, "valid")
    j = next((j for j in range(6, len(r)) if r[j] < pl - 3), None)
    return "above 16k" if j is None else f"{np.exp(np.interp(pl - 3, [r[j], r[j-1]], [np.log(C[j]), np.log(C[j-1])])):.0f} Hz (plateau {pl:+.1f} dB)"
if __name__ == "__main__":
    x, y = load(sys.argv[1]), load(sys.argv[2]); sp = float(sys.argv[3]); labels = sys.argv[4].split(","); w = sys.argv[5] if len(sys.argv) > 5 else "1:4"
    ox, oy = int(np.argmax(np.abs(x) > 1e-4)), int(np.argmax(np.abs(y) > 1e-4))
    for k, lab in enumerate(labels):
        if w.startswith("step="):
            d, a, b = map(float, w[5:].split(":")); ts = [(t, t + d) for t in np.arange(a, b, d)]
        else: ts = [tuple(map(float, w.split(":")))]
        for t0, t1 in ts:
            s, r = [int(o + (k * sp + t0) * fs) for o in (ox, oy)]; n = int((t1 - t0) * fs)
            print(f"{lab} {t0:.2f}-{t1:.2f}s: {fc(x[s:s+n], y[r:r+n])}")
