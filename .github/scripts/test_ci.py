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
    def test_required_gate_fails_closed(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        gate = textwrap.dedent(workflow.split("      - name: Require all checks\n", 1)[1].split("        run: |\n", 1)[1])
        for checks, v3 in itertools.product(("success", "failure", "cancelled", "skipped"), repeat=2):
            env = dict(os.environ, CHECKS=checks, V3=v3)
            result = subprocess.run(["bash", "-e", "-c", gate], env=env)
            self.assertEqual(result.returncode == 0, checks == v3 == "success")

    def test_release_uses_v3_verification(self):
        release = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("    - cron: '0 3 * * *'", release)
        self.assertNotIn("  push:", release)
        self.assertIn("uses: ./.github/workflows/v3-verify.yml", release)
        self.assertIn("needs.check.outputs.released == 'false'", release)
        self.assertIn("    needs: [verify, bundle]", release)
        self.assertIn("cargo moose build --clap --vst3 -p kontra-plugin", release)
        self.assertIn("gh release create", release)
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("uses: ./.github/workflows/v3-verify.yml", ci)
        self.assertNotIn("sampler-", ci)
        self.assertNotIn("kontakto", ci)


if __name__ == "__main__":
    unittest.main()
