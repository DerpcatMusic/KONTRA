#!/usr/bin/env python3
"""Chain calibration: calibrate.py REC.wav [NOISE.wav] [EXPECT_DB] -> exits 1 unless the recorded noise is at EXPECT_DB (+-0.1 dB per channel) relative to the file (0.0 with the protocol's explicit CC7=127).
With the master at 0.00 dB and CC7=127 sent, the chain passes the file at 0 dB (CC10=64 makes L/R differ by 0.07 dB). With CC7 NEVER sent Kontakt outputs exactly -6.0206 dB (0.5x).
REC = scenarios/calibration.txt played on the bare noise instrument (note 60 vel 127 at 0 s, vel 100 at 6 s, no envelope/filter).
Per note: least-squares gain of the recorded channel against the source file, aligned by cross-correlation (lag in +-2000 samples
around the first onset), residual in dB. Only the vel-127 note is the gate; the vel-100 note is reported for the velocity law."""
import sys, struct, numpy as np
def rd(p):
    b = open(p, "rb").read(); i = 12; fmt = None
    while i + 8 <= len(b):
        t, n = b[i:i+4], struct.unpack("<I", b[i+4:i+8])[0]
        if t == b"fmt ": fmt = struct.unpack("<HHIIHH", b[i+8:i+24])
        if t == b"data":
            raw = b[i+8:i+8+n]
            if fmt[0] == 3: return np.frombuffer(raw[:n//8*8], "<f4").reshape(-1, fmt[1]).astype(float), fmt[2]
            k = fmt[5]//8; a = np.frombuffer(raw[:n//(k*fmt[1])*k*fmt[1]], np.uint8).reshape(-1, k)
            v = (a[:, 0].astype(np.int32) | a[:, 1].astype(np.int32) << 8 | a[:, 2].astype(np.int32) << 16); v = np.where(v >= 1 << 23, v - (1 << 24), v)
            return (v / 2**23).reshape(-1, fmt[1]), fmt[2]
        i += 8 + n
EXPECT = float(sys.argv[3]) if len(sys.argv) > 3 else 0.0
rec, sr = rd(sys.argv[1]); src, sr2 = rd(sys.argv[2] if len(sys.argv) > 2 else "/tmp/noise.wav")
assert sr == 48000 and sr2 == 48000, f"sample rate {sr}/{sr2} (protocol needs 48000)"
s = src[:, 0]; on = int(np.argmax(np.abs(rec).max(1) > 1e-4)); ok = True
for name, t0, gate in (("vel127", 0, True), ("vel100", 6, False)):
    a = on + int(t0*sr) if t0 == 0 else int(np.argmax(np.abs(rec[int((t0-0.5)*sr + on):]).max(1) > 1e-4)) + int((t0-0.5)*sr + on)
    seg = slice(a + 24000, a + 24000 + 48000)  # 0.5-1.5 s into the note
    best = (1e9, 0, None)
    for lag in range(-2000, 2001):
        ref = s[24000 + lag + 0:24000 + lag + 48000]
        if len(ref) < 48000: continue
        x = rec[seg, 0]; g = (x @ ref) / (ref @ ref); r = ((x - g*ref)**2).sum()
        if r < best[0]: best = (r, lag, g)
    lag = best[1]; ref = s[24000 + lag:24000 + lag + 48000]; out = []
    for ch in (0, 1):
        x = rec[seg, ch]; g = (x @ ref) / (ref @ ref); res = 10*np.log10(((x - g*ref)**2).sum() / (x @ x) + 1e-30)
        out.append((20*np.log10(abs(g) + 1e-12), res))
    print(f"{name}: lag {lag} samples  gain L {out[0][0]:+.3f} dB  R {out[1][0]:+.3f} dB  residual L {out[0][1]:.1f} R {out[1][1]:.1f} dB")
    if gate and (abs(out[0][0] - EXPECT) > 0.1 or abs(out[1][0] - EXPECT) > 0.1): ok = False
print(f"CALIBRATION {'PASS' if ok else 'FAIL'} (expected {EXPECT:+.4f} dB, tolerance 0.1)")
sys.exit(0 if ok else 1)
