#!/usr/bin/env python3
"""The next release's version, from the last release tag and what merged since.

    tools/version.py next       3.1.0
    tools/version.py previous   v3.0.4

Versions are MAJOR.MINOR.PATCH. A release that contains a new feature (a merged
pull request titled `feat...`) raises MINOR; any other release raises PATCH.
MAJOR is the engine generation and changes by hand, in `MAJOR` below.
"""
import argparse
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
MAJOR = 3
RELEASE = re.compile(r'^v(\d+)\.(\d+)\.(\d+)$')


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def previous(head='HEAD'):
    """The newest release tag before `head` (`vX.Y.Z`), else the last dated
    build of this generation, else None."""
    for pattern, exclude in ((f'v{MAJOR}.*', '*-*'), (f'v{MAJOR}.*', None)):
        args = ['describe', '--first-parent', '--tags', '--abbrev=0', '--match', pattern]
        if exclude:
            args += ['--exclude', exclude]
        found = subprocess.run(['git', '-C', str(ROOT), *args, head + '^'], text=True, capture_output=True)
        if found.returncode == 0:
            return found.stdout.strip()
    return None


def bump(tag, subjects):
    """The version after `tag`, given the titles merged since it."""
    match = RELEASE.match(tag or '')
    major, minor, patch = map(int, match.groups()) if match else (MAJOR, 0, 0)
    if any(re.match(r'feat(\(|!|:)', s) for s in subjects):
        return f'{major}.{minor + 1}.0'
    return f'{major}.{minor}.{patch + 1}'


def next_version(head='HEAD'):
    tag = previous(head)
    span = f'{tag}..{head}' if tag else head
    return bump(tag, git('log', '--first-parent', '--format=%s', span).splitlines())


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('command', choices=['next', 'previous'])
    command = parser.parse_args().command
    print(next_version() if command == 'next' else previous() or '')


if __name__ == '__main__':
    main()
