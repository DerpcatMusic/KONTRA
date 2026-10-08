#!/usr/bin/env python3
"""Publish frozen TSVs, load regressions and per-instrument UI mechanism counts."""
from collections import Counter, defaultdict
import csv
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys

root = Path(sys.argv[1] if len(sys.argv) > 1 else '/home/derpcat/.cache/kontra-scan')
spec=importlib.util.spec_from_file_location('scanner',Path(__file__).with_name('kontra_scan.py'));scanner=importlib.util.module_from_spec(spec);spec.loader.exec_module(scanner)
versions = {}
for version in ['v1', 'v2']:
    folder = root / 'results' / version
    source = folder / 'results.tsv'
    if not source.exists():
        continue
    shutil.copyfile(source, root / 'results' / (version + '.tsv'))
    with source.open() as f:
        versions[version] = list(csv.DictReader(f, delimiter='\t'))

text = ['# Shared scanner results', '', 'Coverage is partial until every manifest ID has a row from the installed binary revision. Original authored view only. `loads=yes` means importer and initial playable bank/plan construction returned successfully; UI and sound are separate outcomes.', '',
        'Timeouts are bounded probe failures, not proof of an intrinsically unsupported library. `silent` means an actual selected note was not audible during the 0.5-second probe with CC1=100 and CC11=127. No-safe-key rows are not auditioned; fallback-note rows are excluded from parity comparisons.', '',
        '| Build | Rows | Loads yes | Loads no | Original OK | Missing images | Blank | No UI | UI error | Budget hit | Audible | Silent |',
        '| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |']
taxonomy = {}
for version, rows in versions.items():
    statuses = Counter(r['ui'] for r in rows)
    loads = Counter(r['loads'] for r in rows)
    sound = Counter(r['plays_note'] for r in rows)
    text.append(f'| {version} | {len(rows)} | {loads["yes"]} | {loads["no"]} | {statuses["original-ok"]} | {statuses["missing-images"]} | {statuses["blank"]} | {statuses["no-ui"]} | {statuses["error"]} | {statuses["budget-hit"]} | {sound["yes"]} | {sound["silent"]} |')
    paths = {r['path'] for r in rows}
    records = [json.loads(p.read_text()) for p in (root / 'results' / version / 'cache').glob('*.json') if p.stem==scanner.signature(json.loads(p.read_text())['path'],json.loads(p.read_text())['revision'])]
    # Cached records belonging to a previous binary revision do not enter published counts.
    revision = hashlib.sha256((root/'bin'/('kontra-scan-'+version)).read_bytes()).hexdigest()
    mechanisms = defaultdict(set)
    symbols = defaultdict(set)
    occurrences = Counter()
    for r in records:
        if r.get('revision') != revision or r['path'] not in paths:
            continue
        path = r['path']
        if r.get('timed_out'):
            mechanisms['worker timeout: ' + r.get('stage', 'unknown')].add(path)
        if r['ui'] in {'error', 'blank', 'missing-images','budget-hit'}:
            mechanisms['category: ' + r['ui']].add(path)
        for slot in r.get('metadata',{}).get('slots',[]):
            for symbol,count in slot.get('symbols',{}).items():
                symbols[symbol].add(path);occurrences[symbol]+=count
        for program in r.get('programs', []):
            for symbol, count in program.get('symbols', {}).items():
                if 'metadata' not in r:
                    symbols[symbol].add(path)
                    occurrences[symbol] += count
            if program.get('script_error_count', 0):
                mechanisms['script initialization failure'].add(path)
            for key in program.get('script_errors', {}):
                mechanisms['script diagnostic: ' + key].add(path)
            for slot in program.get('ksp',{}).get('slots',[]):
                if slot.get('compile_fault'):mechanisms['KSP compiler rejection'].add(path)
                if slot.get('disabled_block_errors'):mechanisms['v1 admitted script has disabled callback blocks'].add(path)
                for phase in ['init','persistence_changed']:
                    status=slot.get(phase,{}).get('status','unknown')
                    if status not in ['absent','completed','unknown']:mechanisms['KSP '+phase+': '+status].add(path)
            for v in program.get('views', []):
                if v.get('missing_images', 0):
                    mechanisms['authored image reference does not resolve/decode'].add(path)
                if v.get('bound', 0) < v.get('interactive', 0):
                    mechanisms['visible interactive widget has no runtime scalar binding'].add(path)
                if v.get('passive_value_changes', 0):
                    mechanisms['passive render changes semantic control value'].add(path)
                for render in v.get('renders',[]):
                    if render.get('budget_hit'):mechanisms['Original paint: MUI tree budget'].add(path)
                    background=render.get('background',{})
                    if background.get('plain_background_fraction',0)>.9:mechanisms['page >90% plain background candidate'].add(path)
                if v.get('custom_font_uses'):mechanisms['authored custom font'].add(path)
                for field in ['placeholder_widgets', 'unsupported_params', 'geometry']:
                    for key in v.get(field, {}):
                        mechanisms[field + ': ' + key].add(path)
    taxonomy[version] = {'rows': len(rows), 'loads': dict(loads), 'ui': dict(statuses), 'audio': dict(sound),
                         'mechanisms': {k: {'instruments': len(v), 'paths': sorted(v)} for k, v in sorted(mechanisms.items(), key=lambda kv: (-len(kv[1]), kv[0]))},
                         'symbols': {k: {'instruments': len(v), 'occurrences': occurrences[k]} for k, v in sorted(symbols.items())}}

if 'v1' in versions and 'v2' in versions:
    v2 = {r['path']: r for r in versions['v2']}
    regressions = [r['path'] for r in versions['v1'] if r['loads'] == 'yes' and r['path'] in v2 and v2[r['path']]['loads'] == 'no']
    with (root / 'results/v1-loads-v2-doesnt.tsv').open('w') as f:
        writer = csv.writer(f, delimiter='\t', lineterminator='\n')
        writer.writerow(['path', 'library', 'v2_reason'])
        for p in regressions:
            writer.writerow([p, v2[p]['library'], v2[p]['reason']])
    mismatches=[p for p,r in v2.items() if p in {x['path'] for x in versions['v1']} and (r.get('note_picked')!=next(x['note_picked'] for x in versions['v1'] if x['path']==p) or r.get('audition_status')=='audition-mismatch')]
    text += ['',f'Audio comparison note mismatches: **{len(mismatches)}**. These cannot support sound regressions.', f'V1 loads successfully and v2 does not: **{len(regressions)} instruments**. Full list: `v1-loads-v2-doesnt.tsv`.', '']

text += ['## Measured UI mechanisms', '', '| Version | Mechanism | Instruments |', '| --- | --- | ---: |']
for version, data in taxonomy.items():
    for key, value in data['mechanisms'].items():
        text.append(f'| {version} | {key.replace("|", "/")} | {value["instruments"]} |')
(root / 'results/summary.md').write_text('\n'.join(text) + '\n')
(root / 'results/taxonomy.json').write_text(json.dumps(taxonomy, indent=2) + '\n')
print('\n'.join(text[:9]))
