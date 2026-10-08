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

complete = {'returncode':0, 'events_dispatched':12, 'events_planned':12, 'peak':.5, 'nonfinite':0, 'contention':'QUIET'}
assert measured_status(complete) == 'MEASURED'
for change in [{'contention':'CONTENDED'}, {'contention':'UNKNOWN'}, {'peak':0}, {'nonfinite':1}, {'returncode':1}, {'events_dispatched':11}]:
    assert measured_status(dict(complete, **change)) == 'UNKNOWN'
print('PASS: silent, incomplete and contended runs cannot certify live playback')

from pathlib import Path
import json, tempfile
from live_host import artifact_receipt, sha
with tempfile.TemporaryDirectory() as tmp:
    folder=Path(tmp); plugin=folder/'KONTRA.clap'; cli=folder/'kontakto'; host=folder/'host'
    for file in [plugin,cli,host]: file.write_bytes(file.name.encode())
    record={'source_sha':'a'*40,'profile':'ci','path':str(plugin),'sha256':sha(plugin),'cli_sha256':sha(cli),'host_sha256':sha(host)}
    (folder/'BUILD.json').write_text(json.dumps(record))
    assert artifact_receipt(plugin,cli,host)['source_sha']=='a'*40
    plugin.write_bytes(b'changed')
    try: artifact_receipt(plugin,cli,host)
    except AssertionError: pass
    else: raise AssertionError('stale provenance accepted')
print('PASS: supplied artifact provenance follows the binary and rejects changed files')

from live_host import private_settings
with tempfile.TemporaryDirectory(dir='/dev/shm') as temp:
    config = Path(temp) / 'config'
    private_settings(config)
    assert json.loads((config / 'kontra/settings.json').read_text()) == {'imported': True, 'roots': []}

# 0.3.152 native Selection omits root; keyed fields avoid positional shifts.
persist = native[36:]; at = 4 + 4 + len(b'selection')
selection = persist[at+8:]
assert struct.unpack_from('<I', selection)[0] == 0xffffff01
print('PASS: native keyed frame supported by frozen 0.3.152 derive')

from live_host import load_status
assert load_status([]) == 'WAITING'
assert load_status([{'event':'load_finished','data':{'status':'failed'}}]) == 'FAILED'
for state in ['loaded','partial']:
    assert load_status([{'event':'load_finished','data':{'status':state}}]) == 'READY'
print('PASS: load diagnostics fail fast for both plugin versions')
