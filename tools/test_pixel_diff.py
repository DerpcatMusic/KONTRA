#!/usr/bin/env python3
"""Checks for tools/pixel-diff.py. Run: python3 tools/test_pixel_diff.py

The tool is the instrument behind both pixel rows in `tools/verify.sh`, so a run
that answers a different question than the caller asked makes every pixel number
in this repository suspect. The case here is the pair of flags that used to be
resolved silently: `--scale` resizes the whole render to the reference, `--crop`
compares one region 1:1 in page pixels, and passing both compared a region of a
resized page against a region of the original. Nothing said so; the caller got a
number about something they had not asked about.

Run as a subprocess, not imported: `pixel-diff` is a script with an argparse
main, and the exit status is half of what is being checked.
"""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
TOOL = HERE / "pixel-diff.py"


def deps() -> bool:
    try:
        import numpy  # noqa: F401
        import PIL  # noqa: F401
    except ImportError:
        return False
    return True


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, str(TOOL), *args],
                          capture_output=True, text=True, check=False)


class Images:
    """Two PNGs, a solid-colour page and a half-size copy of it."""

    def __init__(self, directory: Path) -> None:
        from PIL import Image
        self.page = directory / "page.png"
        self.half = directory / "half.png"
        self.other = directory / "other.png"
        Image.new("RGB", (40, 20), (10, 20, 30)).save(self.page)
        Image.new("RGB", (20, 10), (10, 20, 30)).save(self.half)
        Image.new("RGB", (40, 20), (200, 40, 40)).save(self.other)


@unittest.skipUnless(deps(), "pixel-diff needs numpy and Pillow, and a run without them "
                             "would exit 2 for the wrong reason")
class Flags(unittest.TestCase):
    """The flags mean what they say, or the run refuses to pretend they do."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.images = Images(Path(self.tmp.name))

    def test_scale_alone_compares_a_half_size_page(self) -> None:
        r = run(str(self.images.page), str(self.images.half), "--scale")
        self.assertEqual(r.returncode, 0, r.stderr + r.stdout)
        self.assertIn("PIXEL DIFF PASSED", r.stdout)

    def test_crop_alone_compares_the_region_it_was_given(self) -> None:
        r = run(str(self.images.page), str(self.images.other), "--crop", "10,5,4,3")
        self.assertEqual(r.returncode, 1, r.stderr + r.stdout)  # the colours differ
        self.assertIn("region    10,5,4,3", r.stdout)
        self.assertIn("12 of 12 px", r.stdout)

    def test_scale_with_crop_is_refused(self) -> None:
        """The old behaviour compared something else, quietly. It must not run at all."""
        r = run(str(self.images.page), str(self.images.other), "--scale", "--crop", "10,5,4,3")
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertNotIn("differing", r.stdout, "a refused combination must compare nothing")
        self.assertNotIn("PIXEL DIFF", r.stdout)
        self.assertIn("--scale and --crop cannot be combined", r.stderr)

    def test_the_refusal_names_both_flags(self) -> None:
        r = run(str(self.images.page), str(self.images.page), "--crop", "0,0,4,4", "--scale")
        for flag in ("--scale", "--crop"):
            self.assertIn(flag, r.stderr, f"the message must name {flag}")


if __name__ == "__main__":
    unittest.main(verbosity=2)
