import contextlib
import io
import json
import subprocess
import unittest
from unittest.mock import patch

import dev


def artifact(name="kontakto", executable="/fresh/dev-tests", test=True):
    return json.dumps({"reason": "compiler-artifact", "target": {"name": name},
                       "profile": {"test": test}, "executable": executable})


class DevelopmentCommands(unittest.TestCase):
    def test_checks_only_requested_target_and_keeps_shipping_profiles(self):
        for target, expected in [("lib", ["--lib"]), ("cli", ["--bin", "kontakto"]),
                                 ("standalone", ["--bin", "kontakto-standalone", "--features", "standalone"]),
                                 ("all", ["--all-targets", "--all-features"])]:
            cmd = dev.command(dev.arguments(["check", "--target", target]))
            self.assertEqual(cmd[-len(expected):], expected)
            self.assertIn("--locked", cmd)
            self.assertNotIn("--release", cmd)
        self.assertNotIn("--all-features", dev.command(dev.arguments(["check"])))
        self.assertIn("--no-default-features", dev.command(dev.arguments(["check", "--core"])))

    def test_core_scope_and_jobs_reject_invalid_arguments(self):
        with contextlib.redirect_stderr(io.StringIO()):
            for args in [["--jobs", "0", "check"], ["check", "--core", "--target", "standalone"], ["test"]]:
                with self.assertRaises(SystemExit):
                    dev.arguments(args)

    def test_development_cache_never_inherits_frozen_release_target(self):
        source = {"CARGO_TARGET_DIR": "/frozen/release", "CARGO_INCREMENTAL": "0"}
        env = dev.environment(source)
        self.assertEqual(env["CARGO_TARGET_DIR"], str(dev.ROOT / "artifacts" / "dev-target"))
        self.assertEqual(env["CARGO_INCREMENTAL"], "1")
        self.assertEqual(source["CARGO_INCREMENTAL"], "0")
        self.assertEqual(dev.environment({"KONTRA_DEV_TARGET_DIR": "/my/dev-cache"})["CARGO_TARGET_DIR"], "/my/dev-cache")

    def test_binary_selection_requires_current_cargo_test_artifact(self):
        rows = "\n".join(["not json", artifact(test=False), artifact("dependency", "/other"), artifact()])
        self.assertEqual(dev.test_binary(rows, "lib"), "/fresh/dev-tests")
        self.assertEqual(dev.test_binary(artifact("playback"), "playback"), "/fresh/dev-tests")
        for rows in [artifact(test=False), artifact() + "\n" + artifact(executable="/second")]:
            with self.assertRaises(ValueError):
                dev.test_binary(rows, "lib")

    def test_missing_or_ignored_tests_cannot_silently_pass(self):
        available = dev.listed_tests("a::test: test\na::slow: test\n3 tests, 0 benchmarks")
        dev.validate_names(["a::test"], available, {"a::slow"}, False)
        dev.validate_names(["a::slow"], available, {"a::slow"}, True)
        for names, ignored in [(["test"], False), (["a::slow"], False), (["a::test"], True)]:
            with self.assertRaises(ValueError):
                dev.validate_names(names, available, {"a::slow"}, ignored)

    def test_print_is_read_only_and_does_not_run_cargo(self):
        with patch("dev.subprocess.run") as run, contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(dev.main(["--print", "test", "a::test"]), 0)
            run.assert_not_called()

    def test_scoped_run_builds_once_and_uses_exact_verified_names(self):
        built = subprocess.CompletedProcess([], 0, artifact())
        finished = subprocess.CompletedProcess([], 0)
        with patch("dev.subprocess.run", side_effect=[built, finished, finished]) as run, \
             patch("dev.subprocess.check_output", side_effect=["a::test: test\nb::test: test\n", ""]) as listing, \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(dev.main(["test", "a::test", "b::test", "a::test"]), 0)
            self.assertEqual(run.call_count, 3)
            self.assertIn("--no-run", run.call_args_list[0].args[0])
            self.assertEqual(run.call_args_list[1].args[0],
                             ["/fresh/dev-tests", "a::test", "--exact", "--nocapture", "--test-threads=1"])
            self.assertEqual(listing.call_args_list[1].args[0], ["/fresh/dev-tests", "--list", "--ignored"])

    def test_build_failure_never_runs_an_old_binary(self):
        with patch("dev.subprocess.run", return_value=subprocess.CompletedProcess([], 7, "build failed")) as run, \
             patch("dev.subprocess.check_output") as listing, \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(dev.main(["test", "a::test"]), 7)
            self.assertEqual(run.call_count, 1)
            listing.assert_not_called()


if __name__ == "__main__":
    unittest.main()
