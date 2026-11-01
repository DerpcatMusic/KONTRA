#!/usr/bin/env python3
"""Create concise release notes from first-parent merged PRs."""
import argparse
import json
import re
import subprocess

REPO = 'https://github.com/DerpcatMusic/KONTRA'
SECTIONS = ('Sound & playing', 'Interface', 'Libraries & loading', 'Performance', 'Fixes')


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def previous(head):
    result = subprocess.run(['git', 'describe', '--first-parent', '--tags', '--abbrev=0',
                             '--match', 'v3.*', head + '^'], text=True, capture_output=True)
    if result.returncode == 0:
        return result.stdout.strip()
    for candidate in ('v2-final', '7a8e28298'):
        if subprocess.run(['git', 'merge-base', '--is-ancestor', candidate, head],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
            return candidate
    raise ValueError('No previous v3 tag or v2 baseline is an ancestor')


AREAS = dict(sound='Sound & playing', interface='Interface', libraries='Libraries & loading',
             performance='Performance', fix='Fixes')


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
                or path.endswith(('_test.rs', '/test.rs', '/tests.rs', 'size-baseline.json')))
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


def pull_body(number):
    data = subprocess.check_output(['gh', 'api', f'repos/DerpcatMusic/KONTRA/pulls/{number}'], text=True)
    return json.loads(data).get('body') or ''


def render(entries):
    groups = {name: [] for name in SECTIONS}
    internal = []
    for body, number, paths in entries:
        note, explicit_area = fields(body)
        name = explicit_area or section(paths)
        if not note or not name:
            internal.append(f'- [#{number}]({REPO}/pull/{number})')
        else:
            note = note.rstrip()
            if note[-1] not in '.!?':
                note += '.'
            groups[name].append(f'- {note} ([#{number}]({REPO}/pull/{number}))')
    lines = []
    for name, changes in groups.items():
        if changes:
            lines.extend([f'### {name}', '', *changes, ''])
    if internal:
        count = len(internal)
        lines.extend(['<details>', f'<summary>Internal: {count} change{"s" if count != 1 else ""}</summary>', '', *internal, '', '</details>', ''])
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', help='override the previous release baseline')
    parser.add_argument('--head', default='HEAD')
    args = parser.parse_args()
    base = args.base or previous(args.head)
    log = git('log', '--first-parent', '--format=%H%x09%s', f'{base}..{args.head}')
    entries = []
    for line in log.splitlines():
        sha, title = line.split('\t', 1)
        match = re.search(r'\(#(\d+)\)\s*$', title)
        if match:
            paths = git('diff-tree', '--root', '--first-parent', '-m', '--no-commit-id', '--name-only', '-r', sha).splitlines()
            entries.append((pull_body(match.group(1)), match.group(1), paths))
    print(render(entries))


if __name__ == '__main__':
    main()
