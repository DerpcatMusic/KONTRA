import unittest

from check_rust_doctor import check


class ReportGate(unittest.TestCase):
    def test_incomplete_or_missing_evidence_never_passes(self):
        clean = {"complete": True, "errors": [], "gate": {"status": "passed"}}
        self.assertTrue(check(clean)[0])
        for report in (
            {},
            {**clean, "complete": False},
            {**clean, "errors": [{"code": "build-failed"}]},
            {**clean, "gate": {"status": "failed"}},
        ):
            self.assertFalse(check(report)[0])

    def test_score_requires_authoritative_complete_report(self):
        report = {"complete": True, "errors": [], "gate": {"status": "passed"},
                  "audit": {"score": {"value": 90, "authoritative": True}}}
        self.assertTrue(check(report, 90)[0])
        self.assertFalse(check(report, 91)[0])
        for score in ({}, {"value": 100, "authoritative": False}):
            self.assertFalse(check({**report, "audit": {"score": score}}, 90)[0])
        self.assertFalse(check({**report, "complete": False}, 90)[0])

    def test_scoped_snapshot_preserves_sources_policy_and_profiles(self):
        from pathlib import Path
        import tempfile
        import tomllib
        from scan_new_core import snapshot

        root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary)
            evidence = snapshot(root, destination)
            self.assertEqual(tomllib.loads((destination / 'Cargo.toml').read_text())['profile'],
                             tomllib.loads((root / 'Cargo.toml').read_text())['profile'])
            self.assertIn('crates/sampler-core', evidence['members'])
            self.assertIn('crates/sampler-native', evidence['members'])
            self.assertFalse((destination / 'src').exists())
            for name in evidence['source_sha256']:
                self.assertEqual((destination / name).read_bytes(), (root / name).read_bytes())
