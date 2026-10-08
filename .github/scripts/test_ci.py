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
            for native in (("success",) * 3, ("skipped",) * 3,
                           ("failure", "success", "success"),
                           ("success", "cancelled", "success"),
                           ("success", "success", "skipped")):
                env = dict(os.environ, CODE=code, CHANGES=changes,
                           LINUX=native[0], WINDOWS=native[1], MACOS=native[2])
                result = subprocess.run(["bash", "-e", "-c", gate], env=env)
                expected = changes == "success" and (
                    (code == "true" and native == ("success",) * 3) or
                    (code == "false" and native == ("skipped",) * 3))
                self.assertEqual(result.returncode == 0, expected, (code, changes, native))

    def test_snapshot_graph(self):
        nightly = (ROOT / ".github/workflows/nightly.yml").read_text()
        self.assertIn("  push:\n    branches: [main]", nightly)
        self.assertNotIn("schedule:", nightly)
        self.assertIn("    uses: ./.github/workflows/ci.yml\n    with:\n      release_validation: true", nightly)
        build = nightly.split("  build:\n", 1)[1].split("\n  macos-universal:\n", 1)[0]
        self.assertNotIn("needs:", build)
        self.assertIn("os: ubuntu-22.04, target: x86_64-unknown-linux-gnu", build)
        self.assertIn("key: nightly-${{ matrix.os }}-${{ matrix.target }}-release", build)
        self.assertLess(build.index("uses: actions/setup-python@"), build.index("- name: Nightly version"))
        self.assertIn("python3 .github/scripts/check_glibc.py", build)
        release = nightly.split("\n  release:\n", 1)[1]
        self.assertIn("    needs: [verify, build, macos-universal]\n", release)
        universal = nightly.split("\n  macos-universal:\n", 1)[1].split("\n  release:\n", 1)[0]
        self.assertIn("    needs: build\n", universal)
        self.assertIn("package_macos.sh", universal)
        self.assertIn("          ref: ${{ github.sha }}", nightly)
        self.assertIn("            ${{ env.STAGE }}.zip.sha256", nightly)
        self.assertIn("      cancel-in-progress: false", nightly)
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        triggers = ci.split("\non:\n", 1)[1].split("\npermissions:\n", 1)[0]
        self.assertNotIn("  push:", triggers)
        for event in ("pull_request", "merge_group", "workflow_dispatch", "workflow_call"):
            self.assertIn("  " + event + ":", triggers)
        self.assertNotIn("paths-ignore", ci)
        self.assertIn("FORCE: ${{ inputs.release_validation ||", ci)
        self.assertIn("python3 tools/version.py check\n          python3 tools/version.py self-test", ci)
        self.assertEqual(ci.count("      - name: Nightly shipping identity"), 3)
        self.assertIn("    if: always()\n    needs: [changes, linux, windows, macos]", ci)
        self.assertNotIn("cargo test --release --features library-access", ci)


if __name__ == "__main__":
    unittest.main()
