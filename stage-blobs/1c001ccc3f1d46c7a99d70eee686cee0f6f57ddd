#!/usr/bin/env python3
"""Synthetic composition gate: real Mach-O/lipo, mocked Apple-only tools.

This verifies composition and fail-closed orchestration, not Apple acceptance.
Run on Linux with llvm-lipo or on macOS with lipo; no Cargo or credentials.
"""
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('package_macos', HERE / 'package_macos.py')
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
LIPO = shutil.which('lipo') or shutil.which('llvm-lipo')
REVISION, VERSION = 'a' * 40, '0.3.87-nightly.test'


def fixtures(root):
    sources = {}
    for arch, target in package.TARGETS.items():
        stage = root / arch
        sources[arch] = stage
        stage.mkdir()
        (stage / 'SOURCE_COMMIT.txt').write_text(REVISION + '\n')
        for name in package.REQUIRED + ('licenses/NOTICE.txt', 'licenses/sources/patched.crate'):
            path = stage / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('synthetic legal resource: ' + name)
        for fmt in package.FORMATS:
            info = dict(version=VERSION, revision=REVISION, target=target, profile='release', dirty=False,
                        source_revision='b' * 40, build_hash='c' * 16, import_hash='d' * 16,
                        features=['library-access', 'plugin', fmt])
            (stage / package.MANIFESTS[fmt]).write_text(json.dumps(info))
            path = stage / package.BINARIES[fmt]
            path.parent.mkdir(parents=True, exist_ok=True)
            # Minimal valid Mach-O object header with architecture-specific data.
            cpu = 0x100000c if arch == 'arm64' else 0x1000007
            subtype = 0 if arch == 'arm64' else 3
            path.write_bytes(struct.pack('<8I', 0xfeedfacf, cpu, subtype, 1, 0, 0, 0, 0) + arch.encode() + fmt.encode() + json.dumps(info).encode())
            if fmt == 'standalone':
                (stage / 'KONTRA2.app/Contents/Info.plist').write_bytes(plistlib.dumps(package.app_info(VERSION)))
            else:
                (stage / f'KONTRA2.{fmt}/Contents/Info.plist').write_bytes(plistlib.dumps(dict(
                    CFBundleExecutable='KONTRA2', CFBundleIdentifier=f'audio.matari.kontra2.{fmt}',
                    CFBundlePackageType='BNDL',
                    CFBundleVersion='0.3.87', CFBundleShortVersionString='0.3.87', KONTRAVersion=VERSION)))
                resource = stage / f'KONTRA2.{fmt}/Contents/Resources/native-resource.txt'
                resource.parent.mkdir()
                resource.write_text('unchanged bundle resource')
    return sources


