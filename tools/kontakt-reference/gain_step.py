#!/usr/bin/env python3
"""gain_step.py WAV STEP_DB: held noise note, GUI gain step mid-note. Fits the smoothing ramp.
The instrument-insert Gainer mixes dry and wet (out = (1-m) + m*g, m measured from the plateau, ~0.5 for a freshly added module),
so the model is fitted on the envelope, not on g directly. Envelope = RMS over 4 ms windows (hop 1 ms) of the L/R mean."""
import sys, struct, numpy as np
def stereo(path):
    b = open(path, "rb").read(); i = 12
    while i + 8 <= len(b):
        tag, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
        if tag == b"data": return np.frombuffer(b[i+8:i+8+n//8*8], "<f4").reshape(-1, 2).astype(float)
        i += 8 + n + (n & 1)
    raise SystemExit("no data chunk")
sr = 48000; x = stereo(sys.argv[1]).mean(axis=1); step = float(sys.argv[2]); g1 = 10 ** (step / 20)
w, h = 192, 48; hs = h / sr
n = (len(x) - w) // h
cs = np.concatenate(([0.0], np.cumsum(x ** 2)))
e = np.sqrt((cs[w:w + n * h:h][:n] - cs[0:n * h:h][:n]) / w); t = (np.arange(n) * h + w / 2) / sr
on = np.argmax(e > 1e-4)
a0 = e[on + int(1.0 / hs): on + int(2.0 / hs)].mean()
sm = np.convolve(e, np.ones(10) / 10, mode="same")   # 10 ms smoothing, only to locate the step
j = on + int(2.5 / hs); k = j + np.argmax(sm[j:] < 0.9 * a0 if g1 < 0.9 else sm[j:] > 1.1 * a0)
a1 = e[k + int(1.5 / hs): k + int(2.5 / hs)].mean()
m = (a1 / a0 - 1) / (g1 - 1)
print(f"pre {20*np.log10(a0):.2f} dB  plateau {20*np.log10(a1):.2f} dB  ({20*np.log10(a1/a0):+.2f} dB)  mix m={m:.3f}  (typed step {step:+.0f} dB)")
seg = slice(k - int(0.05 / hs), k + int(0.5 / hs)); ts, es = t[seg], e[seg] / a0
def env(g): return (1 - m) + m * g
def f_exp(d, c): return env(g1 + (1 - g1) * np.exp(-np.clip(d, 0, None) / (c / 1000)))
def f_lin(d, c): return env(1 + (g1 - 1) * np.clip(d / (c / 1000), 0, 1))
def f_exdb(d, c): return env(g1 ** (1 - np.exp(-np.clip(d, 0, None) / (c / 1000))))
def f_db(d, c): return env(g1 ** np.clip(d / (c / 1000), 0, 1))
t0s = np.arange(t[k] - 0.06, t[k] + 0.005, 0.0005); cs_ = np.arange(1, 400, 1.0)
res = {}
for name, f in (("one-pole (amplitude)", f_exp), ("linear amplitude", f_lin), ("linear in dB", f_db), ("one-pole in dB", f_exdb)):
    best = min((np.sqrt(np.mean((f(ts - t0, c) - es) ** 2)), t0, c) for t0 in t0s for c in cs_)
    res[name] = best; print(f"  {name:21s} start {best[1]*1000:8.1f} ms  const {best[2]:6.1f} ms  rms resid {best[0]*100:.2f}% of pre level")
# model-free: time from step start (first window < 0.99*(1-m)...) to 63% of the transition
fr = (es - env(g1)) / (1 - env(g1)); i0 = np.argmax(fr < 0.97); i63 = np.argmax(fr < 0.368); i10 = np.argmax(fr < 0.1)
print(f"  model-free (4 ms windows): 97%->36.8% {1000*(ts[i63]-ts[i0]):.0f} ms, 97%->10% {1000*(ts[i10]-ts[i0]):.0f} ms")
