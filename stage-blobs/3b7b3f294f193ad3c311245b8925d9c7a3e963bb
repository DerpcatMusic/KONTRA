#!/usr/bin/env python3
"""Reverb analysis: reverb_report.py WET.wav DRY.wav [LABEL]
DRY = same scenario with the reverb bypassed. wet = WET - DRY (aligned by cross-correlation).
Prints: pre-delay (first wet window 30 dB under the wet peak), peak time, broadband RT60 from the Schroeder
integral (-5..-35 dB, extrapolated to 60), and RT60 per octave band (250, 1k, 4k, 8k Hz) plus L/R correlation of the tail."""
import sys, struct, numpy as np
def rd(p):
    b = open(p, "rb").read(); i = 12
    while i + 8 <= len(b):
        t, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
        if t == b"data": return np.frombuffer(b[i+8:i+8+n//8*8], "<f4").reshape(-1, 2).astype(float)
        i += 8 + n
sr = 48000
w, d = rd(sys.argv[1]), rd(sys.argv[2])
on = lambda x: int(np.argmax(np.abs(x).max(1) > 1e-4))
w, d = w[on(w):], d[on(d):]
n = min(len(w), len(d)); w, d = w[:n], d[:n]
seg = slice(0, int(.05*sr)); best = (1e9, 0)
for lag in range(-8, 9):
    r = np.roll(d, lag, 0); e = ((w[seg]-r[seg])**2).sum()
    if e < best[0]: best = (e, lag)
wet = w - np.roll(d, best[1], 0)
db = lambda v: 20*np.log10(v + 1e-12)
def rt(x, lo=-5, hi=-35):
    e = (x**2).sum(1) if x.ndim > 1 else x**2
    edc = np.cumsum(e[::-1])[::-1]; edc = 10*np.log10(edc/edc.max() + 1e-30)
    t = np.arange(len(edc))/sr; m = (edc <= lo) & (edc >= hi)
    if m.sum() < 50: return float("nan")
    s = np.polyfit(t[m], edc[m], 1)[0]; return -60/s
win = int(.005*sr); env = np.array([np.sqrt((wet[i:i+win]**2).mean()) for i in range(0, len(wet)-win, win)])
pk = env.max(); first = int(np.argmax(env > pk*10**(-30/20)))
print(f"{sys.argv[3] if len(sys.argv)>3 else ''} lag={best[1]} predelay(-30dB)={first*5:.0f} ms peak@{np.argmax(env)*5} ms RT60={rt(wet):.2f} s  wet_rms_peak={db(pk):.1f} dB")
out = []
for fc in (250, 1000, 4000, 8000):
    F = np.fft.rfft(wet, axis=0); f = np.fft.rfftfreq(len(wet), 1/sr)
    F *= np.exp(-0.5*(np.log2(np.maximum(f, 1)/fc)/0.5)**2)[:, None]  # smooth band (sigma 0.5 oct), no brick-wall ringing
    out.append(f"{fc}:{rt(np.fft.irfft(F, len(wet), axis=0)):.2f}")
tail = wet[int(.5*sr):int(2*sr)]
c = np.corrcoef(tail[:, 0], tail[:, 1])[0, 1] if len(tail) else float("nan")
print("  band RT60 s  " + "  ".join(out) + f"   LR corr(0.5-2s)={c:.2f}")
