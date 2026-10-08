"""Native-slot and independent-oracle verdicts for the shared release gate."""
from collections import Counter
import json

SLOT_METRICS = ('fx_slots_dropped', 'filter_slots_dropped', 'mod_slots_dropped')


def slot_count(value):
    if isinstance(value, str):
        try:
            value = json.loads(value)
        except ValueError:
            return None
    if not isinstance(value, dict) or any(type(value.get(k)) is not int or value[k] < 0 for k in ('enabled', 'bypassed')):
        return None
    return value


def slot_verdict(value):
    count = slot_count(value)
    return 'UNKNOWN' if count is None else 'FAIL' if any(count.values()) else 'PASS'


def family_match(observed, oracle):
    """Compare executed families with an independently supplied native oracle.

    Each take is a multiset (layer multiplicity matters). family includes native
    articulation, dynamic layer, offset and direction; rr is scored across all
    repeats. A probability tolerance must come from the oracle's native repeated
    measurement, not a hard-coded exact-hit test.
    """
    if not isinstance(oracle, dict) or oracle.get('basis') not in ('native-host', 'native-data-reviewed'):
        return {'status':'UNKNOWN', 'reason':'independent-native-oracle-absent'}
    if not oracle.get('complete') or len(observed) < oracle.get('minimum_repeats', 1):
        return {'status':'UNKNOWN', 'reason':'incomplete-repeated-evidence'}
    expected = Counter(oracle.get('families', []))
    if not expected:
        return {'status':'UNKNOWN', 'reason':'native-families-absent'}
    hist = Counter()
    for take in observed:
        if Counter(v['family'] for v in take) != expected:
            return {'status':'FAIL', 'reason':'articulation-dynamic-offset-direction-mismatch'}
        hist.update((v['family'], v.get('rr')) for v in take)
    for family, rr in oracle.get('rr', {}).items():
        n = sum(count for (f, _), count in hist.items() if f == family)
        for take, count in [(r, c) for (f, r), c in hist.items() if f == family]:
            if take not in rr['probabilities']:
                return {'status':'FAIL', 'reason':'round-robin-outside-native-support'}
        for take, probability in rr['probabilities'].items():
            if abs(hist[family, take] / n - probability) > rr['tolerance']:
                return {'status':'FAIL', 'reason':'round-robin-distribution-mismatch'}
    return {'status':'PASS', 'reason':'independent-native-families-and-distribution-match'}
