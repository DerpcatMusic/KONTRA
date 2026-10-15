"""Publication contract: reject misleading support and never import private catalog caches."""
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_site

SHA = 'a' * 40
DATE = '2026-10-16T12:00:00+00:00'

def feature(**changes):
    row = dict(id='kontakt/effect/test', product='Kontakt', area='Effects', name='Test <module>', status='partial', note='Incomplete', src='crates/sampler-kontakt/src/library.rs', proof='unproven')
    row.update(changes)
    return row

class PublicationTests(unittest.TestCase):
    def test_exact_revision_and_escaping(self):
        data = build_site.collect([feature()], SHA, DATE)
        self.assertIn('/' + SHA + '/', data['rows'][0]['source'])
        self.assertEqual(data['counts'], {'partial': 1})
        markup = build_site.render_table(data)
        self.assertIn('Test &lt;module&gt;', markup)
        self.assertNotIn('Test <module>', markup)
        self.assertIn('\\u003c', markup)

    def test_unproved_is_never_implemented(self):
        with self.assertRaisesRegex(ValueError, 'lacks verification'):
            build_site.collect([feature(status='done')], SHA, DATE)
        self.assertEqual(build_site.collect([feature(status='done', proof='proved')], SHA, DATE)['counts'], {'done': 1})

    def test_fail_closed_on_bad_data(self):
        for row in [feature(status='magic'), feature(src='../outside'), feature(src='missing.rs')]:
            with self.assertRaises(ValueError):
                build_site.collect([row], SHA, DATE)
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            build_site.collect([feature(), feature()], SHA, DATE)

    def test_source_only_skips_private_cache(self):
        self.assertEqual(os.environ['KONTRA_COVERAGE_SOURCE_ONLY'], '1')
        with patch.object(build_site.tracker.os.path, 'exists', side_effect=AssertionError('Private cache inspected')):
            build_site.tracker.uvi_snapshot()
        with patch.object(build_site.tracker.os.path, 'exists', side_effect=AssertionError('Private cache inspected')), patch.object(build_site.tracker.build, 'tsv', return_value=[{'target_id': '1'}]) as read:
            build_site.tracker.build.target_table()
            self.assertEqual(Path(read.call_args.args[0]).name, 'modulation-targets.tsv')

    def test_changed_code_probe_downgrades_status(self):
        row = dict(status='exact', note='', probe='example.rs::implemented')
        with patch.object(build_site.tracker.build, 'probe_ok', return_value=False):
            status, note = build_site.tracker.derive(row)
            self.assertEqual(status, 'partial')
            self.assertIn('re-verification', note)
            self.assertEqual(build_site.tracker.proof_of(row), 'unproven')

if __name__ == '__main__':
    unittest.main()
