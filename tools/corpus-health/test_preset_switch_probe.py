import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

MODULE = Path(__file__).with_name("preset_switch_probe.py")
spec = importlib.util.spec_from_file_location("preset_switch_probe", MODULE)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class ProbeTests(unittest.TestCase):
    def test_failed_projection_keeps_decode_counts_unknown(self):
        report = probe.augment({"steps": [{"step": 0, "ui_projection": [{"status": "failed", "asset_summary": None}]}]}, "")
        self.assertEqual(report["steps"][0]["work_counts"]["ui_projection_calls"], 1)
        self.assertIsNone(report["steps"][0]["work_counts"]["asset_decode_jobs"])
        self.assertEqual(probe.projection_count([{"asset_summary": {"decodes": 4}}, {"asset_summary": {"decodes": 3}}], "decodes"), 7)

    def test_spans_follow_actual_phase_and_worker_payload_is_not_retained(self):
        lines = [
            'PRESET_SWITCH {"step":0,"phase":"prepare"}',
            'AUDIT {"stage":"ni_objects_parse","ms":3.5}',
            'AUDIT {"stage":"ni_objects_parse","ms":2}',
            'AUDIT {"stage":"translate_ksp_init","ms":4}',
            'head_prepare_audit {"jobs":7,"failures":1,"dropped_probes":2,"probes":["PRIVATE_SAMPLE_PATH"]}',
            'PRIVATE_AUTHORED_LOG',
            'PRESET_SWITCH {"step":0,"phase":"activation"}',
            'AUDIT {"stage":"runtime_alloc_init","ms":8}',
            'PRESET_SWITCH {"step":0,"phase":"end"}',
            'PRESET_SWITCH {"step":1,"phase":"prepare"}',
            'AUDIT {"stage":"preset_cache_decode","ms":1}',
        ]
        result = probe.augment({"steps": [{"step": 0}, {"step": 1}]}, "\n".join(lines))
        self.assertEqual(result["steps"][0]["work_counts"]["base_object_parse_spans"], 2)
        self.assertEqual(result["steps"][0]["work_counts"]["script_initialization_spans"], 1)
        self.assertEqual(result["steps"][1]["work_counts"]["base_object_parse_spans"], 0)
        self.assertEqual(result["steps"][0]["work_counts"]["head_worker_jobs"]["jobs"], 7)
        self.assertTrue(result["audit_capture"]["complete"])
        self.assertNotIn("PRIVATE", json.dumps(result))

    def test_caller_cpu_and_cross_phase_breadcrumbs_keep_missing_cpu_unknown(self):
        result = probe.augment({"steps": [{"step": 0}]}, "\n".join([
            'PRESET_SWITCH {"step":0,"phase":"prepare"}',
            'AUDIT_BEGIN {"stage":"uvi_directory_namespace","span":9}',
            'PRESET_SWITCH {"step":0,"phase":"activation"}',
            'AUDIT {"stage":"uvi_directory_namespace","span":9,"ms":100,"thread_cpu_ms":4}',
            'AUDIT {"stage":"legacy_stage","ms":2}',
            'AUDIT {"stage":"legacy_stage","ms":3,"thread_cpu_ms":null}',
        ]))
        rows = {r["stage"]:r for r in result["steps"][0]["loader_audit_spans"]}
        self.assertEqual(rows["uvi_directory_namespace"]["phase"], "prepare")
        self.assertEqual(rows["uvi_directory_namespace"]["thread_cpu_ms_sum"], 4)
        self.assertEqual(rows["uvi_directory_namespace"]["thread_cpu_calls"], 1)
        self.assertEqual(rows["legacy_stage"]["calls"], 2)
        self.assertEqual(rows["legacy_stage"]["thread_cpu_calls"], 0)
        self.assertIsNone(rows["legacy_stage"]["thread_cpu_ms_sum"])
        self.assertTrue(result["audit_capture"]["complete"])

    def test_unfinished_breadcrumbs_and_invalid_cpu_do_not_claim_completion(self):
        lines = ['PRESET_SWITCH {"step":0,"phase":"prepare"}',
                 'AUDIT_BEGIN {"stage":"uvi_directory_namespace","span":7}']
        result = probe.augment({"steps": [{"step": 0}]}, "\n".join(lines))
        self.assertFalse(result["audit_capture"]["complete"])
        self.assertEqual(result["audit_capture"]["unfinished_spans"],
                         [{"span":7,"step":0,"phase":"prepare","stage":"uvi_directory_namespace"}])
        self.assertIsNone(result["steps"][0]["work_counts"]["base_object_parse_spans"])
        for cpu in (-1, True, "PRIVATE_VALUE"):
            invalid = probe.augment({"steps": [{"step": 0}]}, "\n".join([
                lines[0], 'AUDIT ' + json.dumps({"stage":"stage","ms":1,"thread_cpu_ms":cpu})]))
            self.assertFalse(invalid["audit_capture"]["complete"])
            self.assertNotIn("PRIVATE", json.dumps(invalid))
        overflow = probe.augment({"steps": [{"step": 0}]}, "\n".join([
            lines[0], 'AUDIT {"stage":"stage","ms":1e308,"thread_cpu_ms":1e308}',
            'AUDIT {"stage":"stage","ms":1e308,"thread_cpu_ms":1e308}']))
        self.assertFalse(overflow["audit_capture"]["complete"])
        json.dumps(overflow, allow_nan=False)

    def test_missing_boundary_or_bad_numeric_audit_retains_unknown_counts(self):
        for lines in (
            'AUDIT {"stage":"ni_objects_parse","ms":1}',
            'PRESET_SWITCH {"step":0,"phase":"prepare"}\nAUDIT {"stage":"ni_objects_parse","ms":-1}',
            'PRESET_SWITCH {"step":0,"phase":"prepare"}\nAUDIT {"stage":"private authored name","ms":1}',
        ):
            result = probe.augment({"steps": [{"step": 0}]}, lines)
            self.assertFalse(result["audit_capture"]["complete"])
            self.assertIsNone(result["steps"][0]["work_counts"]["base_object_parse_spans"])

    def test_audit_overflow_is_reported_and_does_not_claim_exact_zero(self):
        original = probe.AUDIT_LIMIT
        try:
            probe.AUDIT_LIMIT = 1
            result = probe.augment({"steps": [{"step": 0}]}, '\n'.join([
                'PRESET_SWITCH {"step":0,"phase":"prepare"}',
                'AUDIT {"stage":"ni_objects_parse","ms":1}',
                'AUDIT {"stage":"ni_objects_parse","ms":2}',
            ]))
        finally:
            probe.AUDIT_LIMIT = original
        self.assertEqual(result["audit_capture"]["records_omitted"], 1)
        self.assertIsNone(result["steps"][0]["work_counts"]["base_object_parse_spans"])
        try:
            probe.AUDIT_LIMIT = 1
            result = probe.augment({"steps": [{"step": 0}]}, '\n'.join([
                'PRESET_SWITCH {"step":0,"phase":"prepare"}',
                'AUDIT_BEGIN {"stage":"first_stage","span":1}',
                'AUDIT_BEGIN {"stage":"second_stage","span":2}',
            ]))
        finally:
            probe.AUDIT_LIMIT = original
        self.assertEqual(result["audit_capture"]["breadcrumb_records_omitted"], 1)
        self.assertEqual(len(result["audit_capture"]["unfinished_spans"]), 1)
        self.assertFalse(result["audit_capture"]["complete"])

    def test_launcher_timeout_stops_and_reaps_child(self):
        process, _, _ = probe.invoke([sys.executable, "-c", "import time; time.sleep(10)"], dict(os.environ), 0.1)
        self.assertTrue(process["timed_out"])
        self.assertLess(process["elapsed_ms"], 5000)
        self.assertNotEqual(process["exit_code"], 0)

    def test_launcher_drains_both_pipes_and_bounds_output(self):
        original = probe.OUTPUT_LIMIT
        try:
            probe.OUTPUT_LIMIT = 1024
            process, stdout, stderr = probe.invoke([sys.executable, "-c", "import os; os.write(2,b'e'*2048); os.write(1,b'o'*2048)"], dict(os.environ), 5)
        finally:
            probe.OUTPUT_LIMIT = original
        self.assertTrue(process["output_limit_exceeded"])
        self.assertLessEqual(len(stdout), 1024)
        self.assertLessEqual(len(stderr), 1024)

    def test_failed_old_helper_does_not_expose_authored_stderr(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            helper = directory / "helper.py"
            logs = ('PRESET_SWITCH {"step":0,"phase":"prepare"}\n'
                    'AUDIT_BEGIN {"stage":"uvi_directory_namespace","span":1}\nPRIVATE_AUTHORED_LOG')
            helper.write_text(f"#!{sys.executable}\nimport sys\nsys.stderr.write({logs!r})\nraise SystemExit(2)\n")
            helper.chmod(0o700)
            plan = directory / "plan.json"
            plan.write_text('{"schema":1,"steps":[]}')
            output = directory / "evidence.json"
            status = probe.main([str(plan), "--binary", str(helper), "--output", str(output), "--timeout", "5"])
            result = json.loads(output.read_text())
            self.assertEqual(status, 1)
            self.assertEqual(result["probe_report"], "unavailable")
            self.assertEqual(result["process"]["exit_code"], 2)
            self.assertTrue(result["provenance"]["binary_unchanged"])
            self.assertEqual(result["audit_capture"]["unfinished_spans"][0]["stage"], "uvi_directory_namespace")
            self.assertFalse(result["audit_capture"]["complete"])
            self.assertNotIn("PRIVATE", output.read_text())
            self.assertEqual(output.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                probe.main([str(plan), "--binary", str(helper), "--output", str(output)])


if __name__ == "__main__":
    unittest.main()
