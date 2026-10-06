#!/usr/bin/env python3
"""Filter response from noise renders. Usage: fx_response.py REF.wav FILTERED.wav [--cut Hz]
Prints magnitude ratio (dB) of filtered vs reference PSD at octave-ish points, the -3 dB
frequency (relative to the passband median 20-100 Hz... the low band), and the slope per octave
over the decade above it. Needs numpy only."""
import sys, numpy as np
sys.path.insert(0, __file__.rsplit('/', 1)[0])
import compare as c

def psd(path):
    x, sr = c.wav(path); o = c.onset(x, sr); x = x[o + int(0.3 * sr): o + int(2.0 * sr)]
    n = 16384; w = np.hanning(n); acc = 0; k = 0
    for s in range(0, len(x) - n, n // 2):
        acc = acc + np.abs(np.fft.rfft(x[s:s + n] * w)) ** 2; k += 1
    return np.fft.rfftfreq(n, 1 / sr), acc / k

def response(ref, flt):
    f, a = psd(ref); _, b = psd(flt)
    sm = lambda v: np.convolve(v, np.ones(15) / 15, 'same')
    return f, 10 * np.log10(np.maximum(sm(b), 1e-30) / np.maximum(sm(a), 1e-30))

def summary(f, d):
    low = np.median(d[(f > 30) & (f < 60)])
    rel = d - low
    idx = np.nonzero((rel < -3) & (f > 30))[0]
    fc = f[idx[0]] if len(idx) else float('nan')
    pts = [fc * 2 ** k for k in (1, 2, 3)]
    vals = [np.interp(p, f, rel) for p in pts if p < f[-1] * 0.9]
    slope = (vals[-1] - vals[0]) / (len(vals) - 1) if len(vals) > 1 else float('nan')
    return low, fc, rel.min(), rel.max(), slope

if __name__ == '__main__':
    f, d = response(sys.argv[1], sys.argv[2])
    low, fc, mn, mx, slope = summary(f, d)
    print(f"passband {low:+.2f} dB  fc(-3dB) {fc:.0f} Hz  max {mx:+.2f}  slope {slope:.1f} dB/oct over fc..4fc")
