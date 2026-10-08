"""Tier 1 oracle: raw native-reader records only, never the runtime selector.

Native key starts use the audition's latched switch, CC starts use its explicit
controller state. Cycle phase is deliberately unknown: compare support and the
whole repeated distribution, rather than blessing a particular first hit.
"""
from collections import Counter
from math import ceil


def result(verdict, reason, **evidence):
    return dict(status={'MATCH':'PASS', 'MISMATCH':'FAIL'}.get(verdict, 'UNKNOWN'),
                verdict=verdict, reason=reason, script_driven_count=int(verdict == 'SCRIPT_DRIVEN'), **evidence)


def group_allowed(group, plan, phase, default):
    if group.get('muted') or group.get('channel', -1) not in (-1, plan.get('channel', 0)):
        return False
    rows = group.get('criteria', [])
    def test(row):
        mode = row['mode']
        if row.get('sequencer_only'):
            raise ValueError('sequencer-start-requires-native-capture')
        if mode == 0:
            return True
        if mode == 1:
            switch = plan.get('switch')
            if switch is None:
                switch = default
            return switch is not None and row['key_min'] <= switch <= row['key_max']
        if mode == 2:
            value = plan.get('cc', {}).get(str(row['controller']), 0)
            return row['cc_min'] <= value <= row['cc_max']
        if mode == 3:
            return phase == row['cycle']
        raise ValueError('native-start-mode-unverified')
    if not rows:
        return True
    joins = {r.get('next', 0) for r in rows[:-1]}
    if len(joins) > 1:
        raise ValueError('native-start-mixed-precedence-unverified')
    active = test(rows[0])
    for left, right in zip(rows, rows[1:]):
        value = test(right)
        join = left.get('next', 0)
        if join == 0:
            active = active and value
        elif join == 1:
            active = active and not value
        elif join == 2:
            active = active or value
        else:
            raise ValueError('native-start-join-unverified')
    return active


def loop_signature(loops):
    return sorted((l['slot'], l['mode'], l['start'], l['length'], l['count'],
                   l['alternating'], l.get('crossfade', 0), l['tuning']) for l in loops if l['mode'] != 0)


def compare(native, takes):
    if not isinstance(native, dict) or native.get('basis') != 'native-reader':
        return result('UNKNOWN', 'independent-native-reader-absent')
    if native.get('script_driven'):
        return result('SCRIPT_DRIVEN', 'selection-beyond-static-native-data', native_calls=native['script_driven'])
    if native.get('unknown'):
        return result('UNKNOWN', native['unknown'])
    try:
        return _compare(native, takes)
    except (KeyError, TypeError, ValueError, OverflowError) as error:
        return result('UNKNOWN', str(error) if isinstance(error, ValueError) else 'malformed-native-or-executed-evidence')


def _compare(native, takes):
    plan, program = native['audition'], native.get('program', {})
    key, velocity = plan['key'], plan['velocity']
    if any(type(x) is not int or not 0 <= x <= 127 for x in (key, velocity)):
        raise ValueError('invalid-audition-midi')
    groups = {g['id']:g for g in native.get('groups', [])}
    length = max([r['cycle'] for g in groups.values() for r in g.get('criteria', []) if r['mode'] == 3] or [1])
    if type(length) is not int or not 1 <= length <= 128:
        raise ValueError('invalid-native-cycle-length')
    if len(takes) < max(32, length):
        return result('UNKNOWN', 'incomplete-repeated-evidence', repeats=len(takes), minimum_repeats=max(32, length))
    default = program.get('default_switch')
    zones = {z['id']:z for z in native['zones']}
    if len(zones) != len(native['zones']):
        raise ValueError('duplicate-native-source-id')
    phases, admitted = [], set()
    for phase in range(1, length + 1):
        candidates = []
        for source, zone in zones.items():
            if not (zone['low_key'] <= key <= zone['high_key'] and zone['low_velocity'] <= velocity <= zone['high_velocity']):
                continue
            if not (program.get('low_key', 0) <= key <= program.get('high_key', 127) and program.get('low_velocity', 1) <= velocity <= program.get('high_velocity', 127)):
                continue
            group = groups[zone['group']]
            if program.get('group_solo') and not group.get('soloed'):
                continue
            if not group_allowed(group, plan, phase, default):
                continue
            if zone.get('mod_range', 0) != 0 and zone.get('start_modulated') is not False:
                return result('UNKNOWN', 'native-start-modulation-requires-oracle', source_zone=source)
            trigger = 'Release' if group.get('release') else 'Attack'
            candidates.append((trigger, source))
            admitted.add(source)
        phases.append(tuple(sorted(candidates)))
    if not admitted:
        return result('UNKNOWN', 'native-audition-has-no-candidates')
    observed = Counter()
    for take in takes:
        signature = []
        for part in ('attack', 'release'):
            for record in take[part]:
                if record.get('parent_event') is not None:
                    return result('SCRIPT_DRIVEN', 'executed-generated-note-requires-native-capture')
                if record.get('suppressed'):
                    return result('MISMATCH', 'native-attack-unexpectedly-suppressed')
                for actual in record['started']:
                    source = actual['source_zone']
                    if source not in admitted:
                        return result('MISMATCH', 'source-outside-native-candidates', source_zone=source)
                    zone = zones[source]
                    group = groups[zone['group']]
                    reverse = group.get('reverse', False)
                    direction = 'Reverse' if reverse else 'Forward'
                    if actual['direction'] != direction:
                        return result('MISMATCH', 'direction-mismatch', source_zone=source)
                    if reverse and zone['frames'] <= 0:
                        return result('UNKNOWN', 'native-source-length-absent', source_zone=source)
                    frame = zone['frames'] + zone['end'] - 1 if reverse else zone['start']
                    if actual['frame'] != frame:
                        return result('MISMATCH', 'sample-start-mismatch', source_zone=source, expected_frame=frame, actual_frame=actual['frame'])
                    if 'loops' not in actual:
                        return result('UNKNOWN', 'executed-loop-evidence-absent')
                    if loop_signature(actual['loops']) != loop_signature(zone['loops']):
                        return result('MISMATCH', 'loop-mismatch', source_zone=source)
                    signature.append((record['trigger'], source))
        signature = tuple(sorted(signature))
        if signature not in phases:
            return result('MISMATCH', 'native-layer-or-cycle-membership-mismatch', actual_sources=[s for _, s in signature])
        observed[signature] += 1
    support = Counter(phases)
    # A deterministic cycle can start at any phase; each phase differs by at most
    # one visit at the window edges. Missing cycle members always fail.
    for signature, count in support.items():
        visits = observed[signature]
        expected = len(takes) * count / length
        if visits == 0 or abs(visits - expected) > ceil(count):
            return result('MISMATCH', 'round-robin-distribution-mismatch', repeats=len(takes), cycle_length=length)
    return result('MATCH', 'native-candidates-cursor-loops-and-cycle-match', repeats=len(takes), cycle_length=length,
                  expected_source_ids=sorted(admitted), observed_source_ids=sorted({s for signature in observed for _, s in signature}))
