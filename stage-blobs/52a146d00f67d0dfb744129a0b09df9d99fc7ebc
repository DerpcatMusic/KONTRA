#!/usr/bin/env python3
"""Join disjoint census shards and annotate measured fields, without payloads."""
import argparse
import csv
from collections import defaultdict
from pathlib import Path


def read(path):
    with path.open() as stream:
        return list(csv.DictReader(stream, delimiter='\t'))


def write(path, columns, rows):
    with path.open('w') as stream:
        writer = csv.DictWriter(stream, columns, delimiter='\t', lineterminator='\n')
        writer.writeheader()
        writer.writerows(rows)


def counts(paths, keys, values):
    result = {}
    for path in paths:
        for row in read(path):
            key = tuple(row[k] for k in keys)
            saved = result.setdefault(key, {k: row[k] for k in keys} | {v: 0 for v in values})
            for value in values:
                saved[value] += int(row[value])
    return result


USED = {
    'Kontakt:0x28': set('volume pan tune transpose'.split()),
    'Kontakt:0x04': set('volume pan tune key_tracking reverse release_trigger release_trigger_note_monophonic voice_group_index muted'.split()),
    'Kontakt:0x2c': set('sample_start sample_end sample_start_mod_range low_velocity high_velocity low_key high_key fade_low_velocity fade_high_velocity fade_low_key fade_high_key root_key zone_volume zone_pan zone_tune filename_id'.split()),
    'Kontakt:0x05': set('mode start length alternating crossfade'.split()),
    'Kontakt:0x25': set('effect_type bypass output_gain dry_level'.split()),
    'Kontakt:0x06': {'linked_script', 'text', 'bypass', 'persistent'},
}

RAW = {'public', 'private', 'body', 'source', 'trailing', 'flat_prefix', 'flat_suffix', 'extension', 'public_extension', 'reserved_word', 'loading_flags', 'fingerprint', 'reserved1', 'reserved2', 'reserved3', 'reserved4', 'object', 'raw', 'public_data', 'private_data', 'version', 'children', 'trailing_data'}

RECORDS = {
    'ProgramPublicParams': 'Kontakt:0x28', 'ProgramResources': 'Kontakt:0x28',
    'GroupParams': 'Kontakt:0x04', 'ZoneParams': 'Kontakt:0x2c',
    'BankPublicParams': 'Kontakt:0x03', 'BParFXParams': 'Kontakt:0x25',
    'QuickBrowseDataParams': 'Kontakt:0x4e', 'QuickBrowse': 'Kontakt:0x4e',
    'SaveSettings': 'Kontakt:0x47', 'FNTableSampleRecord': 'Kontakt:0x4b',
    'BParScriptParams': 'Kontakt:0x06', 'GroupSnapshot': 'Kontakt:0x50',
}
ALIASES = {'translated': 'translated_reference', 'original': 'original_reference',
           'timestamp': 'sample_timestamp', 'unknown_record': 'sample_unknown_record',
           'textfile_name': 'linked_script', 'container': 'container_reference',
           'snapshot_directory': 'snapshot_directory_reference',
           'full_path': 'full_path_reference', 'wallpaper': 'wallpaper_reference'}


def declarations(rows, registry, owners, out):
    """Keep unmeasured declaration coverage explicit rather than inventing zeroes."""
    result = []
    for source in read(registry.parent / 'KONTAKT_FIELD_REGISTRY.tsv'):
        record, field = source['record'], source['field']
        structure = RECORDS.get(record, '')
        wire_field = ALIASES.get(field, field)
        observed = [r for r in rows if r['structure'] == structure and r['field'] == wire_field]
        state, owner, consequence = coverage({'structure': structure, 'field': wire_field}, owners) if structure else ('raw/unknown', 'see family map', 'declaration only; framing/semantic/runtime evidence in family doc')
        vendor = 'typed field' if record in {'ProgramPublicParams', 'GroupParams', 'ZoneParams', 'BankPublicParams', 'BParFXParams', 'QuickBrowseDataParams', 'BParScriptParams', 'FNTableSampleRecord', 'GroupSnapshot'} else 'source view/declaration; see exact source'
        if record == 'ProgramPublicParams' and field in {'resource_container_filename', 'wallpaper_filename'}:
            vendor = 'convenience reader always returns None'
        if source['v1_definition'] == 'absent':
            v1 = 'absent definition; importer-specific alternatives require family evidence'
        elif record == 'ZoneParams':
            v1 = 'prefix decoded+used' if field in USED['Kontakt:0x2c'] else 'unread by v1 importer after filename_id'
        elif record in {'ProgramPublicParams', 'GroupParams', 'BankPublicParams', 'BParFXParams', 'BParScriptParams'}:
            v1 = 'typed decoder; importer use in family/master matrix'
        else:
            v1 = 'definition present; importer use unestablished here'
        result.append(source | {'structure': structure or 'see family/source', 'vendor_evidence': vendor,
                                'v1_importer_evidence': v1, 'v2_status': state, 'owner': owner,
                                'corpus_by_version': ';'.join(f"{r['version']}:{r['files']}/{r['nonbaseline_files']}" for r in observed) or 'unmeasured; not zero',
                                're_evidence': 'family/master layout; unknown semantics remain unestablished',
                                'consequence': consequence})
    if result:
        write(out / 'declaration-coverage.tsv', list(result[0]), result)


