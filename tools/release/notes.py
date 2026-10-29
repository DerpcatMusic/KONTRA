#!/usr/bin/env python3
"""Create concise release notes from first-parent merged PRs."""
import argparse
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


def clean(title):
    title = re.sub(r'\s*\(#\d+\)\s*$', '', title)
    title = re.sub(r'^(?:(?:P\d+(?:\.\d+)+[a-z]*|(?:feat|fix|perf|refactor|chore|docs|test|ci|build)(?:\([^)]*\))?)\s*:\s*)+', '', title, flags=re.I)
    title = ' '.join(title.split()).strip(' .')
    return title[:1].upper() + title[1:]


def section(title, paths):
    def internal(path):
        return (path.startswith(('tools/', '.github/', 'docs/', 'tests/'))
                or '/tests/' in path or path.endswith(('_test.rs', '/test.rs', '/tests.rs'))
                or path.endswith('size-baseline.json'))
    paths = [p for p in paths if p]
    if paths and all(internal(p) for p in paths):
        return None
    if re.search(r'\b(perf(?:ormance)?|faster|speed|cache|latency|memory|cpu)\b', title, re.I):
        return 'Performance'
    if any(p.startswith(('crates/app/ui/', 'plugin/src/editor')) for p in paths):
        return 'Interface'
    if any(p.startswith(('crates/format/', 'crates/stream/', 'crates/app/library/')) for p in paths):
        return 'Libraries & loading'
    if any(p.startswith(('crates/dsp/', 'crates/engine/', 'crates/voice/')) for p in paths):
        return 'Sound & playing'
    if re.search(r'\b(ui|interface|knob|label|draw|view)\b', title, re.I):
        return 'Interface'
    if re.search(r'\b(librar(?:y|ies)|load|sample|preset)\b', title, re.I):
        return 'Libraries & loading'
    if re.search(r'\b(fix|correct|resolve|prevent)\b', title, re.I):
        return 'Fixes'
    return 'Sound & playing'


def render(entries):
    groups = {name: [] for name in SECTIONS}
    internal = 0
    for title, number, paths in entries:
        name = section(title, paths)
        if name is None:
            internal += 1
        else:
            groups[name].append(f'- {clean(title)}. ([#{number}]({REPO}/pull/{number}))')
    lines = []
    for name, changes in groups.items():
        if changes:
            lines.extend([f'### {name}', '', *changes, ''])
    if internal:
        lines.extend(['<details>', f'<summary>Internal: {internal} change{"s" if internal != 1 else ""}</summary>', '', '</details>', ''])
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
            entries.append((title, match.group(1), paths))
    print(render(entries))


if __name__ == '__main__':
    main()
