#!/usr/bin/env python3
"""Synthetic libtest/probe output tests; never execute a native test artifact."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import ui_library_sweep as sweep


CASE = {"library": "Synthetic", "library_root": "/synthetic", "patch": "/synthetic/fixture.nki",
        "snapshot": None, "program": 0, "mode": "Original", "device_scale": 1,
        "view_scale": 0, "appearance": 0}
LISTING = f"{sweep.TEST}: test\n\n1 test, 0 benchmarks\n"


def evidence(cpu=False, callbacks=False):
    lines = ["UI_BENCH program=0 mode=1 device_scale=1 snapshot_restored=false",
             "UI_BENCH import_ms=1.234 setup_ms=0.125 controls=2 pictures=1",
             "UI_BENCH control=0 kind=Knob size=30x30 sprite_frames=8",
             "UI_BENCH view_scale=0 appearance=0 library_artwork=0",
             "UI_BENCH visible_control=0"]
    if not cpu:
        lines.append('UI_BENCH adapter=AdapterInfo { name: "Synthetic GPU", device_type: DiscreteGpu, backend: Vulkan }')
    renderer = ("cpu_paint", "cpu_raster") if cpu else ("gpu_prepare_upload_submit", "gpu_wait")
    for changing in ("false", "true"):
        stages = ["build", "layout", "scene_clone", *renderer, "snapshot_lock"]
        if changing == "true":
            stages.append("edit_control")
        for stage in stages:
            lines.append(f"UI_BENCH changing={changing} stage={stage} n=24 mean_ms=0.1 median_ms=0.1 p99_ms=0.2")
    if callbacks:
        report = {"worker": {"host_frames": 128, "edits_delivered": 24, "editor_frames": 4,
                             "latest_edits_published": 2}, "observer_frames": 4,
                  "changed_gpu_frames": 0 if cpu else 2,
                  "live_publication_ms": {"mean": 0.1, "p99": 0.2},
                  "callback_published_render_ms": {"mean": 0.1, "p99": 0.2}}
        lines.append("UI_BENCH_CALLBACK " + json.dumps(report))
    return "\n".join(lines)


def successful_output(metrics="", name=sweep.TEST, same_line=True):
    # --nocapture can append the first probe line to libtest's test header.
    separator = "" if same_line else "\n"
    return (f"running 1 test\ntest {name} ... {separator}{metrics}\nok\n\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out; finished in 0.01s\n")


class AdmissionTests(unittest.TestCase):
    def admit(self, output, **kwargs):
        return sweep.admit(output, 0, CASE, **kwargs)

    def test_exact_one_test_transcript_with_complete_gpu_evidence_is_admitted(self):
        for same_line in (True, False):
            with self.subTest(same_line=same_line):
                measurement, status = self.admit(successful_output(evidence(), same_line=same_line))
                self.assertEqual(status, "passed")
                self.assertEqual(len(measurement["stages"]), 13)

    def test_cpu_reference_requires_explicit_cpu_option(self):
        output = successful_output(evidence(cpu=True))
        self.assertEqual(self.admit(output, cpu=True)[1], "passed")
        self.assertEqual(self.admit(output)[1], "invalid_probe")

    def test_zero_tests_rejected_even_with_fabricated_metrics(self):
        output = evidence() + "\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out; finished in 0s\n"
        self.assertEqual(self.admit(output)[1], "invalid_probe")

    def test_different_test_rejected_even_with_complete_metrics(self):
        self.assertEqual(self.admit(successful_output(evidence(), name="ui::tests::frame_cost"))[1], "invalid_probe")

    def test_duplicate_or_ignored_test_transcript_rejected(self):
        output = successful_output(evidence())
        for modified in (output + f"test {sweep.TEST} ... ok\n",
                         output.replace("1 passed; 0 failed; 0 ignored", "0 passed; 0 failed; 1 ignored")):
            self.assertEqual(self.admit(modified)[1], "invalid_probe")

    def test_missing_measurements_rejected_after_one_test_passes(self):
        self.assertEqual(self.admit(successful_output())[1], "invalid_probe")

    def test_missing_stage_or_numeric_metric_rejected(self):
        for metrics in (evidence().replace("stage=build", "stage=missing"),
                        evidence().replace(" p99_ms=0.2", ""),
                        evidence().replace("mean_ms=0.1", "mean_ms=NaN"),
                        evidence().replace("n=24", "n=0")):
            with self.subTest(metrics=metrics[-90:]):
                self.assertEqual(self.admit(successful_output(metrics))[1], "invalid_probe")

    def test_case_identity_mismatch_rejected(self):
        self.assertEqual(self.admit(successful_output(evidence().replace("program=0", "program=2")))[1], "invalid_probe")

    def test_software_adapter_rejected(self):
        self.assertEqual(self.admit(successful_output(evidence().replace("DiscreteGpu", "Cpu")))[1], "invalid_probe")

    def test_explicit_successful_skip_needs_no_measurements(self):
        for reason in ("no_performance_view", "no_animated_knob_or_slider", "no_visible_controllable_surface"):
            output = successful_output(f"UI_BENCH status=skipped reason={reason} program=0")
            self.assertEqual(self.admit(output)[1], "skipped")

    def test_skip_requires_exact_test_and_reason(self):
        for output in (successful_output("UI_BENCH status=skipped program=0"),
                       successful_output("UI_BENCH status=skipped reason=no_performance_view program=0", name="other")):
            self.assertEqual(self.admit(output)[1], "invalid_probe")

    def test_native_worker_evidence_required_when_callbacks_requested(self):
        self.assertEqual(self.admit(successful_output(evidence()), callbacks=True)[1], "invalid_probe")
        self.assertEqual(self.admit(successful_output(evidence(callbacks=True)), callbacks=True)[1], "passed")
        self.assertEqual(self.admit(successful_output(evidence(callbacks=True).replace('"editor_frames": 4', '"editor_frames": 0')), callbacks=True)[1], "invalid_probe")

    def test_invalid_json_is_recordable_rejection(self):
        for json_text in ("{", "null", '{"worker": NaN}'):
            measurement, status = self.admit(successful_output(evidence() + "\nUI_BENCH_CALLBACK " + json_text))
            self.assertEqual(status, "invalid_probe")
            self.assertIn("probe_error", measurement)
        self.assertEqual(self.admit(successful_output(evidence() + "\nUI_BENCH_COST {"))[1], "invalid_probe")

    def test_nonzero_process_stays_failed(self):
        self.assertEqual(sweep.admit(successful_output(evidence()), 1, CASE)[1], "failed")


class DriverTests(unittest.TestCase):
    def invoke(self, output_text, listing=LISTING, *, returncode=0, timeout=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            plan = root / "plan.json"
            plan.write_text(json.dumps({"libraries": [{"name": CASE["library"], "path": CASE["library_root"],
                "patches": [{"path": CASE["patch"], "initial_read_audit": {"programs": [{"program": 0}]}}]}]}))
            binary = root / "not-an-executable"
            binary.write_text("synthetic path; subprocess.run is mocked")
            output = root / "output"
            responses = [subprocess.CompletedProcess([], 0, listing, ""),
                         subprocess.TimeoutExpired([], 1, output=b"partial output") if timeout else
                         subprocess.CompletedProcess([], returncode, output_text, "")]
            argv = ["ui_library_sweep.py", "--plan", str(plan), "--binary", str(binary), "--output", str(output),
                    "--mode", "Original", "--device-scale", "1"]
            with (patch.object(sys, "argv", argv), patch.object(sweep.subprocess, "run", side_effect=responses) as run,
                  contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO())):
                with self.assertRaises(SystemExit) as exited:
                    sweep.main()
            records = json.loads((output / "results.json").read_text())["records"] if output.exists() else None
            return exited.exception.code, records, run.call_args_list, output.exists()

    def test_absent_exact_ignored_test_refuses_before_output_or_case_run(self):
        for listing in ("0 tests, 0 benchmarks\n", f"{sweep.TEST}_replacement: test\n\n1 test, 0 benchmarks\n"):
            code, records, calls, exists = self.invoke("", listing)
            self.assertEqual(code, 1)
            self.assertIsNone(records)
            self.assertFalse(exists)
            self.assertEqual(len(calls), 1)
            self.assertEqual(calls[0].args[0][1:], ["--ignored", "--exact", sweep.TEST, "--list"])

    def test_invalid_case_is_recorded_and_fails_driver_exit(self):
        code, records, calls, exists = self.invoke(successful_output())
        self.assertTrue(exists)
        self.assertEqual(code, 1)
        self.assertEqual(records[0]["status"], "invalid_probe")
        self.assertIn("probe_error", records[0])
        self.assertEqual(len(calls), 2)

    def test_complete_case_and_explicit_skip_keep_success_exit(self):
        for text, expected in ((successful_output(evidence()), "passed"),
                               (successful_output("UI_BENCH status=skipped reason=no_performance_view program=0"), "skipped")):
            code, records, _, _ = self.invoke(text)
            self.assertEqual(code, 0)
            self.assertEqual(records[0]["status"], expected)

    def test_process_failure_and_timeout_fail_exit(self):
        for kwargs, expected in (({"returncode": 1}, "failed"), ({"timeout": True}, "timeout")):
            code, records, _, _ = self.invoke("", **kwargs)
            self.assertEqual(code, 1)
            self.assertEqual(records[0]["status"], expected)


if __name__ == "__main__":
    unittest.main()
