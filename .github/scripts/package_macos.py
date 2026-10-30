#!/usr/bin/env python3
"""Compose same-revision arm64 and Intel plugin bundles into an installer payload."""
import argparse
import json
import plistlib
from pathlib import Path
import shutil
import subprocess

BUNDLES = {'clap': 'Library/Audio/Plug-Ins/CLAP/KONTRA.clap',
           'vst3': 'Library/Audio/Plug-Ins/VST3/KONTRA.vst3'}


def compose(arm64, x86_64, stage, revision, version):
    if stage.exists():
        raise ValueError('Output stage already exists')
    for arch, root in [('arm64', arm64), ('x86_64', x86_64)]:
        if (root / 'SOURCE_COMMIT.txt').read_text().strip() != revision:
            raise ValueError(f'{arch}: source revision mismatch')
        for fmt in BUNDLES:
            binary = root / f'KONTRA.{fmt}/Contents/MacOS/KONTRA'
            actual = subprocess.check_output(['lipo', '-archs', str(binary)], text=True).split()
            if actual != [arch]:
                raise ValueError(f'{arch}/{fmt}: wrong architecture: {actual}')
    payload = stage / 'payload'
    for fmt, destination in BUNDLES.items():
        bundle = payload / destination
        shutil.copytree(arm64 / f'KONTRA.{fmt}', bundle)
        plist = bundle / 'Contents/Info.plist'
        info = plistlib.loads(plist.read_bytes())
        intel = plistlib.loads((x86_64 / f'KONTRA.{fmt}/Contents/Info.plist').read_bytes())
        for key in ('CFBundleExecutable', 'CFBundleIdentifier'):
            if intel.get(key) != info.get(key):
                raise ValueError(f'{fmt}: {key} differs between architectures')
        if info.get('CFBundleExecutable') != 'KONTRA' or not info.get('CFBundleIdentifier'):
            raise ValueError(f'{fmt}: invalid bundle identity')
        info['CFBundleVersion'] = info['CFBundleShortVersionString'] = version.split('-', 1)[0]
        info['KONTRAVersion'] = version
        plist.write_bytes(plistlib.dumps(info, sort_keys=False))
        binary = bundle / 'Contents/MacOS/KONTRA'
        subprocess.run(['lipo', '-create', str(arm64 / f'KONTRA.{fmt}/Contents/MacOS/KONTRA'),
                        str(x86_64 / f'KONTRA.{fmt}/Contents/MacOS/KONTRA'),
                        '-output', str(binary)], check=True)
        subprocess.run(['lipo', str(binary), '-verify_arch', 'arm64', 'x86_64'], check=True)
        binary.chmod(0o755)
    (stage / 'build-info.json').write_text(json.dumps(dict(version=version, revision=revision,
        target='universal-apple-darwin', architectures=['arm64', 'x86_64'], profile='release')) + '\n')
    for path in [payload, *payload.rglob('*')]:
        path.chmod(0o755 if path.is_dir() or path.name == 'KONTRA' else 0o644)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--arm64', type=Path, required=True)
    parser.add_argument('--x86-64', type=Path, required=True)
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--version', required=True)
    args = parser.parse_args()
    compose(args.arm64, args.x86_64, args.stage, args.revision, args.version)