@unittest.skipUnless(LIPO, 'lipo or llvm-lipo is required')
class Packaging(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='kontra-universal-gate-')
        self.root = Path(self.tmp.name)
        self.sources = fixtures(self.root)
        self.stage = self.root / 'universal'

    def tearDown(self):
        self.tmp.cleanup()

    def compose(self):
        return package.compose(self.sources['arm64'], self.sources['x86_64'], self.stage, REVISION, VERSION, LIPO)

    def test_real_lipo_preserves_each_slice_and_install_layout(self):
        previous = os.umask(0o077)
        try:
            identity = self.compose()
        finally:
            os.umask(previous)
        self.assertEqual(identity['architectures'], ['arm64', 'x86_64'])
        self.assertEqual(identity['target'], 'universal-apple-darwin')
        for fmt, destination in package.BUNDLES.items():
            binary = self.stage / 'payload' / destination / 'Contents/MacOS/KONTRA2'
            self.assertEqual(set(subprocess.check_output([LIPO, '-archs', str(binary)], text=True).split()), {'arm64', 'x86_64'})
            for arch in package.TARGETS:
                thin = self.root / f'{fmt}-{arch}'
                subprocess.run([LIPO, str(binary), '-thin', arch, '-output', str(thin)], check=True)
                self.assertEqual(thin.read_bytes(), (self.sources[arch] / package.BINARIES[fmt]).read_bytes())
        resources = self.stage / 'payload/Applications/KONTRA2.app/Contents/Resources/KONTRA'
        self.assertEqual(resources.stat().st_mode & 0o777, 0o755)
        for name in package.REQUIRED + ('licenses/sources/patched.crate',):
            self.assertEqual((resources / name).stat().st_mode & 0o777, 0o644)
            self.assertEqual((resources / name).read_bytes(), (self.sources['arm64'] / name).read_bytes())
        for arch in package.TARGETS:
            for name in package.MANIFESTS.values():
                self.assertEqual((resources / 'source-builds' / arch / name).read_bytes(), (self.sources[arch] / name).read_bytes())
        plist = plistlib.loads((self.stage / 'payload/Applications/KONTRA2.app/Contents/Info.plist').read_bytes())
        self.assertEqual(plist, package.app_info(VERSION))
        self.assertFalse((resources / 'KONTRA2.app').exists())

    def test_mismatched_identity_or_resources_never_produces_payload(self):
        for case in ('revision', 'version', 'profile', 'features', 'resources', 'plist', 'app-types', 'arch', 'binary', 'legal', 'symlink'):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                sources = fixtures(root)
                arm = sources['arm64']
                if case in ('revision', 'version', 'profile', 'features'):
                    path = arm / 'clap-build-info.json'
                    info = json.loads(path.read_text())
                    info[case] = [] if case == 'features' else 'wrong'
                    path.write_text(json.dumps(info))
                elif case == 'resources':
                    (arm / 'NOTICE').write_text('different resource')
                elif case == 'plist':
                    path = arm / 'KONTRA2.vst3/Contents/Info.plist'
                    info = plistlib.loads(path.read_bytes()); info['CFBundleIdentifier'] = 'wrong.identity'
                    path.write_bytes(plistlib.dumps(info))
                elif case == 'app-types':
                    path = arm / 'KONTRA2.app/Contents/Info.plist'
                    info = plistlib.loads(path.read_bytes()); info.pop('UTImportedTypeDeclarations')
                    path.write_bytes(plistlib.dumps(info))
                elif case == 'arch':
                    shutil.copyfile(sources['x86_64'] / package.BINARIES['clap'], arm / package.BINARIES['clap'])
                elif case == 'binary':
                    path = arm / package.BINARIES['clap']
                    path.write_bytes(path.read_bytes().replace(REVISION.encode(), b'e' * 40))
                elif case == 'legal':
                    (arm / 'assets/OFL.txt').unlink()
                elif case == 'symlink':
                    (arm / 'alias').symlink_to('/etc/passwd')
                output = root / 'universal'
                with self.assertRaises(ValueError):
                    package.compose(arm, sources['x86_64'], output, REVISION, VERSION, LIPO)
                self.assertFalse(output.exists())

    def test_signing_and_notary_failures_never_publish(self):
        self.compose()
        tools = self.root / 'tools'; tools.mkdir()
        mock = tools / 'mock'
        mock.write_text('''#!/usr/bin/env python3
import json, os, pathlib, plistlib, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ['MOCK_LOG'], 'a') as log: log.write(json.dumps([name, args]) + '\\n')
if name == os.environ.get('FAIL_TOOL') or (name == 'xcrun' and (args[:2] == ['stapler', os.environ.get('FAIL_TOOL')] or args[:1] == [os.environ.get('FAIL_TOOL')])): sys.exit(1)
if name == 'codesign' and '--force' in args and pathlib.Path(args[-1]).is_dir():
    signature = pathlib.Path(args[-1]) / 'Contents/_CodeSignature'
    signature.mkdir(exist_ok=True)
    signature.chmod(0o700)
    resource = signature / 'CodeResources'
    resource.write_text('synthetic signature resource')
    resource.chmod(0o600)
if name == 'xcrun' and args[:1] == ['SetFile']: raise AssertionError('FinderInfo invalidates strict signatures')
if name == 'xcrun' and args[:1] == ['swift']:
    assert args[2] == '--register'
    app = pathlib.Path(args[3])
    info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
    assert info['CFBundlePackageType'] == 'APPL'
    assert len(info['UTImportedTypeDeclarations']) == 2
    assert all('com.apple.package' in t['UTTypeConformsTo'] for t in info['UTImportedTypeDeclarations'])
if name == 'pkgbuild':
    payload = pathlib.Path(args[args.index('--root')+1])
    signatures = list(payload.rglob('_CodeSignature'))
    assert len(signatures) == 3
    for signature in signatures:
        assert signature.stat().st_mode & 0o777 == 0o755
        assert (signature / 'CodeResources').stat().st_mode & 0o777 == 0o644
    if '--analyze' in args:
        pathlib.Path(args[-1]).write_bytes(plistlib.dumps([{'BundleIsRelocatable': True, 'BundleIsVersionChecked': True}]))
    else:
        components = plistlib.loads(pathlib.Path(args[args.index('--component-plist')+1]).read_bytes())
        assert components[0]['BundleIsRelocatable'] is False
        assert components[0]['BundleIsVersionChecked'] is False
        assert components[0]['BundleOverwriteAction'] == 'upgrade'
        assert args[args.index('--install-location')+1] == '/'
        scripts = pathlib.Path(args[args.index('--scripts')+1])
        assert (scripts / 'postinstall').stat().st_mode & 0o111
        postinstall = (scripts / 'postinstall').read_text()
        assert 'launchctl asuser' in postinstall and 'sudo -u' in postinstall and 'lsregister' in postinstall
        assert 'xattr -' not in postinstall and 'SetFile' not in postinstall
        pathlib.Path(args[-1]).write_bytes(b'synthetic installer payload')
if name == 'productsign': pathlib.Path(args[-1]).write_bytes(pathlib.Path(args[-2]).read_bytes())
if name == 'xcrun' and args[:2] == ['notarytool', 'submit']:
    print(json.dumps({'status': os.environ.get('NOTARY_STATUS', 'Accepted'), 'id': '00000000-0000-4000-8000-000000000000'}))
''')
        mock.chmod(0o755)
        for name in ('codesign', 'pkgbuild', 'productsign', 'pkgutil', 'xcrun', 'spctl'):
            (tools / name).symlink_to(mock)
        (tools / 'lipo').symlink_to(LIPO)
        env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ['PATH'],
                   APPLE_SIGNING_KEYCHAIN='synthetic.keychain', APPLE_TEAM_ID='TESTTEAM',
                   APPLE_DEVELOPER_ID_APPLICATION='Developer ID Application: Synthetic (TESTTEAM)',
                   APPLE_DEVELOPER_ID_INSTALLER='Developer ID Installer: Synthetic (TESTTEAM)',
                   APPLE_ID='synthetic@example.invalid', APPLE_APP_SPECIFIC_PASSWORD='synthetic',
                   MOCK_LOG=str(self.root / 'calls.jsonl'))
        cases = [('valid', '', 'Accepted'), ('codesign', 'codesign', 'Accepted'),
                 ('package-registration', 'swift', 'Accepted'),
                 ('productsign', 'productsign', 'Accepted'), ('pkgutil', 'pkgutil', 'Accepted'),
                 ('rejected', '', 'Invalid'), ('staple', 'staple', 'Accepted'), ('validate', 'validate', 'Accepted'), ('assessment', 'spctl', 'Accepted')]
        for case, tool, status in cases:
            with self.subTest(case=case):
                output = self.root / f'{case}.pkg'
                result = subprocess.run(['bash', str(HERE / 'package_macos.sh'), str(self.stage), str(output)],
                                        env=dict(env, FAIL_TOOL=tool, NOTARY_STATUS=status), capture_output=True)
                self.assertEqual(result.returncode == 0, case == 'valid', result.stderr.decode())
                self.assertEqual(output.exists(), case == 'valid')
                self.assertEqual(output.with_suffix('.notarization.json').exists(), case == 'valid')
                if case == 'valid':
                    receipt = json.loads(output.with_suffix('.notarization.json').read_text())
                    self.assertEqual(receipt['source_builds']['arm64']['vst3']['target'], 'aarch64-apple-darwin')
                    self.assertEqual(len(receipt['products']), 3)
                    self.assertEqual(receipt['status'], 'Accepted')


if __name__ == '__main__':
    if not LIPO:
        raise SystemExit('lipo or llvm-lipo is required for the composition gate')
    unittest.main()
