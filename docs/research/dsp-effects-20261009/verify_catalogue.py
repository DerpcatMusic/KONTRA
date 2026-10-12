"""Validate catalogue coverage and evidence locations against the pinned corpus."""
import csv
import hashlib
import json
from pathlib import Path
import re
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
B = ROOT / 'artifacts/engine-analysis-2026-10-07'
data = json.loads((HERE / 'catalogue-evidence.json').read_text())
original = json.loads((B / 'catalog.json').read_text())
for source in data['inputs']:
    assert hashlib.sha256((ROOT / source['path']).read_bytes()).hexdigest() == source['sha256'], source['path']

def kontakt_key(row): return row['family'], row['category'], row['name']
assert len(data['kontakt']) == 124
assert {kontakt_key(r) for r in data['kontakt']} == {kontakt_key(r) for r in original['kontakt_documented']}
assert sum(r['family'] == 'effects' for r in data['kontakt']) == 65
assert sum(r['family'] == 'filters' for r in data['kontakt']) == 59
expected_uvi = {r['internal_name']: r for r in original['falcon_api'] if r['category'] in ['FX', 'Legacy']}
assert len(data['workstation']) == len(expected_uvi) == 115
assert {r['internal_name'] for r in data['workstation']} == set(expected_uvi)
assert sum(len(r['parameters']) for r in data['workstation']) == 1257
assert sum(r['category'] == 'FX' for r in data['workstation']) == 83
assert sum(r['category'] == 'Legacy' for r in data['workstation']) == 32

vtable_rows = list(csv.DictReader((B / 'kontakt-engine/vtable-methods.csv').open()))
original_structure = json.loads((B / 'uvi-workstation/compiled-module-structure.json').read_text())
assert data['workstation_sha256'] == original_structure['reference_sha256']
assert data['workstation_text_sha256'] == original_structure['snapshot_sha256']
assert data['kontakt_sha256'] == json.loads((B / 'kontakt-engine/pe.json').read_text())['sha256']
registrations = {r['registration_call_va']: r for r in original_structure['records']}
locations = {}

def location(path, line, va):
    assert line and int(line) > 0, (path, line, va)
    locations.setdefault(B / path, set()).add((int(line), va.removeprefix('0x').lower()))

for row in data['kontakt']:
    assert row['source'].startswith('https://docs.native-instruments.com/') and '#' in row['source']
    assert row['processing_contract'] and row['complete_effect_verified'] is False
    for native in row['native_candidates'] + row['related_native_types']:
        assert native['public_name_to_type_mapping_verified'] is False
        for entry in native['observed_virtual_entries']:
            assert entry in vtable_rows, entry
            assert entry['type'] == native['type']
            location(entry['pseudocode_file'], entry['c_line'], entry['entry_va'])

for row in data['workstation']:
    assert row['parameters'] == expected_uvi[row['internal_name']]['parameters']
    assert row['complete_effect_verified'] is False
    native = row['native_candidate']
    r = registrations[native['registration_call_va']]
    assert native['registration_name_candidates'] == r['names']
    assert native['registration_name_candidates'][-1] == row['internal_name']
    assert native['runtime_instantiation_verified'] is False
    for field in ['callback', 'constructor']:
        assert native[field + '_va'] == r[field + '_va']
        p = native[field + '_pseudocode']
        assert p == r[field + '_pseudocode']
        location('uvi-workstation/' + p['file'], p['marker_line'], native[field + '_va'])
    for candidate in native['owner_slot_0x1f8_candidates']:
        assert any(t['va'] == candidate['table_va'] and t['assigned_to_constructor_receiver']
                   and candidate['entry'] in t['entries'] for t in r['pointer_tables'])
        e = candidate['entry']
        location('uvi-workstation/' + e['pseudocode']['file'], e['pseudocode']['marker_line'], e['target_va'])

resource = json.loads((B / 'uvi-workstation/resource-format-catalog.json').read_text())
assert len(data['saved_xml_schemas']) == 49
assert len(data['saved_xml_non_audio_tags']) == 7
assert set(data['saved_xml_schemas']) | set(data['saved_xml_non_audio_tags']) == set(resource['schemas'])
for tag, schema in data['saved_xml_schemas'].items(): assert schema == resource['schemas'][tag]