def coverage(row, owners):
    structure, field = row['structure'], row['field']
    owner = owners.get(structure, 'gpt-format-gaps')
    if structure.startswith('NKR:') or structure.startswith('NICNT:'):
        return ('raw/unknown' if field in RAW or field == 'structure' else 'decoded-unused'), owner, 'archive metadata; access/content resolution is a separate path'
    if structure.startswith('NIS:'):
        unread = structure in {'NIS:NISD:0x79', 'NIS:NISD:0x6c'}
        return ('unread' if unread else 'raw/unknown'), owner, 'wrapper/property presence; no property playback law inferred'
    if field in RAW or field.startswith(('reserved', 'private[')) or field == 'structure':
        return 'raw/unknown', owner, 'bounded region/framing; bytes are not independently named parameters'
    if structure == 'Kontakt:0x47':
        return 'raw/unknown', owner, 'typed references/flags retained; native setting meanings and binding unestablished'
    if structure == 'Kontakt:0x4e':
        return 'raw/unknown', owner, 'known integer has no established audio behavior'
    if field in USED.get(structure, set()):
        return 'decoded+used', owner, 'used by baseline translator; native law/selection parity not implied'
    if structure == 'Kontakt:0x4b' and field.startswith('segment_kind_'):
        return 'decoded+used', owner, 'path identity feeds sample/IR resolution; anchor semantics may be flattened'
    if structure == 'Kontakt:0x4b' and field == 'sample_unknown_record':
        return 'raw/unknown', owner, 'sample record retained; semantic meaning unestablished'
    if field.startswith('persistent_slot_'):
        return 'decoded+used', owner, 'script restore grammar and slot scope owned by persistence family'
    if field in {'trailing_flag', 'unknown'}:
        return 'raw/unknown', owner, 'numeric encoding retained; meaning unestablished'
    return 'decoded-unused', owner, 'readable value; see family doc for runtime admission/unsupported consequence'


def generate(baseline, shards, out, registry, supplement=None):
    out.mkdir(parents=True, exist_ok=True)
    completed = sorted(p.parent for p in shards.glob('*/done')) if shards else []
    status = defaultdict(int)
    seen = set()
    for p in completed:
        for row in read(p / 'files.tsv'):
            assert row['path'] not in seen, 'overlapping census shards'
            seen.add(row['path'])
            status[row['status']] += 1
    if completed:
        assert {r['path'] for r in read(baseline / 'files.tsv')} <= seen, 'incomplete metadata census shards'
    if supplement:
        expected = {r['path'] for r in read(baseline / 'files.tsv') if r['path'].lower().endswith('.nkm')}
        supplied = read(supplement / 'files.tsv')
        assert len(supplied) == len(expected) and {r['path'] for r in supplied} == expected, 'supplement must cover each baseline multi once'
        assert all(r['status'] == 'ok' for r in supplied) and not read(supplement / 'errors.tsv'), 'failed multi-reference supplement'
    owners = {f"Kontakt:{r['id']}": r['owner'] for r in read(registry) if r['domain'] == 'Kontakt'}
    for filename, keys, values in [
        ('fields.tsv', ['structure', 'version', 'field'], ['files', 'nonbaseline_files', 'records']),
        ('byte-profile.tsv', ['structure', 'version', 'region', 'offset'], ['files', 'nonzero_files', 'records']),
    ]:
        original = counts([baseline / filename], keys, values)
        changed = counts([p / filename for p in completed], keys, values)
        if supplement:
            # The first metadata shards did not enter nested multi programs.
            # Add only their newly measured reference fields, not shared scopes.
            references = {'container_reference', 'snapshot_directory_reference', 'full_path_reference', 'wallpaper_reference', 'public_extension'}
            for key, row in counts([supplement / filename], keys, values).items():
                if row['structure'] != 'Kontakt:0x28' or row[keys[2]] not in references:
                    continue
                saved = changed.setdefault(key, {k: row[k] for k in keys} | {v: 0 for v in values})
                for value in values:
                    saved[value] += row[value]
        original.update(changed)  # shards are disjoint; their totals replace matching baseline observations
        rows = sorted(original.values(), key=lambda r: tuple(r[k] for k in keys))
        for row in rows:
            if row['structure'] == 'NIS:NIK4:0x4':
                row['version'] = 'BPatchHeaderV42'  # its leading magic is not a version
            elif row['structure'] == 'NIS:NIK4:0x3':
                row['version'] = 'inherited'
            elif row['structure'] == 'Kontakt:0x48' and row['version'] == '0xff00':
                row['version'] = 'unestablished:0xff00'
            assert 0 <= int(row[values[1]]) <= int(row['files']) <= int(row['records'])
        write(out / filename, keys + values, rows)
        if filename == 'fields.tsv':
            annotated = []
            for row in rows:
                state, owner, consequence = coverage(row, owners)
                annotated.append(row | {'status': state, 'owner': owner, 'consequence': consequence})
            write(out / 'field-impact.tsv', keys + values + ['status', 'owner', 'consequence'], annotated)
            declarations(rows, registry, owners, out)
    errors = counts([baseline / 'errors.tsv'], ['decoder'], ['files', 'records'])
    if completed:
        errors.pop(('save settings fields',), None)  # superseded by corrected reference reader
        errors.update(counts([p / 'errors.tsv' for p in completed], ['decoder'], ['files', 'records']))
    write(out / 'errors.tsv', ['decoder', 'files', 'records'], sorted(errors.values(), key=lambda r: r['decoder']))
    write(out / 'shard-status.tsv', ['status', 'files'], [{'status': k, 'files': v} for k, v in sorted(status.items())])


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('baseline', type=Path)
    parser.add_argument('--shards', type=Path)
    parser.add_argument('--out', type=Path, default=Path('docs/architecture-v2/kontakt-census'))
    parser.add_argument('--registry', type=Path, default=Path('docs/architecture-v2/KONTAKT_ID_REGISTRY.tsv'))
    parser.add_argument('--supplement', type=Path, help='targeted multi-program reference census')
    args = parser.parse_args()
    generate(args.baseline, args.shards, args.out, args.registry, args.supplement)
