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
