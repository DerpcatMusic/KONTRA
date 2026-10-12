#!/usr/bin/env python3
"""Exercise the recovery distinctions on disposable repositories only."""
import contextlib
import io
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from worktree_audit import audit


class RecoveryAuditTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="kontra-audit-test-")
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repository"
        self.repo.mkdir()
        self.run_git("init", "-b", "main")
        self.run_git("config", "user.name", "Recovery fixture")
        self.run_git("config", "user.email", "fixture@example.invalid")
        (self.repo / "voice.rs").write_text("gain = 0\n")
        self.commit("initial")

    def tearDown(self):
        self.temporary.cleanup()

    def run_git(self, *args, cwd=None, check=True):
        return subprocess.run(["git", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", "-C", str(cwd or self.repo), *args],
                              check=check, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def commit(self, message):
        self.run_git("add", "-A")
        self.run_git("commit", "-m", message)

    def collect(self):
        with contextlib.redirect_stdout(io.StringIO()):
            return audit(self.repo, "main", self.root / "output", 2, [self.root])

    def test_squash_equivalence_and_dirty_worktree_are_independent(self):
        self.run_git("checkout", "-b", "feature")
        (self.repo / "voice.rs").write_text("gain = 1\n")
        self.commit("feature fix")
        self.run_git("checkout", "main")
        self.run_git("merge", "--squash", "feature")
        self.run_git("commit", "-m", "squashed fix")
        checkout = self.root / "worktree with spaces"
        self.run_git("worktree", "add", str(checkout), "feature")
        (checkout / "voice.rs").write_text("gain = 2\n")
        (checkout / "pending file.rs").write_text("pending\n")
        before = self.run_git("status", "--porcelain=v1", "-z", cwd=checkout).stdout
        inventory = self.collect()
        feature = next(b for b in inventory["branches"] if b["name"] == "feature")
        self.assertFalse(feature["analysis"]["reachable_from_baseline"])
        self.assertTrue(feature["analysis"]["exact_tree_equal_to_baseline"])
        worktree = next(w for w in inventory["worktrees"] if w["path"] == str(checkout))
        self.assertEqual({c["path"] for c in worktree["changes"]}, {"voice.rs", "pending file.rs"})
        self.assertFalse(next(c for c in worktree["changes"] if c["path"] == "voice.rs")["working_bytes_equal_to_baseline"])
        self.assertEqual(before, self.run_git("status", "--porcelain=v1", "-z", cwd=checkout).stdout)
        self.assertEqual((checkout / "voice.rs").read_text(), "gain = 2\n")

    def test_conflicted_cherry_pick_and_missing_checkout_are_preserved(self):
        initial = self.run_git("rev-parse", "HEAD").stdout.decode().strip()
        self.run_git("checkout", "-b", "incoming")
        (self.repo / "voice.rs").write_text("gain = 1\n")
        self.commit("incoming")
        incoming = self.run_git("rev-parse", "HEAD").stdout.decode().strip()
        self.run_git("checkout", "main")
        (self.repo / "voice.rs").write_text("gain = 9\n")
        self.commit("conflicting change")
        conflict = self.run_git("cherry-pick", incoming, check=False)
        self.assertNotEqual(conflict.returncode, 0)
        missing = self.root / "missing"
        self.run_git("worktree", "add", "--detach", str(missing), initial)
        shutil.rmtree(missing)
        before = self.run_git("status", "--porcelain=v1", "-z").stdout
        inventory = self.collect()
        current = next(w for w in inventory["worktrees"] if w["path"] == str(self.repo))
        self.assertEqual(current["operations"]["CHERRY_PICK_HEAD"], incoming)
        self.assertEqual(current["unmerged_index_paths"], ["voice.rs"])
        self.assertEqual(inventory["counts"]["missing_worktrees"], 1)
        self.assertEqual(before, self.run_git("status", "--porcelain=v1", "-z").stdout)
        self.assertIn("<<<<<<< HEAD", (self.repo / "voice.rs").read_text())


if __name__ == "__main__":
    unittest.main()
