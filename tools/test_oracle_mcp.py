"""Self-check for tools/oracle-mcp/server.py's pure parts (no Kontakt, no display).

    python3 tools/test_oracle_mcp.py

Covers the pieces a wrong measurement would hide behind: the MIDI writer and its own
reader, the WAV writer/reader/trim, the change metric, the display guard and the page
verdict. The rig itself is verified by tools/oracle-mcp/smoke.sh; this file must never
start Kontakt or touch :0.
"""
import importlib.util
import tempfile
import types
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "oracle_mcp_server", Path(__file__).parent / "oracle-mcp" / "server.py")
om = importlib.util.module_from_spec(spec)
spec.loader.exec_module(om)


class Midi(unittest.TestCase):
    def test_note_list_round_trips_through_our_own_reader(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "a.mid"
            meta = om.write_midi(path, [
                {"start_s": 0.1, "length_s": 0.8, "key": 60, "velocity": 100},
                {"start_s": 0.9, "length_s": 0.7, "key": 64, "velocity": 90},
                {"start_s": 1.6, "length_s": 0.4, "key": 67, "velocity": 110},
            ])
            info = om.midi_info(path)
            self.assertEqual(info["notes"], 3)
            self.assertEqual(meta["notes"], 3)
            self.assertAlmostEqual(info["duration_s"], 2.0, places=3)

    def test_a_held_note_is_its_hold(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "b.mid"
            om.write_midi(path, [{"start_s": 0.0, "length_s": 1.5, "key": 60}])
            self.assertAlmostEqual(om.midi_info(path)["duration_s"], 1.5, places=3)

    def test_the_file_carries_the_tempo_bpm_asks_for(self):
        # Without an `FF 51 03` event a reader assumes 120 bpm, so at
        # ORACLE_MCP_BPM=60 a note at 1.0 s would play at 0.5 s and the duration
        # tool_render trims the render to would be half of what was asked for.
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "tempo.mid"
            meta = om.write_midi(path, [{"start_s": 1.0, "length_s": 1.0, "key": 60}], bpm=60)
            self.assertAlmostEqual(meta["duration_s"], 2.0, places=3)
            self.assertAlmostEqual(om.midi_info(path)["duration_s"], 2.0, places=3)

    def test_bad_notes_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "c.mid"
            for bad in ([], [{"key": 200, "length_s": 1.0}], [{"key": 60, "length_s": 0}]):
                with self.assertRaises(om.ToolError):
                    om.write_midi(path, bad)

    def test_not_a_midi_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "d.mid"
            path.write_bytes(b"not a midi file")
            with self.assertRaises(om.ToolError):
                om.midi_info(path)


class Wav(unittest.TestCase):
    def test_write_read_trim(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "x.wav"
            chans = [[0.0] * 100, [0.5] * 100]
            chans[0][10] = 0.9  # one loud frame at 10 of 100
            om.write_wav_f32(src, chans, 1000)  # 1000 Hz keeps the arithmetic obvious
            rate, ch, back = om.read_wav(src)
            self.assertEqual((rate, ch), (1000, 2))
            self.assertEqual(len(back[0]), 100)
            self.assertAlmostEqual(back[0][10], 0.9, places=6)
            dst = Path(tmp) / "y.wav"
            cut = om.trim_wav(src, dst, 0.01, 0.05)  # 0.010 .. 0.060 s
            self.assertEqual(cut["start_frame"], 10)
            rate, ch, trimmed = om.read_wav(dst)
            self.assertEqual(len(trimmed[0]), 50)
            st = om.wav_stats(dst)
            self.assertAlmostEqual(st["duration_s"], 0.05, places=6)
            self.assertAlmostEqual(st["peak"], 0.9, places=5)
            self.assertAlmostEqual(st["onset_s"], 0.0, places=6)

    def test_stats_of_silence_do_not_invent_a_peak(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "z.wav"
            om.write_wav_f32(src, [[0.0] * 64, [0.0] * 64], 48000)
            st = om.wav_stats(src)
            self.assertEqual(st["peak"], 0.0)
            self.assertIsNone(st["peak_dbfs"])
            self.assertIsNone(st["onset_s"])

    def test_trim_beyond_the_end_stops_at_the_end(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "e.wav"
            om.write_wav_f32(src, [[0.1] * 20, [0.1] * 20], 1000)
            dst = Path(tmp) / "f.wav"
            with self.assertRaises(om.ToolError):
                om.trim_wav(src, dst, 0.5, 10.0)  # the window starts past the end
            cut = om.trim_wav(src, dst, 0.01, 10.0)  # the window runs past the end
            self.assertEqual(cut["frames"], 10)


class ChangeMetric(unittest.TestCase):
    def test_identical_images_change_nothing(self):
        import subprocess
        with tempfile.TemporaryDirectory() as tmp:
            img = Path(tmp) / "a.png"
            subprocess.run(["magick", "-size", "64x64", "xc:gray30", str(img)], check=True)
            other = Path(tmp) / "b.png"
            subprocess.run(["magick", "-size", "64x64", "xc:gray30", str(other)], check=True)
            self.assertEqual(om.changed_pixels(img, other), 0)
            subprocess.run(["magick", "-size", "64x64", "xc:white", str(other)], check=True)
            self.assertGreater(om.changed_pixels(img, other), 100)


class Safety(unittest.TestCase):
    def test_display_guard_refuses_the_users_desktop(self):
        for bad in (":0", ":0.0", ":1", ":10", ":77.0", ""):
            with self.assertRaises(om.ToolError):
                om.guard_display(bad)

    def test_display_guard_allows_only_ours(self):
        for good in om.ALLOWED_DISPLAYS:
            om.guard_display(good)
        self.assertNotIn(":0", om.ALLOWED_DISPLAYS)

    def test_live_prefix_is_refused(self):
        # The message, not just the type: every other preflight failure is also a
        # ToolError, so `assertRaises` alone passed on machines where the rig is
        # absent even if this guard were deleted.
        saved = om.PREFIX
        try:
            om.PREFIX = om.LIVE_PREFIX
            with self.assertRaisesRegex(om.ToolError, "live prefix"):
                om.preflight()
        finally:
            om.PREFIX = saved

    def test_a_partial_session_blocks_a_second_up(self):
        # Xvfb without wine is still "up": starting a second one would overwrite
        # the PID and orphan the first display.
        saved = om.SESSION
        try:
            om.SESSION = om.Session()
            om.SESSION.xvfb = types.SimpleNamespace(pid=1)
            with self.assertRaisesRegex(om.ToolError, "already up"):
                om.tool_up()
        finally:
            om.SESSION = saved


class PageVerdict(unittest.TestCase):
    def test_browser_is_not_an_instrument_page(self):
        v = om.page_verdict({"sd": 0.169495, "colours": 5709},
                            {"sd": 0.023239, "colours": 2})
        self.assertTrue(v["ui_rendered"])
        self.assertFalse(v["instrument_page"])
        self.assertEqual(v["mode"], "browser")

    def test_measured_instrument_page(self):
        v = om.page_verdict({"sd": 0.257729, "colours": 56178},
                            {"sd": 0.316852, "colours": 216})
        self.assertTrue(v["instrument_page"])
        self.assertEqual(v["mode"], "instrument")

    def test_blank_window_is_blank(self):
        v = om.page_verdict({"sd": 0.0, "colours": 1}, {"sd": 0.0, "colours": 1})
        self.assertEqual(v["mode"], "blank")
        self.assertFalse(v["ui_rendered"])

    def test_strip_is_clamped_to_the_window(self):
        self.assertEqual(om.strip_region({"w": 1010, "h": 666}), (380, 620, 600, 46))
        self.assertEqual(om.strip_region({"w": 1010, "h": 698}), (380, 620, 600, 60))


if __name__ == "__main__":
    unittest.main(verbosity=2)
