"""Group missing references by resolved archive; resolve each archive once."""
import collections
import gzip
import json
import pathlib
import re

folder = pathlib.Path(__file__).resolve().parents[1] / 'audits'
source = folder / 'library-compatibility.json'
with (source.open() if source.exists() else gzip.open(str(source) + '.gz', 'rt')) as stream:
    presets = json.load(stream)
archives = {}
groups = collections.defaultdict(lambda: {'programs': set(), 'members': set()})
pattern = re.compile(r'\.nk[xr](?=/)', re.I)
for preset in presets:
    parent = pathlib.Path(preset['path']).parent
    for name in preset.get('missing_samples', []):
        name = name.replace('\\', '/')
        match = pattern.search(name)
        if match:
            key = str(parent), name[:match.end()]
            if key not in archives:
                archives[key] = str((parent / key[1]).resolve())
            group = groups[archives[key]]
            group['programs'].add((preset['path'], preset['program']))
            group['members'].add(name[match.end() + 1:])
rows = sorted(({'archive': path, 'exists': pathlib.Path(path).is_file(), 'programs': len(group['programs']), 'members': len(group['members'])} for path, group in groups.items()), key=lambda r: -r['programs'])
health_file = folder / 'archive-health.json'
if health_file.exists():
    health = {str(pathlib.Path(item['path']).resolve()): item for item in json.loads(health_file.read_text())}
    for row in rows:
        item = health.get(row['archive'], {})
        row['invalid_headers'] = item.get('invalid_headers')
        row['index_error'] = item.get('error')
(folder / 'resource-correlations.json').write_text(json.dumps(rows, indent=2) + '\n')
lines = ['# Shared resource blockers', '', 'Counts group unresolved references by archive. An existing archive does not establish that its referenced members are intact or present. Programs may appear in several rows.', '', '| Archive | Exists | Affected programs | Missing member names | Invalid headers in archive |', '|---|---|---:|---:|---:|']
for row in rows:
    name = row['archive'].split('/Kontakt/', 1)[-1]
    lines.append(f"| {name} | {row['exists']} | {row['programs']} | {row['members']} | {row.get('invalid_headers', 'Not audited')} |")
(folder / 'RESOURCES.md').write_text('\n'.join(lines) + '\n')
print(json.dumps(rows[:8], indent=2))
