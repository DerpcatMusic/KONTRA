"""Compare script slots by identity and group shared first blockers across libraries."""
import collections
import json
import pathlib
import re
import sys


def slots(path):
    result = {}
    for preset in json.loads(pathlib.Path(path).read_text()):
        if 'error' in preset:
            raise ValueError(preset['error'])
        for program in preset['programs']:
            for slot, script in enumerate(program['scripts']):
                error = script['initialization'].get('error', 'OK')
                result[preset['path'], program['program'], slot] = re.sub(r'^KSP (?:line|case at line) \d+: ', '', error)
    return result


if __name__ == '__main__':
    before, after = map(slots, sys.argv[1:3])
    assert before.keys() == after.keys(), 'Corpus changed; comparisons require identical script slots'
    transitions = collections.Counter((before[k], after[k]) for k in before)
    old, new = collections.Counter(before.values()), collections.Counter(after.values())
    libraries = collections.defaultdict(collections.Counter)
    for (path, _, _), error in after.items():
        library = path.split('/Kontakt/', 1)[-1].split('/')[0]
        libraries[error][library] += 1
    lines = ['# Shared script blockers', '', f'Compared {len(before)} identical script slots. Completed initialization: {old["OK"]} → {new["OK"]}.', '', 'Moving beyond an old error is not a successful initialization or playable instrument. These are first blockers; additional gaps can remain behind them.', '', '| Shared first blocker | Before | After |', '|---|---:|---:|']
    for error in sorted(old.keys() | new.keys(), key=lambda x: (-old[x], x)):
        lines.append(f'| {error} | {old[error]} | {new[error]} |')
    lines += ['', '## Correlated library failures', '', '| Current blocker | Library | Script slots |', '|---|---|---:|']
    for error, count in new.most_common():
        for library, amount in libraries[error].most_common():
            lines.append(f'| {error} | {library} | {amount} |')
    lines += ['', '## Changed first blockers', '', '| Before | After | Script slots |', '|---|---|---:|']
    for (a, b), count in transitions.most_common():
        if a != b:
            lines.append(f'| {a} | {b} | {count} |')
    print('\n'.join(lines))
