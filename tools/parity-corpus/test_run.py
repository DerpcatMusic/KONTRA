#!/usr/bin/env python3
"""Self-tests for the parity runner: the alignment and the per-case containment.

    python3 tools/parity-corpus/test_run.py

No library, no Kontakt, no render binary: every case is a synthetic WAV or a
stub executable. Each test is a defect the runner had, reduced to the smallest
fixture that shows it.
"""
import contextlib
import io
import json
import os
import stat
import struct
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import run  # noqa: E402


def mono(samples):
    """One channel, in the shape `read_wav` returns."""
    return [list(samples)]


def burst(n_zeros=4800, n_sound=9600):
    """Silence, then a decaying tone: an onset the aligner can find."""
    out = [0.0] * n_zeros
    for i in range(n_sound):
        out.append(0.5 * (0.999 ** i) * ((i % 48) / 48.0 - 0.5))
    return out


def wav(channels, rate=48000, bits=32, tag=3, body=None):
    """A minimal RIFF/WAVE file, float32 or PCM16, as the runner reads them."""
    if body is None:
        flat = [x for frame in zip(*channels) for x in frame]
        if tag == 3:
            body = struct.pack("<%df" % len(flat), *flat)
        else:
            body = struct.pack("<%dh" % len(flat), *(int(round(x * 32767)) for x in flat))
    fmt = struct.pack("<HHIIHH", tag, len(channels), rate, rate * len(channels) * bits // 8,
                      len(channels) * bits // 8, bits)
    chunk = b"fmt " + struct.pack("<I", len(fmt)) + fmt
    chunk += b"data" + struct.pack("<I", len(body)) + body
    return b"RIFF" + struct.pack("<I", 4 + len(chunk)) + b"WAVE" + chunk


class Wav(unittest.TestCase):
    """An unreadable WAV is a ValueError, not the run ending."""

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="parity-wav-"))
        self.addCleanup(lambda: [p.unlink() for p in self.dir.iterdir()] and self.dir.rmdir())

    def write(self, name, data):
        p = self.dir / name
        p.write_bytes(data)
        return p

    def test_a_good_file_reads(self):
        p = self.write("ok.wav", wav([[0.0, 0.5, -0.5]]))
        chans, rate = run.read_wav(p)
        self.assertEqual(rate, run.RATE)
        self.assertEqual(len(chans), 1)
        self.assertAlmostEqual(chans[0][1], 0.5, places=6)

    def test_a_bad_header_raises_value_error(self):
        p = self.write("bad.wav", b"not a wav at all")
        # It was `SystemExit(str)`: status 1 with no table, and `--json` never
        # written -- one odd WAV looked exactly like drift and ended the run.
        with self.assertRaises(ValueError):
            run.read_wav(p)

    def test_a_missing_data_chunk_raises_value_error(self):
        data = wav([[0.0, 0.1]])
        p = self.write("nohunk.wav", data.replace(b"data", b"junk"))
        with self.assertRaises(ValueError):
            run.read_wav(p)

    def test_an_unsupported_format_raises_value_error(self):
        p = self.write("eight.wav", wav([[0.0, 0.1]], bits=8, tag=1,
                                        body=bytes([0, 128, 255, 128])))
        with self.assertRaises(ValueError):
            run.read_wav(p)

    def test_the_wrong_rate_raises_value_error(self):
        p = self.write("rate.wav", wav([[0.0, 0.1]], rate=44100))
        with self.assertRaises(ValueError):
            run.read_wav(p)


class Align(unittest.TestCase):
    """The found lag is applied to the side that starts later."""

    def test_a_negative_lag_drops_kontras_head_not_the_references_tail(self):
        ref = burst()
        later = [0.0] * 240 + ref          # KONTRA starts 5 ms later
        a, b, lag, _, _ = run.align_pair(mono(ref), mono(later))
        self.assertLess(lag, 0, "KONTRA starting later is a negative lag")
        self.assertEqual(len(a[0]), len(b[0]), "both sides lose the same head")
        for i, (x, y) in enumerate(zip(a[0], b[0])):
            self.assertAlmostEqual(x, y, places=9,
                                   msg=f"still misaligned at sample {i}")

    def test_a_positive_lag_still_drops_the_references_head(self):
        ref = [0.0] * 240 + burst()        # the reference starts 5 ms later
        kern = burst()
        a, b, lag, _, _ = run.align_pair(mono(ref), mono(kern))
        self.assertGreater(lag, 0, "the reference starting later is a positive lag")
        for i, (x, y) in enumerate(zip(a[0], b[0])):
            self.assertAlmostEqual(x, y, places=9,
                                   msg=f"still misaligned at sample {i}")

    def test_shift_drops_a_head_and_never_a_tail(self):
        # The old `shift` read a negative count as `c[:n]`, i.e. it dropped the
        # tail, which moves neither signal: it is not an alignment at all.
        self.assertEqual(run.shift(mono([1.0, 2.0, 3.0]), -1), mono([1.0, 2.0, 3.0]))
        self.assertEqual(run.shift(mono([1.0, 2.0, 3.0]), 1), mono([2.0, 3.0]))

    def test_no_lag_changes_nothing(self):
        ref = mono(burst())
        a, b, lag, _, _ = run.align_pair(ref, ref)
        self.assertEqual(lag, 0)
        self.assertIs(a[0], ref[0], "no copy is made when there is nothing to align")


