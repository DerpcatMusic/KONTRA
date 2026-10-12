"""Summarize owner-local inventories without copying scripts, artwork or sample data."""
import collections
import gzip
import json
import pathlib
import re

repo = pathlib.Path(__file__).resolve().parents[1]
folder = repo / 'audits'
source = folder / 'library-compatibility.json'
with (source.open() if source.exists() else gzip.open(str(source) + '.gz', 'rt')) as stream:
    records = json.load(stream)
structures = json.loads((folder / 'source-structures.json').read_text())
# Script-only rescans refresh interpreter results without repeating sample resolution.
script_file = folder / 'script-requirements.json'
if script_file.exists():
    updates = json.loads(script_file.read_text())
    assert all('error' not in item for item in updates), 'Script extraction failed; inspect script-requirements.json'
    scripts = {(item['path'], program['program']): program['scripts'] for item in updates for program in item['programs']}
    assert all((item['path'], item.get('program', 0)) in scripts for item in records if 'error' not in item)
    for item in records:
        if 'error' not in item:
            item['scripts'] = scripts[item['path'], item.get('program', 0)]
root = '/mnt/MAIN_STORAGE/Libraries/Kontakt/'
names = dict(re.findall(r'(0x[0-9a-f]+) => KontaktObject::(\w+)', (repo / 'vendor/ni-file/src/kontakt/chunk.rs').read_text()))
libs = {}
issues = collections.defaultdict(set)
requirements = {key: collections.defaultdict(set) for key in ['callbacks', 'calls', 'controls', 'constants']}
init_errors = collections.defaultdict(set)
for item in records:
    path = item['path']
    library = path.removeprefix(root).split('/')[0]
    lib = libs.setdefault(library, {'presets': set(), 'programs': 0, 'parse_errors': 0, 'complete_references': 0, 'complete_groups': 0, 'script_slots': 0, 'init_previews': 0})
    lib['presets'].add(path)
    lib['programs'] += 1
    identity = path + f"#program={item.get('program', 0)}"
    if 'error' in item:
        lib['parse_errors'] += 1
        issues['Parse: ' + item['error']].add(identity)
        continue
    lib['complete_references'] += not item['missing_samples']
    lib['complete_groups'] += item['complete_groups']
    if item['missing_samples']:
        issues['Missing/damaged sample references'].add(identity)
    for warning in item['warnings']:
        if 'native group start conditions' in warning:
            warning = 'Native group start conditions are not implemented'
        if 'active KSP script(s)' in warning:
            warning = 'KSP runtime callbacks are not implemented'
        if ': ' in warning and ('archive' in warning.lower() or '.nkx:' in warning.lower()):
            warning = 'Archive member/directory damage (see per-program report)'
        issues[warning].add(identity)
    for script in item['scripts']:
        lib['script_slots'] += 1
        for key in requirements:
            for name in script['requirements'].get(key, []):
                requirements[key][name].add(identity)
        if 'inventory_error' in script['requirements']:
            issues['KSP static inventory: ' + script['requirements']['inventory_error']].add(identity)
        init = script['initialization']
        if 'error' in init:
            init_errors[init['error']].add(identity)
        else:
            lib['init_previews'] += 1
            for diagnostic in init['diagnostics']:
                issues['KSP initialization: ' + diagnostic].add(identity)
chunks = collections.defaultdict(set)
structure_errors = collections.defaultdict(set)
for item in structures:
    if 'error' in item:
        structure_errors[item['error']].add(item['path'])
    for key, row in item.get('inventory', {}).get('chunks', {}).items():
        ident = key.split('/')[-1]
        chunks[f'{ident} {names.get(ident, "Unknown")}'].add(item['path'])
        if 'inspection_error' in row:
            structure_errors[key + ': ' + row['inspection_error']].add(item['path'])
def indexed(mapping):
    return {name: {'programs_or_presets': len(paths), 'affected': sorted(paths)} for name, paths in sorted(mapping.items(), key=lambda kv: (-len(kv[1]), kv[0]))}
for lib in libs.values():
    lib['presets'] = len(lib['presets'])
summary = {'scope': 'Static NKI/NKM and resource-reference audit; no full audio decode or Kontakt behavioral validation. Chunk presence does not prove activation. Opaque private fields remain unmapped.', 'libraries': libs, 'gaps': indexed(issues), 'ksp_requirements': {key: indexed(value) for key, value in requirements.items()}, 'ksp_initialization_failures': indexed(init_errors), 'source_chunk_presence': indexed(chunks), 'source_inspection_failures': indexed(structure_errors)}
(folder / 'compatibility-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
lines = ['# Local library compatibility inventory', '', summary['scope'], '', 'Generated with `kontakto audit` (plain JSON or gzip), `kontakto audit-structure`, `kontakto audit-scripts`, then `python3 tools/summarize_audit.py`.', '', '| Library | Presets | Programs | Parse errors | Complete references | Complete groups | Init previews / scripts |', '|---|---:|---:|---:|---:|---:|---:|']
for name, lib in sorted(libs.items()):
    lines.append(f"| {name} | {lib['presets']} | {lib['programs']} | {lib['parse_errors']} | {lib['complete_references']} | {lib['complete_groups']} | {lib['init_previews']} / {lib['script_slots']} |")
lines += ['', 'A complete reference set does not certify decoding or playback. Zero-zone controller instruments can have complete references and no playable groups.', '', '## Implementation gaps', '', '| Gap | Affected programs |', '|---|---:|']
for name, value in summary['gaps'].items():
    lines.append(f"| {name.replace('|', '/')} | {value['programs_or_presets']} |")
lines += ['', '## Initialization blockers', '', '| First error per script | Affected programs |', '|---|---:|']
for name, value in summary['ksp_initialization_failures'].items():
    lines.append(f"| {name.replace('|', '/')} | {value['programs_or_presets']} |")
lines += ['', '## KSP requirements', '', 'All non-init callbacks remain unimplemented. Command presence below does not imply execution support; initialization-only behavior is limited to the subset described in the project README.', '']
for category in ['callbacks', 'calls', 'controls']:
    lines += [f'### {category.capitalize()}', '', '| Observed name | Programs |', '|---|---:|']
    for name, value in summary['ksp_requirements'][category].items():
        lines.append(f"| `{name}` | {value['programs_or_presets']} |")
    lines.append('')
lines += ['', '## Source structures', '', 'Presence includes bypassed/default objects. Counts are presets, not active effects. The JSON sidecar retains affected paths and every observed KSP symbol.', '', '| Chunk | Presets |', '|---|---:|']
for name, value in summary['source_chunk_presence'].items():
    lines.append(f"| {name} | {value['programs_or_presets']} |")
lines += ['', f"Source inspection error categories: {len(structure_errors)}.", '', '## Acceptance gates', '', '- Reference resolution, bounded sample decoding and finite audio renders.', '- Restore source envelopes, modulation, effects, group triggers and multi routing.', '- Execute KSP callbacks and restore saved control values; initialization previews alone do not satisfy this.', '- Compare controlled MIDI renders and UI actions against Kontakt reference results.', '- Keep missing or zero-filled resources blocked until intact data is available.', '']
(folder / 'README.md').write_text('\n'.join(lines))
print(json.dumps({'presets': len(structures), 'programs': len(records), 'libraries': len(libs), 'parse_errors': sum(x['parse_errors'] for x in libs.values()), 'source_error_categories': len(structure_errors), 'initialization_error_categories': len(init_errors)}, indent=2))
