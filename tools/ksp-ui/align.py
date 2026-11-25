#!/usr/bin/env python3
"""align.py OURS.png KONTAKT.png [KONTAKT-lower.png] [OUT.png] -- where KONTRA's page (ui-shots) sits
in Kontakt's shots (shoot.sh with an .nki), and which parts of it sit elsewhere.

Finds the page in each Kontakt shot (the shift with the least mean difference), then matches every
64-pixel block of the page that has detail, within 12 pixels: a block whose best match is 2 pixels
or more away from where the page puts it is printed (x, y, dx, dy, residual) and boxed in OUT.png
(red: moved, yellow: no close match anywhere, e.g. a different picture or text)."""
import sys

import numpy as np
from PIL import Image, ImageDraw

B, R = 64, 12


def grey(path):
    return np.asarray(Image.open(path).convert("L"), dtype=np.float32)


def best(a, b, xs, ys, step=1):
    """The shift (x, y) of `a` in `b` among xs, ys with the least mean difference, and that difference."""
    h, w = a.shape
    out = (None, None, 1e9)
    for y in ys[::step]:
        for x in xs[::step]:
            if y < 0 or x < 0 or y + h > b.shape[0] or x + w > b.shape[1]:
                continue
            d = np.abs(b[y:y + h, x:x + w] - a).mean()
            if d < out[2]:
                out = (x, y, d)
    return out


def place(page, shot, rows):
    """Where the page rows `rows` (a slice) sit in the shot: coarse on a 4x step, then exact."""
    a = page[rows]
    x, y, _ = best(a, shot, range(0, 40), range(-600, 400), step=4)
    if x is None:
        return None
    x, y, d = best(a, shot, range(max(0, x - 4), x + 5), range(y - 4, y + 5))
    return x, y - rows.start, d


def main():
    ours, shots = sys.argv[1], [a for a in sys.argv[2:] if not a.endswith("-diff.png")]
    out = next((a for a in sys.argv[2:] if a.endswith("-diff.png")), None)
    page = grey(ours)
    h, w = page.shape
    # The page's top strip places the first shot; a strip near the bottom places the lower one.
    where = []
    for i, s in enumerate(shots):
        shot = grey(s)
        rows = slice(0, 120) if i == 0 else slice(h - 220, h - 100)
        p = place(page, shot, rows)
        print(f"{s}: page at {p[:2]} (mean diff {p[2]:.1f})" if p else f"{s}: page not found")
        where.append((shot, p))
    img = Image.open(ours).convert("RGB")
    draw = ImageDraw.Draw(img)
    moved = 0
    for by in range(0, h - B + 1, B // 2):
        for bx in range(0, w - B + 1, B // 2):
            blk = page[by:by + B, bx:bx + B]
            if blk.std() < 12:
                continue
            # The shot that shows this block whole, the first one that does.
            for shot, p in where:
                if p is None:
                    continue
                x0, y0 = p[0] + bx, p[1] + by
                if y0 - R >= 0 and y0 + B + R <= min(shot.shape[0], 612) and x0 + B + R <= shot.shape[1]:
                    x, y, d = best(blk, shot, range(x0 - R, x0 + R + 1), range(y0 - R, y0 + R + 1))
                    dx, dy = x - x0, y - y0
                    if d > 25:
                        draw.rectangle([bx, by, bx + B, by + B], outline=(255, 220, 0))
                        print(f"block {bx},{by}: no match (residual {d:.0f})")
                    elif max(abs(dx), abs(dy)) >= 2:
                        moved += 1
                        draw.rectangle([bx, by, bx + B, by + B], outline=(255, 0, 0), width=2)
                        print(f"block {bx},{by}: Kontakt has it at {dx:+d},{dy:+d} (residual {d:.0f})")
                    break
    print(f"{moved} blocks moved")
    if out:
        img.save(out)


if __name__ == "__main__":
    main()
