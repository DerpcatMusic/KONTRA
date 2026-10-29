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
    row = dict(id='kontakt/effect/test', product='Kontakt', area='Effects', name='Test <module>', status='partial', note='Incomplete', src='tools/coverage/v3-facts.json', proof='unproven')
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

    def test_v3_snapshot_has_scoped_evidence_without_private_paths(self):
        rows = build_site.tracker.all_features()
        self.assertTrue(any(row['id'].startswith('ksp/') for row in rows))
        self.assertTrue(any(row['id'].startswith('law/') for row in rows))
        self.assertTrue(any(row['id'].startswith('corpus/') for row in rows))
        self.assertTrue(all(row['status'] == 'partial' for row in rows))
        self.assertNotIn('/data/', str(rows))
        build_site.collect(rows, SHA, DATE)

if __name__ == '__main__':
    unittest.main()
