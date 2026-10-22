"""Self-test for ksp_symbols.py: the committed catalogue is in sync and the doctor resolves."""
import contextlib
import io
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ksp_symbols as k  # noqa: E402

FIXTURE = Path(__file__).resolve().parent / "fixtures" / "doctor_sample.ksp"


class KspSymbols(unittest.TestCase):
    def test_committed_catalogue_is_in_sync(self):
        self.assertEqual(k.check(), 0)

    def test_every_row_has_status_and_test_link(self):
        rows = k.load_rows()
        self.assertTrue(rows)
        self.assertTrue(all(r["status"] in k.STATUSES for r in rows))
        self.assertTrue(all(r["tests"] for r in rows))

    def test_doctor_resolves_templates_and_flags_unknown_constants(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = k.doctor([str(FIXTURE)], "unknown")
        out = buf.getvalue()
        self.assertIn("$ENGINE_PAR_RV2_DAMPING", out)
        self.assertIn("KSP-UNKNOWN", out)
        self.assertIn("on note", out)
        self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
