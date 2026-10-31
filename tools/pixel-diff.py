#!/usr/bin/env python3
"""Compare a rendered page against a reference capture and say *where* it differs.

    tools/pixel-diff.py REF.png GOT.png                 report regions, exit 1 if over threshold
    tools/pixel-diff.py REF.png GOT.png --out DIR       also write diff.png / mask.png
    tools/pixel-diff.py REF.png GOT.png --tol 8 --max-pct 0.4

A geometry harness answers "is the control where it is drawn". It cannot see a
picture that resolved to nothing, a face that never painted, or a panel drawn in
the wrong colour -- all of those still have a rect. This answers the other
question: "does the page look like Kontakt's".

Regions are reported as bounding boxes over connected runs of differing pixels,
largest first, so a missing graphic arrives as "180x120 at (40,120), 21,600 px"
rather than "4% of the image differs". Nearby differences are merged (a dashed
outline should be one finding, not forty), and each region reports the mean
colour on both sides, which is usually enough to name the defect: a region where
the reference is photographic and the render is flat black is a missing image.

Exit status: 0 within tolerance, 1 over, 2 the two images cannot be compared
(different sizes without --scale, unreadable file).
"""
import argparse
import sys
from collections import deque
from pathlib import Path

try:
    import numpy as np
    from PIL import Image
except ImportError as e:  # pragma: no cover - environment problem, not a defect
    print(f"pixel-diff: needs numpy and Pillow ({e})", file=sys.stderr)
    sys.exit(2)


class Unreadable(Exception):
    """The images cannot be compared at all -- exit status 2, not "they differ" (1)."""


def load(path, scale_to=None):
    try:
        im = Image.open(path)
    except OSError as e:
        raise Unreadable(f"cannot open {path}: {e}") from e
    try:
        return as_array(im, scale_to)
    except (OSError, ValueError) as e:
        raise Unreadable(f"cannot read {path}: {e}") from e


def as_array(im, scale_to):
    """`im` as an int16 RGB array, flattened over black and optionally rescaled."""
    im.load()
    if im.mode != "RGB":
        # Flatten alpha over black: a page is drawn opaque, so a transparent
        # reference pixel means "nothing there", which reads as black in a host.
        if im.mode in ("RGBA", "LA", "P"):
            im = im.convert("RGBA")
            flat = Image.new("RGB", im.size, (0, 0, 0))
            flat.paste(im, mask=im.split()[-1])
            im = flat
        else:
            im = im.convert("RGB")
    if scale_to is not None and im.size != scale_to:
        im = im.resize(scale_to, Image.LANCZOS)
    return np.asarray(im, dtype=np.int16)


def merge(mask, radius):
    """Grow the mask by `radius` px so one visual defect is one region.

    Done with a separable max filter: a difference in a 1 px border and the
    difference inside the control it belongs to should not be two findings.
    """
    if radius <= 0:
        return mask
    m = mask.astype(np.uint8)
    k = 2 * radius + 1
    # horizontal then vertical running max, cheap enough at page sizes
    pad = np.pad(m, ((0, 0), (radius, radius)), mode="constant")
    h = np.zeros_like(m)
    for d in range(k):
        h = np.maximum(h, pad[:, d : d + m.shape[1]])
    pad = np.pad(h, ((radius, radius), (0, 0)), mode="constant")
    v = np.zeros_like(m)
    for d in range(k):
        v = np.maximum(v, pad[d : d + m.shape[0], :])
    return v.astype(bool)


def regions(mask, limit):
    """Connected components of `mask` as (area, x, y, w, h), largest first."""
    seen = np.zeros_like(mask, dtype=bool)
    h, w = mask.shape
    found = []
    for y0 in range(h):
        row = mask[y0]
        for x0 in np.flatnonzero(row & ~seen[y0]):
            x0 = int(x0)
            if seen[y0, x0]:
                continue
            q = deque([(x0, y0)])
            seen[y0, x0] = True
            minx = maxx = x0
            miny = maxy = y0
            area = 0
            while q:
                x, y = q.popleft()
                area += 1
                if x < minx: minx = x
                if x > maxx: maxx = x
                if y < miny: miny = y
                if y > maxy: maxy = y
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < w and 0 <= ny < h and mask[ny, nx] and not seen[ny, nx]:
                        seen[ny, nx] = True
                        q.append((nx, ny))
            found.append((area, minx, miny, maxx - minx + 1, maxy - miny + 1))
    found.sort(key=lambda r: -r[0])
    return found[:limit]


