#!/usr/bin/env python3
"""Pinned-state check: state_check.py CLOSED.png MASTER_OPEN.png -> exit 0 when the screen regions equal the golden images.
Golden regions (1600x1000 desktop, classic view): Master Editor Volume+Tune block (0.00 dB, 440.00 Hz), instrument Tune knob + Pan
slider (tune 0.00 st, pan Center) and the instrument Volume slider: default 0.0 dB (CC7 never sent) or the CC7=127 position (CC7 moves the slider). Pixel-exact; re-make the goldens only when Kontakt's skin changes."""
import sys, os, numpy as np
from PIL import Image
d = os.path.dirname(os.path.abspath(__file__))
def same(shot, box, gold):
    a = np.asarray(Image.open(shot).convert("RGB").crop(box)).astype(int); b = np.asarray(Image.open(os.path.join(d, gold)).convert("RGB")).astype(int)
    return a.shape == b.shape and int(np.abs(a - b).max()) <= 6   # tolerance for hover highlight / antialiasing
closed, opened = sys.argv[1], sys.argv[2]
r = {"master volume 0.00 dB, tune 440.00 Hz": same(opened, (520, 125, 960, 205), "golden_master.png"),
     "instrument tune 0.00 st, pan Center": same(closed, (1170, 185, 1310, 280), "golden_inst_tune_pan.png"),
     }
vol = (1318, 250, 1450, 276)
if same(closed, vol, "golden_inst_vol_cc127.png"): r["instrument volume slider = state after CC7=127 (explicit-CC protocol; unity)"] = True
elif same(closed, vol, "golden_inst_vol.png"): r["instrument volume 0.0 dB (default, CC7 never sent: output is 0.5x = -6.02 dB)"] = True
else: r["instrument volume slider (neither the default nor the CC7=127 golden)"] = False
for k, v in r.items(): print(("PINNED  " if v else "MISMATCH ") + k)
sys.exit(0 if all(r.values()) else 1)
