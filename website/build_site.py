#!/usr/bin/env python3
"""Build the public site from the checked-out revision, without private catalog caches."""
import argparse
from datetime import datetime
from html import escape
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

SITE = Path(__file__).resolve().parent
ROOT = SITE.parent

REPO = 'https://github.com/DerpcatMusic/KONTRA'


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def render_sound(sound):
    """The sound scoreboard: the same notes played in Kontakt and in KONTRA, per library."""
    libraries = sound['libraries']
    passed, total = sum(l['passed'] for l in libraries), sum(l['total'] for l in libraries)
    measured = datetime.fromisoformat(sound['measured']).strftime('%d %b %Y').lstrip('0')
    revision = sound.get('revision')
    at = f' at <a href="{REPO}/commit/{escape(revision, quote=True)}">{escape(revision[:7])}</a>' if revision else ''
    rows = ''
    for l in libraries:
        name, score = escape(l['name']), f'{l["passed"]} of {l["total"]}'
        misses = ''.join(f'<li>{escape(m)}</li>' for m in l['misses'])
        rows += (f'<div class="sampler-row"><h3>{name}<meter class="meter" min="0" max="{l["total"]}" value="{l["passed"]}">{score}</meter></h3>'
                 f'<span class="score {"status-done" if l["passed"] == l["total"] else "status-partial"}">{score} match</span></div>')
        if misses:
            rows += f'<details class="sampler-scope"><summary>What differs in {name}</summary><ul>{misses}</ul></details>'
    return f'''<section class="sampler-phase"><h2>Sound against Kontakt</h2><div>
<p class="progress-total"><strong>{passed} of {total}</strong> test plays sound the same in KONTRA as in Kontakt.</p>
<p>Each library plays the same 13 things in both: single notes low, middle and high at three strengths, a chord, fast repeats, a mod wheel sweep and a pedalled phrase. A play matches when loudness, tone in every third-octave band, start time, attack, release and left/right balance are all within about 1 dB and 5 ms. Measured {measured}{at}.</p>
<div class="sampler-list">{rows}</div></div></section>'''


def build(output):
    output = output.resolve()
    if output == SITE or SITE.is_relative_to(output):
        raise ValueError('Build into a separate output directory, not the source tree')
    output.mkdir(parents=True, exist_ok=True)
    shutil.copytree(SITE / 'assets', output / 'assets', dirs_exist_ok=True)
    for pattern in ('*.html', '*.css', '*.js'):
        for source in SITE.glob(pattern):
            shutil.copy2(source, output / source.name)
    progress = (SITE / 'progress.html').read_text()
    marker = '<!-- progress:sound -->'
    assert progress.count(marker) == 1, 'Progress template marker missing or duplicated'
    sound = json.loads((SITE / 'progress.json').read_text())['sound']
    (output / 'progress.html').write_text(progress.replace(marker, render_sound(sound)))
    # The compatibility table is the parity board, from the page tools/parity-board.py writes, in the site's
    # chrome. It is not committed in the monorepo; the public mirror carries the copy the mirror job generated.
    if not (ROOT / 'docs/PARITY.html').is_file():
        subprocess.run([sys.executable, str(ROOT / 'tools/parity-board.py')], check=True)
    board = re.search(r'<!-- board:start -->.*<!-- board:end -->', (ROOT / 'docs/PARITY.html').read_text(), re.S).group()
    page = (SITE / 'features.html').read_text()
    assert page.count('<!-- compatibility:board -->') == 1, 'Compatibility template marker missing or duplicated'
    revision = git('rev-parse', 'HEAD')
    meta = f'<a href="{REPO}/commit/{revision}">{revision[:7]}</a>'
    (output / 'features.html').write_text(page.replace('<!-- compatibility:board -->', board).replace('<!-- compatibility:revision -->', meta))
    # Old addresses of the compatibility table.
    for old in ('parity.html', 'tracker/index.html'):
        (output / old).parent.mkdir(exist_ok=True)
        up = '../' * old.count('/')
        (output / old).write_text(f'<!doctype html><html lang="en"><meta charset="utf-8"><meta http-equiv="refresh" content="0;url={up}features.html"><title>Compatibility — KONTRA</title><a href="{up}features.html">Compatibility</a></html>')
    print(f'Built the site at {revision[:7]} into {output}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    build(parser.parse_args().output)
