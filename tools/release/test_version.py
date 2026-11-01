import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('release_version', Path(__file__).resolve().parents[1] / 'version.py')
version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(version)


class VersionTest(unittest.TestCase):
    def test_nightly_uses_commit_time_and_count(self):
        with patch.object(version, 'git', side_effect=['0', '42']):
            self.assertEqual(version.nightly(), '3.0.0-nightly.19700101.42')


if __name__ == '__main__':
    unittest.main()
