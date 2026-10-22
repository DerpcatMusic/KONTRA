"""Self-check for ksp_symbols.py doctor: classification, JSON shape and --strict exit."""
import importlib.util
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path

spec = importlib.util.spec_from_file_location("ksp_symbols", Path(__file__).parent / "re" / "ksp_symbols.py")
ks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ks)

SCRIPT = "on init\n  $UNKNOWN_KSP_DOCTOR_PROBE = 1\nend on\n"


def run(files, **kw):
    buf = StringIO()
    with redirect_stdout(buf):
        code = ks.doctor(files, **kw)
    return code, buf.getvalue()


class DoctorTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = Path(self.dir.name) / "probe.ksp"
        self.path.write_text(SCRIPT)

    def tearDown(self):
        self.dir.cleanup()

    def test_unknown_sigil_is_unsupported(self):
        rep = ks.script_report("probe", ks.scan_script(SCRIPT, ks.Resolver(ks.load_rows())),
                               ks.Resolver(ks.load_rows()))
        codes = {s["symbol"]: s["code"] for s in rep["symbols"]}
        self.assertEqual(codes["$UNKNOWN_KSP_DOCTOR_PROBE"], "KSP-UNKNOWN")
        self.assertIn("$UNKNOWN_KSP_DOCTOR_PROBE", rep["blockers"])

    def test_json_shape_and_strict_exit(self):
        code, out = run([str(self.path)], as_json=True, strict=True)
        data = json.loads(out)
        self.assertEqual(code, 1)
        self.assertEqual(data["scripts"][0]["script"], "probe.ksp")
        self.assertIn("$UNKNOWN_KSP_DOCTOR_PROBE", data["unsupported"])

    def test_non_strict_exit_is_zero(self):
        code, out = run([str(self.path)])
        self.assertEqual(code, 0)
        self.assertIn("summary:", out)
        self.assertIn("KSP-UNKNOWN", out)

    def test_full_without_tests_is_simplified(self):
        row = {"status": "full", "tests": "untested", "class": "compat", "category": "command",
               "owner": "", "corpus_rank": "", "name": "x"}
        self.assertEqual(ks.classify(row), ("simplified", "KSP-SIMPLIFIED"))
        row["tests"] = "1 (a.rs)"
        self.assertEqual(ks.classify(row), ("full", "KSP-FULL"))

    def test_modern_extension_class(self):
        row = {"status": "unsupported", "tests": "untested", "class": "modern-extension", "category": "event_par"}
        self.assertEqual(ks.classify(row), ("modern-extension", "KSP-MODERN"))


if __name__ == "__main__":
    unittest.main()