# Exporter line metadata can point at the blank line immediately before a marker.
for path, targets in locations.items():
    wanted = {line + delta for line, va in targets for delta in range(-1, 4)}
    last_line = max(wanted)
    found = {}
    with path.open() as file:
        for number, line in enumerate(file, 1):
            if number in wanted: found[number] = line
            if number > last_line: break
    for number, va in targets:
        assert any(re.search(r'/\* VA ' + va + r' \*/', found.get(number + delta, ''), re.I)
                   for delta in range(-1, 4)), (path, number, va)

def slug(heading):
    heading = re.sub(r'[^\w\- ]', '', heading.lower())
    return heading.replace(' ', '-')

documents = [ROOT / 'docs' / name for name in ['DSP_EFFECT_CATALOGUE.md', 'KONTAKT_DSP_EFFECTS.md', 'WORKSTATION_DSP_EFFECTS.md']]
documents.append(HERE / 'README.md')
for path in documents:
    text = path.read_text()
    for target in re.findall(r'\]\(([^)]+)\)', text):
        if target.startswith(('https://', 'http://')): continue
        name, _, anchor = target.partition('#')
        linked = (path.parent / unquote(name)).resolve() if name else path
        assert linked.is_file(), (path, target)
        if anchor and linked.suffix == '.md':
            headings = re.findall(r'^#+ (.+)$', linked.read_text(), re.M)
            assert anchor in {slug(h) for h in headings}, (path, target)
for path, expected in [(documents[1], {r['name'] for r in data['kontakt']}),
                       (documents[2], {r['name'] + ' — `' + r['internal_name'] + '`' for r in data['workstation']})]:
    headings = re.findall(r'^### (.+)$', path.read_text(), re.M)
    assert len(headings) == len(expected) and set(headings) == expected, path

results = json.loads((HERE / 'specialized-kernel-results.json').read_text())
assert results['reference_sha256'] == data['workstation_sha256']
assert results['snapshot_sha256'] == data['workstation_text_sha256']
assert len(results['dispersor_cases']) == 168 and len(results['robotizer_cases']) == 864
assert all(r['exact_output_and_history'] for r in results['dispersor_cases'] + results['robotizer_cases'])
assert results['helper_substitutions'] == []
assert results['public_parameter_mapping_verified'] is False and results['full_host_parity_verified'] is False

total_cases = len(results['dispersor_cases']) + len(results['robotizer_cases'])
for filename, counts in [
    ('onepole-machine-code-verification.json', {'setter_cases': 10, 'kernel_cases': 480}),
    ('shape-decimation-machine-code-verification.json', {'rectifier_cases': 480, 'fractional_decimator_cases': 495, 'bitcrusher_cases': 2268}),
    ('lfo-machine-code-verification.json', {'setter_cases': 12, 'render_cases': 192}),
]:
    retained = json.loads((B / 'uvi-workstation' / filename).read_text())
    assert retained['reference_sha256'] == data['workstation_sha256']
    assert retained['snapshot_sha256'] == data['workstation_text_sha256']
    assert retained['helper_substitutions'] == [] and retained['full_host_or_dsp_parity_verified'] is False
    for field, count in counts.items():
        assert len(retained[field]) == count, (filename, field)
        total_cases += count
kontakt = json.loads((B / 'deep-dsp/machine-code-verification.json').read_text())
assert kontakt['binary_sha256'] == data['kontakt_sha256'] and kontakt['full_host_audio_parity_verified'] is False
kontakt_cases = len(kontakt['daft_subtype_selector']) + len(kontakt['daft_parameter_mapping'])
kontakt_cases += sum(len(kontakt[field]['cases']) for field in
                     ['gainer', 'gain_copy', 'daft_cadence', 'daft_ramp', 'daft_setup', 'daft_curves', 'compressor_link'])
assert kontakt_cases == 70
total_cases += kontakt_cases
assert total_cases == 5039

live = json.loads((HERE / 'live-uvi-interface-audit.json').read_text())
assert hashlib.sha256((ROOT / live['snapshot_path']).read_bytes()).hexdigest() == live['snapshot_sha256']
assert live['catalogue_anchor_count'] == 115 and live['missing_catalogue_anchors'] == []
assert live['fx_legacy_type_count'] == 125 and len(live['unmatched_public_types']) == 10
registered_names = {name for row in original_structure['records'] for name in row['names']}
assert all(row['internal_name'] not in registered_names for row in live['unmatched_public_types'])
print('Catalogue checks passed: 239 entries; 1,257 typed parameters; 49 audio XML tags; source hashes, native locations and local links; 5,039 retained native cases; 115 live anchors.')
