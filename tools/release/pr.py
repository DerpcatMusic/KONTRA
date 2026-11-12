#!/usr/bin/env python3
"""Check that a pull request reads well where people will meet it.

Its title becomes the commit on main and a line in the release, so it says
what changed in plain words. Its description becomes the commit's body, and
its `Release note:` line becomes the release notes.

    PR_TITLE=... PR_BODY=... tools/release/pr.py
"""
import os
import re
import sys

TYPES = ('feat', 'fix', 'perf', 'refactor', 'docs', 'test', 'build', 'ci', 'chore', 'revert')
TITLE = re.compile(rf'^({"|".join(TYPES)})(\([a-z0-9][a-z0-9,/-]*\))?(!)?: (\S.*)$')
LIMIT = 72
# Words only the people who did the work understand.
INTERNAL = (
    (re.compile(r'\b0x[0-9a-fA-F]{3,}\b'), 'a memory address'),
    (re.compile(r'\bFUN_[0-9a-fA-F]+\b'), 'a decompiler function name'),
    (re.compile(r'\b[Ll]ane [A-Z]\b|\b[Rr]ound \d+\b|\bP\d+\.\d+[a-z]?\b'), 'an internal plan or lane name'),
)
AREAS = ('sound', 'interface', 'libraries', 'performance', 'fix')


def jargon(text):
    return [what for pattern, what in INTERNAL if pattern.search(text)]


def problems(title, body):
    found = []
    match = TITLE.match(title)
    if not match:
        found.append(f'Title must read `type(scope): what changed`, type one of {", ".join(TYPES)}. '
                     'Example: `fix(ui): long preset menus scroll`.')
    else:
        if match.group(4).endswith('.'):
            found.append('Title ends without a full stop.')
    if len(title) > LIMIT:
        found.append(f'Title is {len(title)} characters; keep it within {LIMIT}. Details go in the description.')
    found += [f'Title contains {what}; say what changed for someone using KONTRA.' for what in jargon(title)]
    note = re.search(r'^Release note:[ \t]*(.*?)[ \t]*$', body, re.I | re.M)
    if not note:
        found.append('Description needs a `Release note:` line: one plain sentence for musicians, or `none`.')
    elif note.group(1).lower() != 'none':
        text = note.group(1)
        if not text:
            found.append('`Release note:` is empty; write one sentence, or `none`.')
        if len(text) > 200:
            found.append(f'Release note is {len(text)} characters; one sentence within 200.')
        found += [f'Release note contains {what}; musicians read it.' for what in jargon(text)]
        if '`' in text:
            found.append('Release note contains code formatting; say it in plain words.')
        area = re.search(r'^Area:[ \t]*(.*?)[ \t]*$', body, re.I | re.M)
        if not area or area.group(1).strip().lower() not in AREAS:
            found.append(f'A release note needs an `Area:` line, one of {", ".join(AREAS)}.')
    return found


def main():
    found = problems(os.environ.get('PR_TITLE', '').strip(), os.environ.get('PR_BODY', ''))
    for problem in found:
        print(f'::error::{problem}')
    if found:
        print('\nSee CONTRIBUTING.md, "Pull requests, commits and releases".')
    sys.exit(1 if found else 0)


if __name__ == '__main__':
    main()
