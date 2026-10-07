#!/usr/bin/env python3
"""Write the reference noise sample: make_noise.py OUT.wav  (20 s, 48 kHz mono, 24-bit, white noise, seed 1, peak -6 dBFS).
Copy it to D:\\kontra_ref_src\\noise.wav (/mnt/MAIN_STORAGE/kontra_ref_src) and run noise_instrument.sh with Kontakt up in classic view
(Kontakt cannot start with a WAV argument; a WAV double-clicked in the Files tab loads as a one-zone instrument spanning the keyboard).
Keys below 60 pitch the sample down and last longer than 20 s: space notes >= 21 s only for keys >= 60, or expect tails."""
import sys, wave, numpy as np
r = np.random.default_rng(1); x = r.standard_normal(48000 * 20); x *= 0.5 / np.abs(x).max()
pcm = (x * (2**23 - 1)).astype("<i4"); b = pcm.tobytes(); out = bytearray()
for i in range(0, len(b), 4): out += b[i:i+3]
w = wave.open(sys.argv[1], "wb"); w.setnchannels(1); w.setsampwidth(3); w.setframerate(48000); w.writeframes(bytes(out)); w.close()
