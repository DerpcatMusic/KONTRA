#!/usr/bin/env python3
"""Publish the numbers behind the website's Progress page.

    tools/progress.py sound SCORE.json    the sound scoreboard (tools/oracle/scoreboard.py score)

It rewrites that section of website/progress.json in plain words. The site is
rebuilt from the file when it reaches main, so the page shows what was last
measured, with its date and source revision.
"""
import argparse
from datetime import date
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / 'website/progress.json'
LIBRARIES = dict(
    analog='Analog Strings', trumpets='Afflatus Brass: 3 Trumpets Legato', horn='Afflatus Brass: Solo French Horn Legato',
    pacific='Pacific Ensemble Strings: 10 Cellos Legato', chorus='Chorus: Women', areia='Areia: 16 Violins',
    dolce='Dolce: 7 1st Violins Legato', unacorda='Una Corda: Felt')
REGISTER = {'48': 'Low', '60': 'Middle', '72': 'High'}
TOUCH = {'40': 'soft', '90': 'medium', '127': 'hard'}
PLAYS = dict(chord='Five-note chord', repeats='Fast repeated notes', modsweep='Held note with a mod wheel sweep',
             pedal='Phrase with the sustain pedal')


def play(case):
    note = re.fullmatch(r'k(\d+)v(\d+)', case)
    return f'{REGISTER[note[1]]} note, {TOUCH[note[2]]}' if note else PLAYS[case]


def difference(fail):
    """One scoreboard failure in words. Its numbers are Kontakt minus KONTRA."""
    kind, value = fail.split(' ', 1)
    if kind == 'band':
        hz, db = value.split('Hz ')
        return f'{abs(float(db)):.1f} dB {"quieter" if float(db) > 0 else "louder"} around {hz} Hz'
    if kind == 'onset':
        ms = float(value.removesuffix('ms'))
        return f'starts {abs(ms):.0f} ms {"early" if ms > 0 else "late"}'
    db = float(value)
    what = dict(level='overall', attack='in the first 60 ms', release='in the release tail')
    if kind == 'balance':
        return f'{abs(db):.1f} dB off to the {"left" if db > 0 else "right"}'
    return f'{abs(db):.1f} dB {"quieter" if db > 0 else "louder"} {what[kind]}'


def sound(table, revision, measured):
    libraries = []
    for key, rows in table.items():
        misses = []
        for row in rows:
            if row['ok']:
                continue
            silent = row.get('silent')
            why = ('KONTRA is silent' if silent == 'v3' else 'KONTRA plays where Kontakt is silent' if silent
                   else ', '.join(difference(f) for f in row['fails']))
            misses.append(f'{play(row["case"])}: {why}')
        libraries.append(dict(name=LIBRARIES.get(key, key), passed=sum(r['ok'] for r in rows), total=len(rows), misses=misses))
    return dict(measured=measured, revision=revision, libraries=libraries)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('section', choices=['sound'])
    parser.add_argument('source', type=Path)
    parser.add_argument('--revision', help='the commit that was measured (default: HEAD)')
    args = parser.parse_args()
    revision = args.revision or subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip()
    data = json.loads(DATA.read_text()) if DATA.exists() else {}
    data[args.section] = sound(json.loads(args.source.read_text()), revision, date.today().isoformat())
    DATA.write_text(json.dumps(data, indent=1, ensure_ascii=False) + '\n')
    done = sum(l['passed'] for l in data['sound']['libraries'])
    print(f'sound: {done}/{sum(l["total"] for l in data["sound"]["libraries"])} written to {DATA}')


if __name__ == '__main__':
    main()
