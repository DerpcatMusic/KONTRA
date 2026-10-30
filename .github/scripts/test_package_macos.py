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


class SigningTests(unittest.TestCase):
    def test_failures_never_publish_installer(self):
        import json
        import os
        import subprocess
        with tempfile.TemporaryDirectory(dir='.') as directory:
            root = Path(directory).resolve()
            stage = root / 'stage'
            stage.mkdir()
            (stage / 'build-info.json').write_text(json.dumps({
                'target': 'universal-apple-darwin', 'architectures': ['arm64', 'x86_64'],
                'profile': 'release', 'version': '3.0.0-nightly.20261030.1'}))
            for bundle in package_macos.BUNDLES.values():
                binary = stage / 'payload' / bundle / 'Contents/MacOS/KONTRA'
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b'synthetic')
            tools = root / 'tools'
            tools.mkdir()
            mock = tools / 'mock'
            mock.write_text('''#!/usr/bin/env python3
import json, os, pathlib, plistlib, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
if name == os.environ.get('FAIL_TOOL') or (name == 'xcrun' and args[:2] == ['stapler', os.environ.get('FAIL_TOOL')]):
    sys.exit(1)
if name == 'pkgbuild':
    if '--analyze' in args:
        pathlib.Path(args[-1]).write_bytes(plistlib.dumps([{}]))
    else:
        components = plistlib.loads(pathlib.Path(args[args.index('--component-plist')+1]).read_bytes())
        assert not components[0]['BundleIsRelocatable'] and not components[0]['BundleIsVersionChecked']
        pathlib.Path(args[-1]).write_bytes(b'package')
if name == 'productsign': pathlib.Path(args[-1]).write_bytes(pathlib.Path(args[-2]).read_bytes())
if name == 'xcrun' and args[:2] == ['notarytool', 'submit']:
    print(json.dumps({'id': '00000000-0000-4000-8000-000000000000'}))
if name == 'xcrun' and args[:2] == ['notarytool', 'wait']:
    print(json.dumps({'id': args[2], 'status': os.environ.get('NOTARY_STATUS', 'Accepted')}))
''')
            mock.chmod(0o755)
            for name in ('lipo', 'codesign', 'pkgbuild', 'productsign', 'pkgutil', 'xcrun', 'spctl'):
                (tools / name).symlink_to(mock)
            env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ['PATH'],
                       TMPDIR=str(root), APPLE_SIGNING_KEYCHAIN='test.keychain', APPLE_TEAM_ID='TEAM',
                       APPLE_DEVELOPER_ID_APPLICATION='Developer ID Application: Test (TEAM)',
                       APPLE_DEVELOPER_ID_INSTALLER='Developer ID Installer: Test (TEAM)',
                       APPLE_ID='test', APPLE_APP_SPECIFIC_PASSWORD='test')
            script = Path(__file__).resolve().parent / 'package_macos.sh'
            for case in ('success', 'codesign', 'productsign', 'rejected', 'staple', 'validate', 'spctl'):
                with self.subTest(case=case):
                    output = root / (case + '.pkg')
                    result = subprocess.run(['bash', str(script), str(stage), str(output)],
                        env=dict(env, FAIL_TOOL=case if case != 'success' else '',
                                 NOTARY_STATUS='Invalid' if case == 'rejected' else 'Accepted'),
                        capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, case == 'success', result.stderr)
                    self.assertEqual(output.exists(), case == 'success')
                    self.assertEqual(output.with_suffix('.notarization.json').exists(), case == 'success')


if __name__ == '__main__':
    unittest.main()
