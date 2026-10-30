import tempfile
import plistlib
import unittest
from pathlib import Path
from unittest.mock import patch
import package_macos


class PackageTests(unittest.TestCase):
    def test_compose_and_reject_mismatched_revision(self):
        with tempfile.TemporaryDirectory(dir='.') as directory:
            root = Path(directory)
            for arch in ('arm64', 'x86_64'):
                source = root / arch
                source.mkdir()
                (source / 'SOURCE_COMMIT.txt').write_text('abc\n')
                for fmt in package_macos.BUNDLES:
                    binary = source / f'KONTRA.{fmt}/Contents/MacOS/KONTRA'
                    binary.parent.mkdir(parents=True)
                    binary.write_bytes(b'binary')
                    (binary.parent.parent / 'Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable': 'KONTRA', 'CFBundleIdentifier': 'audio.matari.kontra'}))
            def output(command, **_):
                return 'arm64\n' if '/arm64/' in str(command[-1]) else 'x86_64\n'
            def run(command, **_):
                if '-create' in command:
                    Path(command[-1]).write_bytes(b'universal')
            with patch('package_macos.subprocess.check_output', side_effect=output), patch('package_macos.subprocess.run', side_effect=run):
                package_macos.compose(root / 'arm64', root / 'x86_64', root / 'stage', 'abc', '3.0.0-nightly.20261030.1')
                for name in package_macos.BUNDLES.values():
                    self.assertEqual((root / 'stage/payload' / name / 'Contents/MacOS/KONTRA').read_bytes(), b'universal')
                intel_plist = root / 'x86_64/KONTRA.clap/Contents/Info.plist'
                intel_plist.write_bytes(plistlib.dumps({'CFBundleExecutable': 'KONTRA', 'CFBundleIdentifier': 'wrong'}))
                with self.assertRaisesRegex(ValueError, 'CFBundleIdentifier differs'):
                    package_macos.compose(root / 'arm64', root / 'x86_64', root / 'mismatch', 'abc', '3.0.0')
                with self.assertRaisesRegex(ValueError, 'revision mismatch'):
                    package_macos.compose(root / 'arm64', root / 'x86_64', root / 'other', 'wrong', '3.0.0')


if __name__ == '__main__':
    unittest.main()
