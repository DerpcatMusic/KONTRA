"""Source-only checks for reproducible Nightly stamping; no Cargo invocation."""
import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("version", Path(__file__).with_name("version.py"))
version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(version)


class NightlySource(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git("init", "-q", "-b", "nightly-test")
        self.git("config", "user.name", "Nightly test")
        self.git("config", "user.email", "nightly@example.invalid")
        self.base = {
            "Cargo.toml": '[package]\nname = "kontakto"\nversion = "0.3.344"\n',
            "Cargo.lock": '[[package]]\nname = "kontakto"\nversion = "0.3.344"\n\n[[package]]\nname = "other"\nversion = "1.0.0"\n',
            "source.rs": "// reviewed source\n",
        }
        for name, text in self.base.items():
            (self.root / name).write_text(text)
        self.git("add", ".")
        self.git("commit", "-qm", "reviewed source")
        revision = self.git("rev-parse", "HEAD")
        epoch = int(self.git("show", "-s", "--format=%ct", "HEAD"))
        self.stamp = version.nightly("0.3.344", revision, epoch)
        for name in ("Cargo.toml", "Cargo.lock"):
            (self.root / name).write_text(self.base[name].replace('version = "0.3.344"', f'version = "{self.stamp}"'))

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args], text=True).strip()

    def dirty(self):
        with patch.object(version, "ROOT", self.root):
            return version.nightly_dirty()

    def test_only_exact_nightly_stamp_is_clean(self):
        self.assertTrue(self.git("status", "--porcelain", "--untracked-files=no"))
        self.assertFalse(self.dirty())
        (self.root / "build-output.json").write_text("{}")
        self.assertFalse(self.dirty(), "untracked output is not source")

    def test_source_and_dependency_changes_remain_dirty(self):
        for name, suffix in (("source.rs", "// unexpected edit\n"),
                             ("Cargo.toml", '\n[features]\nunreviewed = []\n'),
                             ("Cargo.lock", '\n[[package]]\nname = "extra"\nversion = "1.0.0"\n')):
            path = self.root / name
            before = path.read_text()
            path.write_text(before + suffix)
            self.assertTrue(self.dirty(), name)
            path.write_text(before)
        self.git("mv", "source.rs", "renamed.rs")
        self.assertTrue(self.dirty(), "staged rename is a source change")

    def test_wrong_or_unstamped_version_is_dirty(self):
        path = self.root / "Cargo.toml"
        path.write_text(path.read_text().replace(self.stamp, "0.3.344-nightly.wrong"))
        self.assertTrue(self.dirty())
        for name in ("Cargo.toml", "Cargo.lock"):
            (self.root / name).write_text(self.base[name])
        self.assertTrue(self.dirty(), "Nightly mode requires the exact stamp")


if __name__ == "__main__":
    unittest.main()
