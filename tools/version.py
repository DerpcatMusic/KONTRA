#!/usr/bin/env python3
"""Deliberate package SemVer edits and validation. Python 3.11+, standard library only."""
import argparse
import datetime as dt
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?")


def validate(version):
    match = SEMVER.fullmatch(version)
    if not match or any(x.isdigit() and len(x) > 1 and x.startswith('0') for x in (match[4] or '').split('.')):
        raise ValueError(f'Invalid SemVer: {version}')
    return match


def package():
    return tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']


def check():
    pkg = package()
    validate(pkg['version'])
    entries = tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']
    versions = [p['version'] for p in entries if p['name'] == pkg['name'] and 'source' not in p]
    if versions != [pkg['version']]:
        raise ValueError(f'Cargo.lock root version differs: {versions}')
    plugins = tomllib.loads((ROOT / 'moose.toml').read_text()).get('plugin', [])
    if any(p.get('version', pkg['version']) != pkg['version'] for p in plugins):
        raise ValueError('moose.toml overrides the authoritative Cargo package version')
    return pkg['version']


def write(version):
    validate(version)
    pkg = package()
    for filename, header in [('Cargo.toml', '[package]'), ('Cargo.lock', '[[package]]')]:
        path = ROOT / filename
        text = path.read_text()
        pattern = re.compile(r'(?ms)^' + re.escape(header) + r'\n(?P<body>.*?)(?=^\[|\Z)')
        count = 0
        def replace(match):
            nonlocal count
            body = match['body']
            if not re.search(r'^name\s*=\s*"' + re.escape(pkg['name']) + r'"\s*$', body, re.M):
                return match[0]
            count += 1
            body, n = re.subn(r'(?m)^version\s*=\s*"[^"]+"', f'version = "{version}"', body, count=1)
            if n != 1:
                raise ValueError(f'Missing version in {filename}')
            return header + '\n' + body
        changed = pattern.sub(replace, text)
        if count != 1:
            raise ValueError(f'Expected one root package in {filename}, found {count}')
        path.write_text(changed)
    check()


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def nightly(base, revision, epoch):
    base = '.'.join(validate(base).group(1, 2, 3))
    if not re.fullmatch(r'[0-9a-fA-F]{40}|[0-9a-fA-F]{64}', revision):
        raise ValueError('Nightly revision must be a full Git object ID')
    date = dt.datetime.fromtimestamp(epoch, dt.timezone.utc).strftime('%Y%m%d')
    return f'{base}-nightly.{date}.g{revision[:12].lower()}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    p = commands.add_parser('check', help='check package, lockfile, plugin metadata and optional tag')
    p.add_argument('--tag')
    p = commands.add_parser('set', help='choose a release version explicitly')
    p.add_argument('version')
    p.add_argument('--write', action='store_true')
    p = commands.add_parser('nightly', help='derive a reproducible prerelease from this checkout')
    p.add_argument('--write', action='store_true')
    p = commands.add_parser('manifest', help='validate and print the exact build.rs package manifest')
    p.add_argument('--file', required=True, type=Path)
    commands.add_parser('self-test', help='check SemVer edge cases and deterministic nightly identity')
    args = parser.parse_args()
    if args.command == 'self-test':
        for bad in ['01.2.3', '1.2.3-01', '1.2', '1.2.3-']:
            try:
                validate(bad)
            except ValueError:
                continue
            raise AssertionError(f'Accepted invalid version {bad}')
        revision = '000000000001' + 'a' * 28
        actual = nightly('0.2.0', revision, 0)
        assert actual == nightly('0.2.0-nightly.old', revision, 0) == '0.2.0-nightly.19700101.g000000000001'
        validate(actual)
        print('SemVer and deterministic nightly checks passed')
        return
    current = check()
    if args.command == 'check':
        if args.tag and args.tag != f'v{current}':
            raise ValueError(f'Tag must be v{current}')
        print(current)
    elif args.command in ('set', 'nightly'):
        if args.command == 'set':
            version = args.version
        else:
            revision = git('rev-parse', '--verify', 'HEAD')
            epoch = int(os.environ.get('SOURCE_DATE_EPOCH', git('show', '-s', '--format=%ct', 'HEAD')))
            version = nightly(current, revision, epoch)
        validate(version)
        if args.write:
            write(version)
        print(version)
    elif args.command == 'manifest':
        manifest = json.loads(args.file.read_text())
        if manifest['version'] != current:
            raise ValueError('Built manifest version differs from Cargo package')
        for field in ('revision', 'source_revision', 'built_at_utc', 'source_date_epoch', 'target', 'profile', 'features', 'dirty', 'build_hash', 'import_hash'):
            if field not in manifest:
                raise ValueError(f'Manifest missing {field}')
        print(json.dumps(manifest, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
