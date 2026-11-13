import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('release_version', Path(__file__).resolve().parents[1] / 'version.py')
version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(version)


class VersionTest(unittest.TestCase):
    def test_a_fix_raises_patch_and_a_feature_raises_minor(self):
        self.assertEqual(version.bump('v0.4.4', ['fix(ui): menus scroll (#1)']), '0.4.5')
        self.assertEqual(version.bump('v0.4.4', ['fix: a (#1)', 'feat(kontakt): snapshots (#2)']), '0.5.0')
        self.assertEqual(version.bump('v0.5.9', ['feat!: new state format (#3)']), '0.6.0')
        self.assertEqual(version.bump('v0.5.9', ['feature flags tidied (#4)']), '0.5.10')

    def test_the_first_release_is_0_4_0(self):
        self.assertEqual(version.bump(None, ['feat: a (#1)']), '0.4.0')


if __name__ == '__main__':
    unittest.main()