def describe(ref, got, area, x, y, w, h):
    r = ref[y : y + h, x : x + w].reshape(-1, 3).mean(axis=0)
    g = got[y : y + h, x : x + w].reshape(-1, 3).mean(axis=0)
    return (
        f"  {area:7d} px  {w:4d}x{h:<4d} at ({x:3d},{y:3d})"
        f"   ref rgb({r[0]:3.0f},{r[1]:3.0f},{r[2]:3.0f})"
        f"   got rgb({g[0]:3.0f},{g[1]:3.0f},{g[2]:3.0f})"
    )


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("ref")
    ap.add_argument("got")
    ap.add_argument("--tol", type=int, default=12,
                    help="per-channel difference that counts as a difference (default 12)")
    ap.add_argument("--max-pct", type=float, default=0.5,
                    help="percent of the page allowed to differ (default 0.5)")
    ap.add_argument("--scale", action="store_true",
                    help="resize the render to the reference when sizes differ")
    ap.add_argument("--merge", type=int, default=3,
                    help="grow differences by this many px before grouping (default 3)")
    ap.add_argument("--max-regions", type=int, default=12)
    ap.add_argument("--out", help="write diff.png and mask.png here")
    args = ap.parse_args()

    ref_path, got_path = Path(args.ref), Path(args.got)
    for p in (ref_path, got_path):
        if not p.exists():
            print(f"pixel-diff: no such file: {p}", file=sys.stderr)
            return 2

    try:
        ref = load(ref_path)
        if args.scale:
            got = load(got_path, scale_to=(ref.shape[1], ref.shape[0]))
        else:
            got = load(got_path)
    except Unreadable as e:
        print(f"pixel-diff: {e}", file=sys.stderr)
        return 2
    if got.shape != ref.shape:
        print(f"pixel-diff: sizes differ — reference {ref.shape[1]}x{ref.shape[0]}, "
              f"render {got.shape[1]}x{got.shape[0]}. Pass --scale to compare anyway.",
              file=sys.stderr)
        return 2

    delta = np.abs(ref - got).max(axis=2)
    mask = delta > args.tol
    differing = int(mask.sum())
    total = mask.size
    pct = 100.0 * differing / total

    print(f"reference {ref_path.name}: {ref.shape[1]}x{ref.shape[0]}")
    print(f"render    {got_path.name}: {got.shape[1]}x{got.shape[0]}")
    print(f"differing {differing} of {total} px ({pct:.3f}%), tolerance {args.tol}/channel, "
          f"ceiling {args.max_pct}%")

    if differing:
        found = regions(merge(mask, args.merge), args.max_regions)
        print(f"\n{len(found)} region(s), largest first:")
        for area, x, y, w, h in found:
            print(describe(ref, got, area, x, y, w, h))

    if args.out:
        out = Path(args.out)
        out.mkdir(parents=True, exist_ok=True)
        # Amplified difference: grey where the pages agree, bright where they do not.
        vis = np.zeros_like(ref, dtype=np.uint8)
        vis[..., 0] = np.clip(delta * 4, 0, 255)
        vis[..., 1] = np.clip(ref.mean(axis=2) * 0.25, 0, 255)
        vis[..., 2] = np.clip(got.mean(axis=2) * 0.25, 0, 255)
        Image.fromarray(vis).save(out / "diff.png")
        Image.fromarray((mask * 255).astype(np.uint8)).save(out / "mask.png")
        print(f"\nwrote {out/'diff.png'} and {out/'mask.png'}")

    if pct > args.max_pct:
        print(f"\nPIXEL DIFF FAILED — {pct:.3f}% differs, ceiling {args.max_pct}%")
        return 1
    print(f"\nPIXEL DIFF PASSED — within {args.max_pct}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
