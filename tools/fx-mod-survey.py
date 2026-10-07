#!/usr/bin/env python3
"""Merge metadata-only per-item fx_mod_survey caches; never opens library files."""
import collections
import pathlib
import sys


def merge(directory):
    counts = collections.defaultdict(lambda: [0, set()])
    fields = collections.defaultdict(lambda: collections.defaultdict(lambda: [0, set()]))
    for path in sorted(pathlib.Path(directory).glob('*.tsv')):
        for line in path.read_text().splitlines():
            kind, occurrences, _, *parts = line.split('\t')
            if kind == 'COUNT':
                item = counts['\t'.join(parts)]
            elif kind == 'VALUE':
                key = '\t'.join(parts[:3])
                item = fields[key]['\t'.join(parts[3:])]
            else:
                raise ValueError(f'Unknown metadata row in {path.name}')
            item[0] += int(occurrences)
            item[1].add(path.stem)
    rows = []
    for key, (count, files) in sorted(counts.items()):
        rows.append(f'COUNT\t{count}\t{len(files)}\t{key}')
    for key, values in sorted(fields.items()):
        baseline = max(values, key=lambda value: values[value][0])
        files = set().union(*(v[1] for v in values.values()))
        varied = set().union(*(v[1] for k, v in values.items() if k != baseline))
        rows.append(f'FIELD\t{key}\t{len(files)}\t{len(varied)}\t{len(values)}\t{baseline}')
    return '\n'.join(rows) + '\n'


def check():
    import tempfile
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory)
        (root / '0000.tsv').write_text('COUNT\t3\t1\tfx\tid=0x10\nVALUE\t3\t1\t0x10\t0x50\trate\t1\n')
        (root / '0001.tsv').write_text('COUNT\t2\t1\tfx\tid=0x10\nVALUE\t2\t1\t0x10\t0x50\trate\t2\n')
        assert merge(root) == 'COUNT\t5\t2\tfx\tid=0x10\nFIELD\t0x10\t0x50\trate\t2\t1\t2\t1\n'


if __name__ == '__main__':
    if sys.argv[1:] == ['--check']:
        check()
    else:
        print(merge(sys.argv[1]), end='')
