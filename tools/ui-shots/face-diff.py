#!/usr/bin/env python3
"""Compare aligned, equal-size script-face crops; optional mask excludes occlusion."""
import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image


def lab(rgb):
    """sRGB to CIE Lab, D65 white point."""
    rgb = rgb.astype(np.float32) / 255
    linear = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
    xyz = linear @ np.array([
        [0.4124564, 0.2126729, 0.0193339],
        [0.3575761, 0.7151522, 0.1191920],
        [0.1804375, 0.0721750, 0.9503041],
    ], np.float32)
    xyz /= np.array([0.95047, 1, 1.08883], np.float32)
    t = 6 / 29
    xyz = np.where(xyz > t**3, np.cbrt(xyz), xyz / (3 * t * t) + 4 / 29)
    return np.stack([116 * xyz[:, :, 1] - 16,
                     500 * (xyz[:, :, 0] - xyz[:, :, 1]),
                     200 * (xyz[:, :, 1] - xyz[:, :, 2])], axis=2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference', type=Path)
    parser.add_argument('actual', type=Path)
    parser.add_argument('--mask', type=Path, help='nonzero pixels are compared')
    parser.add_argument('--visible-height', type=int, help='exclude occluded bottom rows')
    parser.add_argument('--threshold', type=float, default=10, help='ΔE76 threshold')
    parser.add_argument('--tile', type=int, default=100, help='region size in pixels')
    args = parser.parse_args()
    reference = np.asarray(Image.open(args.reference).convert('RGB'))
    actual = np.asarray(Image.open(args.actual).convert('RGB'))
    if reference.shape != actual.shape:
        parser.error('reference and actual must have the same dimensions')
    height, width = reference.shape[:2]
    mask = np.ones((height, width), dtype=bool)
    if args.mask:
        mask_image = np.asarray(Image.open(args.mask).convert('L'))
        if mask_image.shape != mask.shape:
            parser.error('mask must have the same dimensions as the images')
        mask &= mask_image != 0
    if args.visible_height is not None:
        if not 0 < args.visible_height <= height:
            parser.error('visible-height must be inside the image')
        mask[args.visible_height:] = False
    if not mask.any() or args.tile <= 0:
        parser.error('mask must include pixels and tile must be positive')
    changed = (np.linalg.norm(lab(reference) - lab(actual), axis=2) > args.threshold) & mask
    regions = []
    for y in range(0, height, args.tile):
        for x in range(0, width, args.tile):
            y2, x2 = min(y + args.tile, height), min(x + args.tile, width)
            included = int(mask[y:y2, x:x2].sum())
            if included:
                count = int(changed[y:y2, x:x2].sum())
                regions.append({'rect': [x, y, x2, y2], 'pixels': count,
                                'percent': round(count * 100 / included, 2)})
    regions.sort(key=lambda region: region['pixels'], reverse=True)
    print(json.dumps({'size': [width, height], 'compared_pixels': int(mask.sum()),
                      'delta_e76_over_threshold_percent': round(float(changed.sum() * 100 / mask.sum()), 2),
                      'threshold': args.threshold, 'top_regions': regions[:3]}, indent=2))


if __name__ == '__main__':
    main()
