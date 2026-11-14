#!/usr/bin/env python3
"""drive.py PAGE.drive SHOT.png -- replays a probe page's interactions with xdotool on $DISPLAY.

The page is at $ORIGIN (x,y,scale) when set, else found in SHOT.png by its background (the stock cream, or the pictures page's
wallpaper): its bottom rows give the page's left, right and so its scale, and the page's known
height its top. Each line of PAGE.drive is one action in page pixels:
  click X Y [MOD]   dclick X Y   drag X Y DX DY [MOD]   key NAME   type TEXT
MOD is shift, alt or ctrl, held through the action.
"""
import os
import subprocess
import sys
import time

import numpy as np
from PIL import Image

WIDTH, HEIGHT = 900, 380
# The stock cream, the text page's set_ui_color and the pictures page's wallpaper.
BACKGROUNDS = [(240, 239, 228), (255, 51, 34), (30, 30, 36)]
MODS = {"shift": "shift", "alt": "alt", "ctrl": "ctrl"}


def locate(shot):
    # A host whose page sits at a known place says so: ORIGIN=x,y,scale. Kontakt draws a
    # wallpaper from the top of the instrument header, so its edge is not the page's.
    if os.environ.get("ORIGIN"):
        x, y, s = map(float, os.environ["ORIGIN"].split(","))
        return x, y, s
    a = np.asarray(Image.open(shot).convert("RGB")).astype(int)
    for bg in BACKGROUNDS:
        mask = np.abs(a - np.array(bg)).sum(2) < 8
        rows = np.nonzero(mask.sum(1) >= 200)[0]
        if len(rows) == 0:
            continue
        bottom = rows.max()
        xs = np.nonzero(mask[bottom - 2])[0]
        scale = (xs.max() - xs.min() + 1) / WIDTH
        return xs.min(), bottom - HEIGHT * scale + 1, scale
    sys.exit(f"{shot}: no probe page found")


def xdo(*args):
    subprocess.run(["xdotool", *map(str, args)], check=False)


def main():
    ox, oy, s = locate(sys.argv[2])
    print(f"page at {ox:.0f},{oy:.0f} scale {s:.3f}", file=sys.stderr)

    def at(x, y):
        return round(ox + float(x) * s), round(oy + float(y) * s)

    for line in open(sys.argv[1]):
        w = line.split()
        if not w:
            continue
        op, rest = w[0], w[1:]
        mod = MODS.get(rest[-1]) if rest else None
        if mod:
            rest = rest[:-1]
            xdo("keydown", mod)
        if op in ("click", "dclick"):
            x, y = at(*rest[:2])
            xdo("mousemove", x, y)
            time.sleep(0.2)
            xdo("click", *(["--repeat", 2, "--delay", 90] if op == "dclick" else []), 1)
        elif op == "drag":
            x, y = at(*rest[:2])
            dx, dy = float(rest[2]) * s, float(rest[3]) * s
            xdo("mousemove", x, y)
            time.sleep(0.2)
            xdo("mousedown", 1)
            for k in range(1, 9):
                xdo("mousemove", round(x + dx * k / 8), round(y + dy * k / 8))
                time.sleep(0.05)
            xdo("mouseup", 1)
        elif op == "key":
            xdo("key", rest[0])
        elif op == "type":
            xdo("type", "--delay", 80, " ".join(rest))
        if mod:
            xdo("keyup", mod)
        time.sleep(0.6)


if __name__ == "__main__":
    main()
