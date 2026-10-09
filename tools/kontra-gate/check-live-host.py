"""Small trust-boundary and schedule checks before any real-library run."""
import struct
from live_host import events, v1_state, frozen_underruns, measured_status

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
template = b'OAST\x01\0\0\0' + b'\0' * 8 + struct.pack('<IQQ', 0, 0, 0)
native = v1_state(template, '/generated/tone.nki', 3)
assert native[:28] == template[:28] and len(native) > len(template)
assert frozen_underruns([]) is None
assert frozen_underruns([{'event': 'host_audio_config'}]) == 0
assert frozen_underruns([{'event': 'host_audio_config'}, {'code': 'playback_drops', 'data': {'state': {'underruns': 4}}}]) == 4
print('PASS: event plan, native v1-only envelope, unavailable diagnostics')

complete = {'returncode':0, 'events_dispatched':12, 'events_planned':12, 'peak':.5, 'nonfinite':0, 'contention':'QUIET', 'native_state_verified':True}
assert measured_status(complete) == 'MEASURED'
for change in [{'contention':'CONTENDED'}, {'contention':'UNKNOWN'}, {'peak':0}, {'nonfinite':1}, {'returncode':1}, {'events_dispatched':11}, {'native_state_verified':False}, {'native_state_verified':None}]:
    assert measured_status(dict(complete, **change)) == 'UNKNOWN'
print('PASS: silent, incomplete and contended runs cannot certify live playback')
audit = dict(complete, cpu_audit={'steady': {'blocks': 141}, 'steady_peak': .2}, underruns=0, profiled=False)
assert measured_status(audit) == 'MEASURED'
for change in [{'cpu_audit': {}}, {'cpu_audit': {'steady': {'blocks': 141}, 'steady_peak': 0}},
               {'underruns': None}, {'profiled': True}, {'profiled': True, 'profiler_exit_code': 1}]:
    assert measured_status(dict(audit, **change)) == 'UNKNOWN'
for code in (0, -2):
    assert measured_status(dict(audit, profiled=True, profiler_exit_code=code, profile_samples_steady=10)) == 'MEASURED'
assert measured_status(dict(audit, profiled=True, profiler_exit_code=0, profile_samples_steady=0)) == 'UNKNOWN'
print('PASS: audit requires audible steady window, underrun evidence and successful requested profiler')
from live_host import steady_leaf_samples
leaves = '\n'.join(f'100.{fraction}: 123 (fixture)' for fraction in ('249999999', '250000000', '999999999')) + '\n101.000000000: 123 (fixture)\n'
assert len(steady_leaf_samples(leaves, 100_000_000_000)) == 2
assert steady_leaf_samples(leaves + 'invalid timestamp\n', 100_000_000_000) == []
print('PASS: realtime leaf-IP profile excludes attack, release and teardown')

from pathlib import Path
import json, tempfile
from live_host import artifact_receipt, sha
with tempfile.TemporaryDirectory() as tmp:
    folder=Path(tmp); plugin=folder/'KONTRA.clap'; cli=folder/'kontakto'; host=folder/'host'
    for file in [plugin,cli,host]: file.write_bytes(file.name.encode())
    record={'source_sha':'a'*40,'profile':'ci','path':str(plugin),'sha256':sha(plugin),'cli_sha256':sha(cli),'host_sha256':sha(host)}
    (folder/'BUILD.json').write_text(json.dumps(record))
    assert artifact_receipt(plugin,cli,host)['source_sha']=='a'*40
    record['profile'] = 'release'
    (folder/'BUILD.json').write_text(json.dumps(record))
    assert artifact_receipt(plugin,cli,host)['profile'] == 'release'
    record['profile'] = 'unknown'
    (folder/'BUILD.json').write_text(json.dumps(record))
    try: artifact_receipt(plugin,cli,host)
    except AssertionError: pass
    else: raise AssertionError('unknown artifact profile accepted')
    record['profile'] = 'ci'
    (folder/'BUILD.json').write_text(json.dumps(record))
    plugin.write_bytes(b'changed')
    try: artifact_receipt(plugin,cli,host)
    except AssertionError: pass
    else: raise AssertionError('stale provenance accepted')
print('PASS: supplied artifact provenance follows the binary and rejects changed files')

from live_host import private_settings
with tempfile.TemporaryDirectory(dir='/dev/shm') as temp:
    config = Path(temp) / 'config'
    private_settings(config)
    assert json.loads((config / 'kontra/settings.json').read_text()) == {'version': 2, 'imported': True, 'roots': []}

# 0.3.152 native Selection omits root; keyed fields avoid positional shifts.
persist = native[36:]; at = 4 + 4 + len(b'selection')
selection = persist[at+8:]
assert struct.unpack_from('<I', selection)[0] == 0xffffff01
print('PASS: native keyed frame supported by frozen 0.3.152 derive')

from live_host import native_selection, verify_native_state
assert native_selection(native)[1] == (0,)
assert verify_native_state(native, native)
for saved in [template, native[:-1], native + b'\0', v1_state(template, '/generated/other.nki', 3),
              v1_state(template, '/generated/tone.nki', 0), native[:8] + b'changed!' + native[16:]]:
    assert not verify_native_state(native, saved)
# Additional native defaults may be saved; keyed field order is not identity.
from live_host import keyed, sized
import live_host
original = live_host.keyed
try:
    live_host.keyed = lambda fields: original(dict(reversed(list(fields.items()))))
    reordered = v1_state(template, '/generated/tone.nki', 3)
finally:
    live_host.keyed = original
assert verify_native_state(native, reordered)
for key, value in [('port', b'\1'), ('output', b'\1'), ('gain', struct.pack('<f', -6.))]:
    try:
        live_host.keyed = lambda fields: original(dict(fields, **({key: value} if key in fields else {})))
        changed = v1_state(template, '/generated/tone.nki', 3)
    finally:
        live_host.keyed = original
    assert not verify_native_state(native, changed)
print('PASS: saved native selection rejects default, wrong source/program and malformed frames')

from live_host import load_status
assert load_status([]) == 'WAITING'
assert load_status([{'event':'load_finished','data':{'status':'failed'}}]) == 'FAILED'
for state in ['loaded','partial']:
    assert load_status([{'event':'load_finished','data':{'status':state}}]) == 'READY'
print('PASS: load diagnostics fail fast for both plugin versions')
