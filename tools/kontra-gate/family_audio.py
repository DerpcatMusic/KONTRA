"""Compare host fingerprints as numeric leads; never certify native sample identity."""
import math
from collections import defaultdict


def validate(run):
    assert run.get('returncode') == 0 and run.get('native_state_verified') is True
    assert run.get('nonfinite') == 0 and run.get('underruns') == 0
    assert type(run.get('events_dispatched')) is int and run['events_dispatched'] == run.get('events_planned') > 0
    notes = run['note_audio']
    assert notes, 'per-note evidence absent'
    for note in notes:
        for key in ['event_index', 'key', 'velocity', 'start_frame', 'window_frames', 'length_frames']:
            assert type(note[key]) is int and note[key] >= 0
        assert note['key'] < 128 and 0 < note['velocity'] < 128
        assert type(note['onset_frame']) is int and -1 <= note['onset_frame'] < note['window_frames']
        assert note['length_frames'] <= note['window_frames']
        for x in [note['peak'], note['rms'], *note['spectrum']]:
            assert type(x) in (int, float) and math.isfinite(x) and x >= 0
        assert len(note['spectrum']) == 32
        if note['peak'] == 0:
            assert note['onset_frame'] == -1 and note['length_frames'] == 0 and note['rms'] == 0
        else:
            assert note['onset_frame'] >= 0 and note['length_frames'] > 0 and note['rms'] > 0
        assert math.isclose(sum(note['spectrum']), 1, abs_tol=1e-6) or sum(note['spectrum']) == 0
    return notes


def cosine(a, b):
    norm = math.sqrt(sum(x*x for x in a) * sum(x*x for x in b))
    return sum(x*y for x, y in zip(a, b)) / norm if norm else None


def compare(v1, v2):
    left, right = validate(v1), validate(v2)
    assert v1['audition_sha256'] == v2['audition_sha256'], 'different schedules'
    identity = lambda n: tuple(n[k] for k in ['event_index', 'key', 'velocity', 'start_frame', 'window_frames'])
    assert list(map(identity, left)) == list(map(identity, right)), 'incomplete note windows'
    rows, families = [], defaultdict(lambda: ([], []))
    for a, b in zip(left, right):
        rows.append(dict(event_index=a['event_index'], onset_delta_frames=b['onset_frame']-a['onset_frame'] if min(a['onset_frame'],b['onset_frame']) >= 0 else None,
                         length_delta_frames=b['length_frames']-a['length_frames'], spectral_cosine=cosine(a['spectrum'],b['spectrum']),
                         v1_silent=a['peak']==0, v2_silent=b['peak']==0))
        family = families[a['key'], a['velocity'], a['window_frames']]
        family[0].append(a['spectrum']); family[1].append(b['spectrum'])
    # RR is a distribution: compare its mean spectrum separately from ordered hits.
    mean = lambda spectra: [sum(band)/len(spectra) for band in zip(*spectra)]
    distribution = [dict(key=k[0], velocity=k[1], window_frames=k[2], repeats=len(a), mean_spectral_cosine=cosine(mean(a),mean(b)))
                    for k,(a,b) in families.items()]
    return dict(status='UNKNOWN', basis='host-audio-numeric-leads; native family identity unverified', per_note=rows, distributions=distribution)
