#!/usr/bin/env python3
"""Release notes from the pull requests merged since the last release.

Each pull request says what changed for someone using KONTRA in its
`Release note:` line and where in its `Area:` line. Those sentences are the
notes, each credited to its pull request and author. Pull requests whose note
is `none` are listed by title under "Under the hood".
"""
import argparse
import json
import re
import subprocess

REPO = 'https://github.com/DerpcatMusic/KONTRA'
SECTIONS = ('Sound & playing', 'Interface', 'Libraries & loading', 'Performance', 'Fixes')
AREAS = dict(sound='Sound & playing', interface='Interface', libraries='Libraries & loading',
             performance='Performance', fix='Fixes')


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def previous(head):
    result = subprocess.run(['git', 'describe', '--first-parent', '--tags', '--abbrev=0',
                             '--match', 'v0.*', '--exclude', '*-*', head + '^'], text=True, capture_output=True)
    if result.returncode == 0:
        return result.stdout.strip()
    for candidate in ('v2-final', '7a8e28298'):
        if subprocess.run(['git', 'merge-base', '--is-ancestor', candidate, head],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
            return candidate
    raise ValueError('No previous v3 tag or v2 baseline is an ancestor')


def fields(body):
    note = re.search(r'^Release note:[ \t]*(.*?)[ \t]*$', body or '', re.I | re.M)
    area = re.search(r'^Area:[ \t]*(.*?)[ \t]*$', body or '', re.I | re.M)
    text = note.group(1).strip() if note else ''
    if text.lower() == 'none':
        text = ''
    name = AREAS.get(area.group(1).strip().lower()) if area else None
    return text, name


def section(paths):
    def internal(path):
        return (path.startswith(('tools/', '.github/', 'docs/', 'tests/'))
                or '/tests/' in path or '/playtest' in path or '/survey' in path
                or path.endswith(('_test.rs', '/test.rs', '/tests.rs')))
    paths = [p for p in paths if p and not internal(p)]
    if not paths:
        return None
    if any(p.startswith(('crates/app/ui/', 'plugin/src/editor')) for p in paths):
        return 'Interface'
    if any(p.startswith(('crates/format/', 'crates/engine/stream/', 'crates/app/library/')) for p in paths):
        return 'Libraries & loading'
    if any(p.startswith(('crates/dsp/', 'crates/engine/', 'crates/script/', 'crates/model/', 'crates/app/host/')) for p in paths):
        return 'Sound & playing'
    return None


def pull(number):
    """One merged pull request: its title, description and author."""
    data = json.loads(subprocess.check_output(['gh', 'api', f'repos/DerpcatMusic/KONTRA/pulls/{number}'], text=True))
    return dict(number=str(number), title=data.get('title') or '', body=data.get('body') or '',
                author=(data.get('user') or {}).get('login') or '')


def credit(entry):
    link = f'[#{entry["number"]}]({REPO}/pull/{entry["number"]})'
    return f'{link} by @{entry["author"]}' if entry.get('author') else link


def render(entries, version=None, base=None):
    """`entries`: pull requests, newest first, each with `paths` it changed."""
    groups = {name: [] for name in SECTIONS}
    internal = []
    for entry in entries:
        note, explicit_area = fields(entry['body'])
        name = explicit_area or section(entry.get('paths', ()))
        if not note or not name:
            title = re.sub(r'\s*\(#\d+\)\s*$', '', entry.get('title') or '') or 'Untitled change'
            internal.append(f'- {title} ({credit(entry)})')
        else:
            note = note.rstrip()
            if note[-1] not in '.!?':
                note += '.'
            groups[name].append(f'- {note} ({credit(entry)})')
    lines = []
    shown = sum(len(changes) for changes in groups.values())
    if version:
        count = len(entries)
        since = (f' since [{base.lstrip("v")}]({REPO}/releases/tag/{base})' if base and base.startswith('v0.')
                 else ' since the current engine began')
        lines += [f'{count} change{"s" if count != 1 else ""}{since}, {shown} of them '
                  'something you can hear, see or measure. KONTRA is alpha software: '
                  'expect rough edges, and [tell us](' + REPO + '/issues/new/choose) what breaks.', '']
    for name, changes in groups.items():
        if changes:
            lines.extend([f'### {name}', '', *changes, ''])
    if internal:
        count = len(internal)
        lines.extend(['<details>', f'<summary>Under the hood: {count} change{"s" if count != 1 else ""}</summary>',
                      '', *internal, '', '</details>', ''])
    if version:
        lines += ['### Install', '',
                  '- **Windows and Linux:** download the ZIP below, extract it and run the install script inside.',
                  '- **macOS:** run the universal `.pkg` installer.',
                  '', 'KONTRA loads as a CLAP or VST3 plugin; rescan plugins in your DAW after installing.', '']
        lines += ['Every pull request and commit since the project began: **CHANGELOG.md** below.', '']
        if base:
            lines += [f'**Full changelog:** [{base}...v{version}]({REPO}/compare/{base}...v{version})', '']
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', help='override the previous release baseline')
    parser.add_argument('--head', default='HEAD')
    parser.add_argument('--version', help='the version being released, for the summary and compare link')
    args = parser.parse_args()
    base = args.base or previous(args.head)
    log = git('log', '--first-parent', '--format=%H%x09%s', f'{base}..{args.head}')
    entries = []
    for line in log.splitlines():
        sha, title = line.split('\t', 1)
        match = re.search(r'\(#(\d+)\)\s*$', title)
        if match:
            paths = git('diff-tree', '--root', '--first-parent', '-m', '--no-commit-id', '--name-only', '-r', sha).splitlines()
            entries.append(dict(pull(match.group(1)), paths=paths))
    print(render(entries, args.version, base))


if __name__ == '__main__':
    main()
