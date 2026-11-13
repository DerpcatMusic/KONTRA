#!/usr/bin/env python3
"""The whole history as one file: every merged pull request, then every commit.

    tools/release/changelog.py > CHANGELOG.md

Pull requests come newest first, by month, with their release note when they
have one. Commits follow, by month, so work that never went through a pull
request is listed too. Every release carries this file.
"""
import json
import re
import subprocess
from collections import defaultdict

REPO = 'https://github.com/DerpcatMusic/KONTRA'


def note(body):
    found = re.search(r'^Release note:[ \t]*(.*?)[ \t]*$', body or '', re.I | re.M)
    text = found.group(1).strip() if found else ''
    return '' if text.lower() == 'none' else text


def render(pulls, commits):
    """`pulls`: dicts with number, title, mergedAt, body. `commits`: (sha, date, subject)."""
    lines = ['# KONTRA changelog', '',
             f'Every pull request ({len(pulls)}) and every commit ({len(commits)}) since the project began, newest first.', '',
             '## Pull requests', '']
    by_month = defaultdict(list)
    for p in sorted(pulls, key=lambda p: p['mergedAt'], reverse=True):
        by_month[p['mergedAt'][:7]].append(p)
    for month, items in by_month.items():
        lines += [f'### {month}', '']
        for p in items:
            title = re.sub(r'\s*\(#\d+\)\s*$', '', p['title'])
            extra = note(p.get('body'))
            lines.append(f'- {title} ([#{p["number"]}]({REPO}/pull/{p["number"]}))' + (f': {extra}' if extra else ''))
        lines.append('')
    lines += ['## Commits', '']
    by_month = defaultdict(list)
    for sha, date, subject in commits:
        by_month[date[:7]].append((sha, subject))
    for month, items in by_month.items():
        lines += [f'### {month}', '']
        lines += [f'- {subject} ([{sha[:9]}]({REPO}/commit/{sha}))' for sha, subject in items]
        lines.append('')
    return '\n'.join(lines)


def main():
    pulls = json.loads(subprocess.check_output(
        ['gh', 'pr', 'list', '--repo', 'DerpcatMusic/KONTRA', '--state', 'merged', '--limit', '100000',
         '--json', 'number,title,mergedAt,body'], text=True))
    log = subprocess.check_output(['git', 'log', '--format=%H%x09%cI%x09%s', 'HEAD'], text=True)
    commits = [tuple(line.split('\t', 2)) for line in log.splitlines() if line.count('\t') >= 2]
    print(render(pulls, commits))


if __name__ == '__main__':
    main()
