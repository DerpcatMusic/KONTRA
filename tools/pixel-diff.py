#!/usr/bin/env python3
"""Compare a rendered page against a reference capture and say *where* it differs.

    tools/pixel-diff.py REF.png GOT.png                 report regions, exit 1 if over threshold
    tools/pixel-diff.py REF.png GOT.png --out DIR       also write diff.png / mask.png
    tools/pixel-diff.py REF.png GOT.png --tol 8 --max-pct 0.4
    tools/pixel-diff.py REF.png GOT.png --crop 444,12,177,69

`--crop` and `--scale` are alternatives, not stages: `--scale` resizes the whole
render onto the whole reference, `--crop` compares one region of each, 1:1 in page
pixels. Passing both asks for a comparison neither flag means -- the render
resized and then cut, which compares the crop of a rescaled image against the
crop of an unscaled one. That combination is refused (exit 2) rather than
silently decided in favour of one of them, because the report would then name a
comparison the caller never asked for.

`--crop X,Y,W,H` compares one region of both images instead of all of it, which
is how a single control is held to a reference taken from Kontakt itself while
the rest of the page is still allowed its own rasterisation: a 177x69 panel is
6,000 px of signal where the whole page is 316,500 px of mostly-plate.
Region coordinates are then reported in the image's own coordinates, not the
crop's, so a finding can be named against the control it belongs to.

`--scale` and `--crop` cannot be combined. `--scale` resizes the whole render to
the reference; `--crop` compares one region 1:1 in page pixels, which is the
whole point of it. Accepting both would compare a region of a resized page
against a region of the original and report a number that answers neither
question -- a wrong comparison, quietly. The combination is refused (exit 2)
rather than given a precedence nobody can see.

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


def describe(ref, got, area, x, y, w, h, origin=(0, 0)):
    r = ref[y : y + h, x : x + w].reshape(-1, 3).mean(axis=0)
    g = got[y : y + h, x : x + w].reshape(-1, 3).mean(axis=0)
    return (
        f"  {area:7d} px  {w:4d}x{h:<4d} at ({x + origin[0]:3d},{y + origin[1]:3d})"
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
                    help="resize the render to the reference when sizes differ "
                         "(not with --crop: that compares a region 1:1)")
    ap.add_argument("--merge", type=int, default=3,
                    help="grow differences by this many px before grouping (default 3)")
    ap.add_argument("--max-regions", type=int, default=12)
    ap.add_argument("--crop", metavar="X,Y,W,H",
                    help="compare only this region of both images (page coordinates); "
                         "1:1, so it does not combine with --scale")
    ap.add_argument("--out", help="write diff.png and mask.png here")
    args = ap.parse_args()

    # Both at once used to be silently resolved in --scale's favour: the render
    # was resized to the whole reference, and then a region of it was compared
    # against a region of the unresized original. A caller that asked for a
    # region got a number about something else, and nothing said so. A wrong
    # comparison has to be loud, so the combination is a usage error.
    if args.scale and args.crop:
        ap.error("--scale and --crop cannot be combined: --scale resizes the whole render to "
                 "the reference, while --crop compares one region 1:1 in page pixels. Pick one "
                 "-- resize both pages to the same size, or crop a region of each")

    ref_path, got_path = Path(args.ref), Path(args.got)
    for p in (ref_path, got_path):
        if not p.exists():
            print(f"pixel-diff: no such file: {p}", file=sys.stderr)
            return 2

    try:
        ref = load(ref_path)
        # --crop replaces --scale's job: the render is cut to the region, not
        # resized to the reference, so the two are compared 1:1 in page pixels.
        if args.scale:
            got = load(got_path, scale_to=(ref.shape[1], ref.shape[0]))
        else:
            got = load(got_path)
    except Unreadable as e:
        print(f"pixel-diff: {e}", file=sys.stderr)
        return 2

    origin = (0, 0)
    ref_size, got_size = (ref.shape[1], ref.shape[0]), (got.shape[1], got.shape[0])
    if args.crop:
        try:
            cx, cy, cw, ch = (int(v) for v in args.crop.split(","))
        except ValueError:
            print("pixel-diff: --crop wants X,Y,W,H in pixels", file=sys.stderr)
            return 2

        def cut(name, im):
            if cw < 1 or ch < 1 or cx < 0 or cy < 0 or \
                    cx + cw > im.shape[1] or cy + ch > im.shape[0]:
                print(f"pixel-diff: crop {args.crop} does not fit the {name} "
                      f"({im.shape[1]}x{im.shape[0]})", file=sys.stderr)
                return None
            return im[cy:cy + ch, cx:cx + cw]

        # The render is cut to the region. The reference is cut too, unless it
        # is already exactly the region's size -- a reference cut from Kontakt
        # to the panel *is* that region, and cutting it again would be an
        # out-of-bounds error rather than a comparison.
        if (got.shape[1], got.shape[0]) != (cw, ch):
            got = cut("render", got)
            if got is None:
                return 2
        if (ref.shape[1], ref.shape[0]) != (cw, ch):
            ref = cut("reference", ref)
            if ref is None:
                return 2
        origin = (cx, cy)

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

    print(f"reference {ref_path.name}: {ref_size[0]}x{ref_size[1]}")
    print(f"render    {got_path.name}: {got_size[0]}x{got_size[1]}")
    if args.crop:
        print(f"region    {args.crop} — {ref.shape[1]}x{ref.shape[0]} of it, "
              f"at ({origin[0]},{origin[1]}) in the page")
    print(f"differing {differing} of {total} px ({pct:.3f}%), tolerance {args.tol}/channel, "
          f"ceiling {args.max_pct}%")

    if differing:
        found = regions(merge(mask, args.merge), args.max_regions)
        print(f"\n{len(found)} region(s), largest first:")
        for area, x, y, w, h in found:
            print(describe(ref, got, area, x, y, w, h, origin))

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
