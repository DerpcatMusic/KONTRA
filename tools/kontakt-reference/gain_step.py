#!/usr/bin/env python3
"""gain_step.py WAV [EXPECT_DB]: find the gain step in a held noise note and fit the ramp.
Envelope = RMS over 2 ms windows (hop 0.5 ms) of the L/R mean. Fits one-pole (amplitude exp), linear-amplitude ramp, linear-dB ramp."""
import sys, numpy as np
import struct
def stereo(path):
    b = open(path, "rb").read(); i = 12
    while i + 8 <= len(b):
        tag, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
        if tag == b"data": return np.frombuffer(b[i+8:i+8+n//8*8], "<f4").reshape(-1, 2).astype(float)
        i += 8 + n + (n & 1)
    raise SystemExit("no data chunk")
x = stereo(sys.argv[1]).mean(axis=1); sr = 48000
w, h = 96, 24
n = (len(x) - w) // h
e = np.sqrt(np.array([np.mean(x[i*h:i*h+w]**2) for i in range(n)])); t = (np.arange(n) * h + w / 2) / sr
on = np.argmax(e > 1e-4); 
pre = e[on + int(1.0 / (h / sr)): on + int(2.0 / (h / sr))].mean()   # level 1-2 s after onset, before the step
# step = first time after 2.5 s the envelope drops below 0.8*pre
i0 = on + int(2.5 / (h / sr)); k = i0 + np.argmax(e[i0:] < 0.8 * pre); 
post = np.median(e[-int(1.0 / (h / sr)):-int(0.3 / (h / sr))])
tail = e[k + int(1.5 / (h / sr)): k + int(2.5 / (h / sr))].mean()
print(f"pre {20*np.log10(pre):.2f} dB  post {20*np.log10(tail):.2f} dB  step {20*np.log10(tail/pre):.2f} dB  (expect {sys.argv[2] if len(sys.argv)>2 else '?'})")
a1, a0 = tail, pre
seg = slice(k - int(0.15 / (h / sr)), k + int(0.6 / (h / sr))); ts, es = t[seg], e[seg]
def m_exp(t0, c): d = np.clip(ts - t0, 0, None); return a1 + (a0 - a1) * np.exp(-d / (c / 1000))
def m_lin(t0, c): d = np.clip((ts - t0) / (c / 1000), 0, 1); return a0 + (a1 - a0) * d
def m_db(t0, c): d = np.clip((ts - t0) / (c / 1000), 0, 1); return a0 * (a1 / a0) ** d
t0s = np.arange(t[k] - 0.12, t[k] + 0.03, 0.0005); cs = np.arange(0.5, 200, 0.5)
for name, f in (("one-pole(amp)", m_exp), ("linear amp", m_lin), ("linear dB", m_db)):
    best = min(((np.sqrt(np.mean(((f(t0, c) - es) / a0) ** 2)), t0, c) for t0 in t0s for c in cs))
    print(f"  {name:14s} t0 {best[1]*1000:8.1f} ms  const {best[2]:7.2f} ms  rel.rms {best[0]*100:.2f}%")
# model-free: 10-90% fall time and 63% time of the amplitude step
f = (e[seg] - a1) / (a0 - a1); idx = np.where(f < 0.9)[0][0]; i90 = idx; i10 = np.where(f < 0.1)[0][0]; i37 = np.where(f < 0.368)[0][0]
print(f"  model-free: 90%->10% fall {1000*(ts[i10]-ts[i90]):.1f} ms   t(90%) {1000*ts[i90]:.1f}  t(36.8%) {1000*ts[i37]:.1f}  -> 63%-time after 90% point {1000*(ts[i37]-ts[i90]):.1f} ms")
