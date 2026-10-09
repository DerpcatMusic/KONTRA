#!/usr/bin/env python3
"""Summarize ui_health JSONL; counts are distinct corpus instruments, not widgets."""
import collections
import json
import sys


def summarize(path):
    coverage = collections.defaultdict(lambda: collections.Counter())
    failures = collections.Counter()
    total = usable = 0
    with open(path) as file:
        for line in file:
            record = json.loads(line)
            total += 1
            per_frontend = collections.defaultdict(list)
            problems = set()
            programs = record['ui']['programs']
            item_ok = bool(programs)
            for program in programs:
                ui = program.get('ui', {})
                item_ok &= ui.get('usable', False)
                if 'error' in program:
                    problems.add('instrument load: ' + program['error'])
                for error in ui.get('errors', []):
                    problems.add(error['feature'] + ': ' + error['error'])
                if not ui.get('authored', True):
                    per_frontend['no_authored_ui'].append({'loaded': True, 'usable': ui.get('usable', False), 'complete': False})
                for expected in ui.get('expected_frontends', []):
                    if not expected['loaded']:
                        per_frontend[expected['frontend']].append({'loaded': False, 'usable': False, 'complete': False})
                for view in ui.get('views', []):
                    if not view['widgets']:
                        continue
                    per_frontend[view['frontend']].append(view)
                    for field in ['missing_resources', 'layout_errors', 'unbound_controls', 'passive_value_changes']:
                        if view[field]:
                            problems.add(field)
                    for feature in view['unsupported_widgets']:
                        problems.add(feature)
                    for prop in view['unsupported_properties']:
                        problems.add(prop['feature'])
                    if any(not render['ok'] for render in view['renders']):
                        problems.add('render error')
            usable += item_ok
            for frontend, views in per_frontend.items():
                coverage[frontend]['total'] += 1
                for field in ['loaded', 'usable', 'complete']:
                    coverage[frontend][field] += all(v[field] for v in views)
            failures.update(problems)
    print(f'{path}: {total} corpus instruments; usable {usable}/{total} ({usable / max(total, 1):.2%})')
    print('| Frontend | Instruments | Loads | Usable | Complete |')
    print('| --- | ---: | ---: | ---: | ---: |')
    for frontend, counts in sorted(coverage.items()):
        n = counts['total']
        print(f'| {frontend} | {n} | ' + ' | '.join(f'{counts[f]}/{n} ({counts[f] / n:.2%})' for f in ['loaded', 'usable', 'complete']) + ' |')
    print('\nFailure classes (instruments affected):')
    for feature, count in failures.most_common():
        print(f'{count:5} {feature}')


if __name__ == '__main__':
    for path in sys.argv[1:]:
        summarize(path)
