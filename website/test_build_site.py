"""Publication contract: the progress scoreboard counts and escapes what it publishes."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_site

class PublicationTests(unittest.TestCase):
    def test_progress_counts_and_escapes(self):
        sound = dict(measured='2026-11-12', libraries=[
            dict(name='A <b>', passed=2, total=3, misses=['Chord: 2.0 dB quieter around 630 Hz']),
            dict(name='B', passed=3, total=3, misses=[])])
        markup = build_site.render_sound(sound)
        self.assertIn('<strong>5 of 6</strong>', markup)
        self.assertIn('A &lt;b&gt;', markup)
        self.assertEqual(markup.count('<details'), 1)


if __name__ == '__main__':
    unittest.main()
