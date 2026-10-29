"""Offline tests for change filtering, stable gate and snapshot dependencies."""
import itertools
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

from ci_changes import build_relevant, changed_code

ROOT = Path(__file__).resolve().parents[2]


class Changes(unittest.TestCase):
    def test_paths(self):
        for path in ("README.md", "docs/CI.md", "audits/report.json"):
            self.assertFalse(build_relevant(path), path)
        for path in ("src/access.rs", "tests/playback.rs", "Cargo.toml", "Cargo.lock",
                     "build.rs", ".cargo/config.toml", "vendor/new/file.rs", "assets/font.ttf",
                     ".github/workflows/ci.yml", "rust-toolchain.toml", "new-input.dat",
                     "THIRD_PARTY.md", "LICENSE", "NOTICE", "docs/LEGAL.md",
                     "licenses/MOOSE/NOTICE", "licenses/THIRD_PARTY_NOTICES.txt"):
            self.assertTrue(build_relevant(path), path)

    def test_git_diffs_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, stderr=subprocess.DEVNULL).decode().strip()
            git("init", "-q", "-b", "ci-test")
            git("config", "user.email", "ci@example.invalid")
            git("config", "user.name", "CI test")
            (root / "README.md").write_text("one")
            git("add", ".")
            git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            (root / "README.md").write_text("two")
            git("commit", "-qam", "docs")
            self.assertFalse(changed_code(base, "HEAD", cwd=root))
            self.assertFalse(changed_code("HEAD", "HEAD", cwd=root))
            self.assertTrue(changed_code(base, "HEAD", force=True, cwd=root))
            self.assertTrue(changed_code("missing-tag", "HEAD", cwd=root))
            self.assertTrue(changed_code("", "HEAD", cwd=root))
            self.assertTrue(changed_code("0" * 40, "HEAD", cwd=root))
            git("mv", "README.md", "source.rs")
            git("commit", "-qm", "renamed into source")
            self.assertTrue(changed_code(base, "HEAD", cwd=root))
            before_delete = git("rev-parse", "HEAD")
            git("rm", "source.rs")
            git("commit", "-qm", "remove source")
            self.assertTrue(changed_code(before_delete, "HEAD", cwd=root))


class Gates(unittest.TestCase):
    def test_required_gate_states(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        gate = textwrap.dedent(workflow.split("      - name: Require every applicable check\n", 1)[1].split("        run: |\n", 1)[1])
        states = ("success", "failure", "cancelled", "skipped")
        for code, changes in itertools.product(("true", "false", ""), states):
            for native in (("success",) * 4, ("skipped",) * 4,
                           ("failure", "success", "success", "success"),
                           ("success", "cancelled", "success", "success"),
                           ("success", "success", "skipped", "success"),
                           ("success", "success", "success", "failure")):
                env = dict(os.environ, CODE=code, CHANGES=changes,
                           LINUX=native[0], LAYER_TESTS=native[0], WINDOWS=native[1], MACOS=native[2], EDITOR=native[3],
                           RELEASE_VALIDATION="false", SHIPPING_PLAN="skipped",
                           SHIPPING_TESTS="skipped", SHIPPING_COVERAGE="skipped")
                result = subprocess.run(["bash", "-e", "-c", gate], env=env)
                expected = changes == "success" and (
                    (code == "true" and native == ("success",) * 4) or
                    (code == "false" and native == ("skipped",) * 4))
                self.assertEqual(result.returncode == 0, expected, (code, changes, native))

    def test_shipping_required_gate_states(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        gate = textwrap.dedent(workflow.split("      - name: Require every applicable check\n", 1)[1].split("        run: |\n", 1)[1])
        states = ("success", "failure", "cancelled", "skipped")
        for release, code, shipping in itertools.product(("true", "false", ""), ("true", "false"), itertools.product(states, repeat=3)):
            native = "success" if code == "true" else "skipped"
            env = dict(os.environ, CODE=code, CHANGES="success", LINUX=native, LAYER_TESTS=native,
                       WINDOWS=native, MACOS=native, EDITOR=native,
                       RELEASE_VALIDATION=release, SHIPPING_PLAN=shipping[0],
                       SHIPPING_TESTS=shipping[1], SHIPPING_COVERAGE=shipping[2])
            result = subprocess.run(["bash", "-e", "-c", gate], env=env)
            expected = ((release == "true" and code == "true" and shipping == ("success",) * 3)
                        or (release == "false" and shipping == ("skipped",) * 3))
            self.assertEqual(result.returncode == 0, expected, (release, code, shipping))

    def test_snapshot_graph(self):
        release = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("  push:\n    branches: [main]", release)
        self.assertIn("    needs: [verify, bundle]", release)
        self.assertIn("cargo moose build --clap --vst3 -p kontra-plugin", release)
        self.assertIn("gh release create", release)
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        triggers = ci.split("\non:\n", 1)[1].split("\npermissions:\n", 1)[0]
        self.assertNotIn("  push:", triggers)
        for event in ("pull_request", "merge_group", "workflow_dispatch", "workflow_call"):
            self.assertIn("  " + event + ":", triggers)
        self.assertNotIn("paths-ignore", ci)
        self.assertIn("FORCE: ${{ inputs.release_validation ||", ci)
        self.assertIn("python3 tools/version.py check\n          python3 tools/version.py self-test", ci)
        self.assertEqual(ci.count("      - name: Nightly shipping identity"), 5)
        self.assertEqual(ci.count('echo "KONTRA_NIGHTLY_BUILD=1"'), 5)
        self.assertEqual(ci.count("python3 tools/test_version.py"), 6)
        self.assertEqual(ci.count("python3 tools/version.py nightly-dirty"), 5)
        self.assertEqual(ci.count("if: inputs.release_validation || github.event_name == 'pull_request'"), 5)
        self.assertIn("    if: always()\n    needs: [changes, linux, layer-tests, windows, macos, editor, shipping-plan, shipping-tests, shipping-coverage]", ci)
        self.assertIn("    uses: ./.github/workflows/plugin-ui.yml", ci)
        self.assertNotIn("cargo test --release --features library-access", ci)


if __name__ == "__main__":
    unittest.main()
