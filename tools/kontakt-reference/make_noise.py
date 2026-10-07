#!/usr/bin/env python3
"""Reference test samples: make_noise.py OUT.wav [noise|stereo|burst]   (48 kHz, 24-bit, seed 1)
  noise   mono, 20 s white noise, peak -6 dBFS (default)
  stereo  2 ch, 20 s, left and right independent white noise A and B (regress outputs on A,B = the effect's stereo matrix)
  burst   mono, 15 s: 50 ms white noise burst at t = 0.1 s, then silence (reverb tail / pre-delay tests)
Copy to D:\\kontra_ref_src\\NAME.wav (/mnt/MAIN_STORAGE/kontra_ref_src) and load with noise_instrument.sh NAME with Kontakt in classic view
(Kontakt cannot start with a WAV argument; a WAV double-clicked in the Files tab loads as a one-zone instrument spanning the keyboard).
Play key 60 for original speed. The sample is cut at note-off + release, so hold the note for the length you need."""
import sys, wave, numpy as np
kind = sys.argv[2] if len(sys.argv) > 2 else "noise"; r = np.random.default_rng(1)
def nz(n): x = r.standard_normal(n); return x * 0.5 / np.abs(x).max()
if kind == "noise": ch = [nz(48000 * 20)]
elif kind == "stereo": ch = [nz(48000 * 20), nz(48000 * 20)]
elif kind == "burst":
    x = np.zeros(48000 * 15); x[4800:4800 + 2400] = nz(2400); ch = [x]
else: sys.exit(__doc__)
pcm = (np.stack(ch, 1) * (2**23 - 1)).astype("<i4"); b = pcm.tobytes(); out = bytearray()
for i in range(0, len(b), 4): out += b[i:i+3]
w = wave.open(sys.argv[1], "wb"); w.setnchannels(len(ch)); w.setsampwidth(3); w.setframerate(48000); w.writeframes(bytes(out)); w.close()
