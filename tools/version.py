#!/usr/bin/env python3
"""The next release's version, from the last release tag and what merged since.

    tools/version.py next       0.5.0
    tools/version.py previous   v0.4.2
    tools/version.py pending    2

Versions are semver, 0.MINOR.PATCH while KONTRA is pre-1.0. A release that
contains a new feature (a merged pull request titled `feat...`) raises MINOR;
any other release raises PATCH. The first 0.x release is FIRST; the dated
builds before it (0.3.x and 3.0.0 nightlies) are not counted.
"""
import argparse
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
MAJOR = 0
FIRST = '0.4.0'
RELEASE = re.compile(r'^v(\d+)\.(\d+)\.(\d+)$')


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def previous(head='HEAD'):
    """The newest release tag before `head` (`vX.Y.Z`), else None."""
    args = ['describe', '--first-parent', '--tags', '--abbrev=0', '--match', f'v{MAJOR}.*', '--exclude', '*-*']
    found = subprocess.run(['git', '-C', str(ROOT), *args, head + '^'], text=True, capture_output=True)
    return found.stdout.strip() if found.returncode == 0 else None


def bump(tag, subjects):
    """The version after `tag`, given the titles merged since it."""
    match = RELEASE.match(tag or '')
    if not match:
        return FIRST
    major, minor, patch = map(int, match.groups())
    if any(re.match(r'feat(\(|!|:)', s) for s in subjects):
        return f'{major}.{minor + 1}.0'
    return f'{major}.{minor}.{patch + 1}'


def next_version(head='HEAD'):
    tag = previous(head)
    span = f'{tag}..{head}' if tag else head
    return bump(tag, git('log', '--first-parent', '--format=%s', span).splitlines())


def pending(head='HEAD'):
    """Pull requests merged on main since the last release."""
    tag = previous(head)
    span = f'{tag}..{head}' if tag else head
    subjects = git('log', '--first-parent', '--format=%s', span).splitlines()
    return sum(1 for s in subjects if re.search(r'\(#\d+\)$', s))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('command', choices=['next', 'previous', 'pending'])
    command = parser.parse_args().command
    print({'next': next_version, 'previous': lambda: previous() or '', 'pending': pending}[command]())


if __name__ == '__main__':
    main()
