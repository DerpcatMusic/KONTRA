#!/usr/bin/env python3
"""Build the public site from the checked-out revision, without private catalog caches."""
import argparse
from collections import Counter
from datetime import datetime
from html import escape
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from urllib.parse import quote

SITE = Path(__file__).resolve().parent
ROOT = SITE.parent
os.environ['KONTRA_COVERAGE_SOURCE_ONLY'] = '1'
sys.path.insert(0, str(ROOT / 'tools/coverage'))
import tracker

REPO = 'https://github.com/DerpcatMusic/KONTRA'
LABELS = {'done': 'Implemented', 'partial': 'Partial', 'todo': 'Not implemented'}
PROOFS = {'proved': 'Verified mapping', 'unproven': 'Needs verification', 'debunked': 'Known mismatch', 'n/a': ''}


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def collect(features, revision, committed_at):
    """Validate the tracker contract before publishing compatibility claims."""
    if len(revision) != 40 or any(c not in '0123456789abcdef' for c in revision):
        raise ValueError('Expected a full Git commit SHA')
    datetime.fromisoformat(committed_at)
    rows, seen = [], set()
    for feature in features:
        f = dict(feature)
        if f['id'] in seen:
            raise ValueError(f'Duplicate feature: {f["id"]}')
        seen.add(f['id'])
        if f['status'] not in LABELS or f['proof'] not in PROOFS:
            raise ValueError(f'Unknown tracker status: {f["id"]}')
        if f['status'] == 'done' and f['proof'] != 'proved':
            raise ValueError(f'Implemented feature lacks verification: {f["id"]}')
        source = f.get('src')
        if source:
            path = (ROOT / source).resolve()
            if not path.is_relative_to(ROOT) or not path.is_file():
                raise ValueError(f'Invalid source path: {source}')
        rows.append({
            'id': f['id'], 'product': f['product'], 'area': f['area'], 'name': f['name'],
            'status': f['status'], 'label': LABELS[f['status']], 'note': f['note'],
            'proof': PROOFS[f['proof']],
            'source': f'{REPO}/blob/{revision}/{quote(source)}' if source else '',
        })
    rows.sort(key=lambda f: (f['name'].casefold(), f['product'], f['id']))
    return {'revision': revision, 'committedAt': committed_at, 'rows': rows,
            'counts': dict(Counter(f['status'] for f in rows))}


def render_row(f):
    source = (f'<a href="{escape(f["source"], quote=True)}" aria-label="Source for {escape(f["name"], quote=True)}">View</a>'
              if f['source'] else '—')
    proof = f'<small class="proof-note">{escape(f["proof"])}</small>' if f['status'] == 'partial' else ''
    return (f'<tr><th scope="row">{escape(f["name"])}</th>'
            f'<td class="status-{f["status"]}">{f["label"]}{proof}</td>'
            f'<td>{escape(f["product"])}</td><td>{escape(f["area"])}</td>'
            f'<td class="feature-note">{escape(f["note"])}</td><td>{source}</td></tr>')


def render_table(data):
    options = lambda values: ''.join(f'<option value="{escape(v, quote=True)}">{escape(v)}</option>' for v in sorted(set(values)))
    controls = f'''<form class="table-filters" id="feature-filters" hidden role="search" aria-label="Filter compatibility">
<label class="feature-search">Search features<input id="feature-search" type="search" placeholder="Search features or notes" autocomplete="off"></label>
<label>Sampler<select id="feature-product"><option value="">All samplers</option>{options(f['product'] for f in data['rows'])}</select></label>
<label>Category<select id="feature-area"><option value="">All categories</option>{options(f['area'] for f in data['rows'])}</select></label>
<label>Status<select id="feature-status"><option value="">All statuses</option>{''.join(f'<option value="{key}">{label}</option>' for key, label in LABELS.items())}</select></label>
<button type="reset" class="quiet-button">Clear filters</button></form>'''
    headers = ''
    for key, label in [('name', 'Feature'), ('status', 'Status'), ('product', 'Sampler'), ('area', 'Category')]:
        sort_state = ' aria-sort="ascending"' if key == 'name' else ''
        headers += f'<th scope="col"{sort_state}><button type="button" data-sort="{key}" disabled>{label}<span class="sort-mark" aria-hidden="true"></span></button></th>'
    # Escape '<' so source text can never terminate this non-executable JSON script.
    embedded = json.dumps(data, ensure_ascii=False).replace('<', '\\u003c')
    return f'''{controls}
<div class="table-result-line"><p id="feature-results" role="status" aria-live="polite">{len(data['rows'])} features</p><a href="compatibility.json">Download data</a></div>
<div class="table-scroll feature-scroll" tabindex="0" role="region" aria-label="Compatibility table, scroll horizontally for notes and source">
<table class="feature-table" id="feature-table"><caption class="sr-only">Feature implementation in Kontakt and UVI. Sortable columns: feature, status, sampler and category.</caption>
<thead><tr>{headers}<th scope="col">Implementation notes</th><th scope="col">Source</th></tr></thead>
<tbody>{''.join(render_row(f) for f in data['rows'])}</tbody></table></div>
<p class="empty-results" id="feature-empty" hidden>No features match. <button type="button" class="text-button" id="feature-clear">Clear filters</button></p>
<nav class="table-pagination" id="feature-pagination" aria-label="Table pages" hidden><button type="button" id="feature-prev" class="quiet-button">Previous</button><span id="feature-page"></span><button type="button" id="feature-next" class="quiet-button">Next</button></nav>
<script type="application/json" id="compatibility-data">{embedded}</script>'''


def build(output):
    output = output.resolve()
    if output == SITE or SITE.is_relative_to(output):
        raise ValueError('Build into a separate output directory, not the source tree')
    data = collect(tracker.all_features(), git('rev-parse', 'HEAD'), git('show', '-s', '--format=%cI', 'HEAD'))
    output.mkdir(parents=True, exist_ok=True)
    shutil.copytree(SITE / 'assets', output / 'assets', dirs_exist_ok=True)
    for pattern in ('*.html', '*.css', '*.js'):
        for source in SITE.glob(pattern):
            shutil.copy2(source, output / source.name)
    page = (SITE / 'features.html').read_text()
    marker = '<!-- compatibility:table -->'
    assert page.count(marker) == 1, 'Compatibility template marker missing or duplicated'
    page = page.replace(marker, render_table(data))
    date = datetime.fromisoformat(data['committedAt']).strftime('%d %b %Y').lstrip('0')
    revision = data['revision']
    meta = f'<a href="{REPO}/commit/{revision}" title="Source revision {revision}">{revision[:7]}</a> · <time datetime="{data["committedAt"]}">{date}</time>'
    page = page.replace('<!-- compatibility:revision -->', meta)
    counts = data['counts']
    page = page.replace('<!-- compatibility:counts -->', ' · '.join(f'<span class="status-{key}">{counts.get(key, 0)} {label.lower()}</span>' for key, label in LABELS.items()))
    (output / 'features.html').write_text(page)
    (output / 'compatibility.json').write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
    # Preserve links to the old tracker while keeping one public compatibility surface.
    (output / 'tracker').mkdir(exist_ok=True)
    (output / 'tracker/index.html').write_text('<!doctype html><html lang="en"><meta charset="utf-8"><meta http-equiv="refresh" content="0;url=../features.html"><title>Compatibility — KONTRA</title><a href="../features.html">Compatibility table</a></html>')
    print(f'Built {len(data["rows"])} tracked features at {revision[:7]} into {output}')
    return data


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    build(parser.parse_args().output)
