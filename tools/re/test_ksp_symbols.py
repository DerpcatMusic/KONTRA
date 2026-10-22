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
            code = k.doctor([str(FIXTURE)], strict=True)
        out = buf.getvalue()
        self.assertIn("$ENGINE_PAR_RV2_DAMPING", out)
        self.assertIn("KSP-UNKNOWN", out)
        self.assertIn("on note", out)
        self.assertEqual(code, 1)

    def test_nkb_bank_input_is_a_usage_error(self):
        with self.assertRaises(SystemExit):
            k.scripts_of("bank.nkb")

    def test_full_row_that_loses_its_test_fails_the_gate(self):
        row = {"id": "command:x", "status": "full", "tests": "untested"}
        self.assertEqual(len(k.full_without_test([row], set())), 1)
        self.assertEqual(k.full_without_test([row], {"command:x"}), [])
        tested = {**row, "tests": "2 (audit.rs)"}
        self.assertEqual(len(k.full_without_test([tested], {"command:x"})), 1)
        self.assertEqual(k.full_without_test([{**row, "status": "simplified"}], set()), [])

    def test_checked_in_ksp_fixtures_pass_strict_doctor(self):
        fixtures = sorted(str(p) for p in (Path(__file__).resolve().parent / "fixtures" / "ksp").glob("*.ksp"))
        self.assertTrue(fixtures)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            self.assertEqual(k.doctor(fixtures, strict=True), 0, buf.getvalue())

    def test_library_is_the_folder_above_instruments(self):
        self.assertEqual(k.library_of("/x/Conflux/Instruments/Conflux.nki"), "Conflux")
        self.assertEqual(k.library_of("/x/Loose/a.nki"), "Loose")


if __name__ == "__main__":
    unittest.main()
