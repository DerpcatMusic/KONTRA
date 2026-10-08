"""Small trust-boundary and schedule checks before any real-library run."""
import struct
from live_host import events, keyed, v1_state, frozen_underruns, measured_status

plan = events({'key': 60, 'velocity': 64, 'keyswitch': 12}, 6)
assert plan[:3] == [(0, 0xb0, 1, 100), (0, 0xb0, 11, 127), (0, 0x90, 12, 64)]
assert (128, 0x80, 12, 0) in plan and (128, 0x90, 60, 64) in plan
assert all(a[0] <= b[0] for a, b in zip(plan, plan[1:]))
assert all(e[0] < 6 * 48000 for e in plan)
assert len([e for e in plan if e[1] == 0x90]) == 6
for bad in [{'key': 128, 'velocity': 64}, {'key': 60, 'velocity': 0}, {'key': 60, 'velocity': 64, 'keyswitch': -1}]:
    try: events(bad, 6)
    except AssertionError: pass
    else: raise AssertionError('invalid audition accepted')
assert struct.unpack_from('<II', keyed({'path': b'x'})) == (0xffffff01, 1)
template = b'OAST\x01\0\0\0' + b'\0' * 8 + struct.pack('<IQQ', 0, 0, 0)
native = v1_state(template, '/generated/tone.nki', 3)
assert native[:28] == template[:28] and len(native) > len(template)
assert frozen_underruns([]) is None
assert frozen_underruns([{'event': 'host_audio_config'}]) == 0
assert frozen_underruns([{'event': 'host_audio_config'}, {'code': 'playback_drops', 'data': {'state': {'underruns': 4}}}]) == 4
print('PASS: event plan, native v1-only envelope, unavailable diagnostics')

complete = {'returncode':0, 'events_dispatched':12, 'events_planned':12, 'peak':.5, 'nonfinite':0, 'contention':'QUIET'}
assert measured_status(complete) == 'MEASURED'
for change in [{'contention':'CONTENDED'}, {'contention':'UNKNOWN'}, {'peak':0}, {'nonfinite':1}, {'returncode':1}, {'events_dispatched':11}]:
    assert measured_status(dict(complete, **change)) == 'UNKNOWN'
print('PASS: silent, incomplete and contended runs cannot certify live playback')