class Containment(unittest.TestCase):
    """One case that cannot be measured is a failed row, not the end of the run."""

    def setUp(self):
        self.work = Path(tempfile.mkdtemp(prefix="parity-case-"))
        self.addCleanup(lambda: [p.unlink() for p in self.work.iterdir()]
                        and self.work.rmdir())

    def stub(self, name, text):
        p = self.work / name
        p.write_text("#!/bin/sh\n" + text + "\n")
        p.chmod(p.stat().st_mode | stat.S_IXUSR)
        return str(p)

    def case(self):
        return {"id": "glory-trumpet1-multi", "name": "stub", "expect": "matches",
                "instrument": "x.nki", "threshold": {"level_db": 0.5, "band_rms_db": 1.0},
                "baseline": {"level_db": 0.0, "band_rms_db": 0.0, "align_ms": 0.0},
                "references": ["glory-trumpet1-multi-a.wav"],
                "midi": {"notes": [{"key": 60, "length_s": 0.1}]}}

    def run_with(self, render):
        saved = os.environ.get("KONTRA_RENDER")
        os.environ["KONTRA_RENDER"] = render
        try:
            return run.run_case(self.case(), self.work, Path("/tmp"))
        finally:
            if saved is None:
                del os.environ["KONTRA_RENDER"]
            else:
                os.environ["KONTRA_RENDER"] = saved

    def test_a_render_that_exits_zero_without_a_wav_is_a_failed_row(self):
        # The runner renders "successfully" and writes no WAV: before, `read_wav`
        # raised SystemExit(str) out of run_case and ended every case after this.
        row = self.run_with(self.stub("norender", "exit 0"))
        self.assertFalse(row["ok"])
        self.assertEqual(row["status"], "could not compare")

    def test_a_render_that_never_finishes_is_a_failed_row(self):
        saved = run.RENDER_TIMEOUT
        run.RENDER_TIMEOUT = 0.2
        try:
            row = self.run_with(self.stub("slow", "sleep 5"))
        finally:
            run.RENDER_TIMEOUT = saved
        self.assertFalse(row["ok"])
        self.assertEqual(row["status"], "KONTRA failed to render")
        self.assertIn("did not finish", row["detail"])

    def test_a_rendered_wav_in_the_wrong_format_is_a_failed_row(self):
        # A real, readable reference and a 16-bit/44.1 kHz "render": the runner
        # has to say it cannot compare, not abort the corpus.
        kern = self.work / "wrong.wav"
        kern.write_bytes(wav([[0.0, 0.1]], rate=44100, bits=16, tag=1))
        row = self.run_with(self.stub("copy", f'cp "{kern}" "$4"'))
        self.assertFalse(row["ok"])
        self.assertEqual(row["status"], "could not compare")
        self.assertIn("44100", row["detail"])


class Exit(unittest.TestCase):
    """The documented statuses: 1 for a failed row, 2 only for a corpus problem."""

    def test_a_case_that_cannot_be_compared_exits_1_and_still_writes_the_summary(self):
        work = Path(tempfile.mkdtemp(prefix="parity-exit-"))
        self.addCleanup(lambda: [p.unlink() for p in work.iterdir()] and work.rmdir())
        kern = work / "wrong.wav"
        kern.write_bytes(wav([[0.0, 0.1]], rate=44100, bits=16, tag=1))
        stub = work / "render"
        stub.write_text("#!/bin/sh\ncp \"%s\" \"$4\"\n" % kern)
        stub.chmod(stub.stat().st_mode | stat.S_IXUSR)
        out = work / "parity.json"
        saved = os.environ.get("KONTRA_RENDER")
        os.environ["KONTRA_RENDER"] = str(stub)
        try:
            # The row it prints is the point of the run, not of this test.
            with contextlib.redirect_stdout(io.StringIO()):
                status = run.main(["--case", "glory-trumpet1-multi", "--json", str(out)])
        finally:
            if saved is None:
                del os.environ["KONTRA_RENDER"]
            else:
                os.environ["KONTRA_RENDER"] = saved
        self.assertEqual(status, 1, "a failed row is drift's status, not exit 2")
        rows = json.loads(out.read_text())["cases"]
        self.assertEqual([r["id"] for r in rows], ["glory-trumpet1-multi"])
        self.assertEqual(rows[0]["status"], "could not compare")


if __name__ == "__main__":
    unittest.main(verbosity=2)
