from copy import deepcopy
from family_audio import compare as compare_runs

def compare(a,b):
    a=dict(a,version='v1',plugin_sha256='20ff6b471069d6891d2847e72a4f863db5b50ce24265fda82d083b713b3496de',host_sha256='host',native_selection_sha256='source')
    b=dict(b,version='v2',host_sha256='host',native_selection_sha256='source')
    return compare_runs(a,b)

note = dict(event_index=0,key=60,velocity=64,start_frame=0,window_frames=48000,onset_frame=48,length_frames=23952,peak=1,rms=.5,spectrum=[1.]+[0.]*31)
base = dict(returncode=0,native_state_verified=True,nonfinite=0,underruns=0,events_dispatched=2,events_planned=2,audition_sha256='same',note_audio=[note],family_audio=deepcopy(note))
assert compare(base,base)['per_note'][0]['spectral_cosine']==1
assert compare(base,base)['full_instrument']['rms_ratio']==1
assert compare(base,base)['full_instrument']['spectral_cosine']==1
changed=deepcopy(base);changed['note_audio'][0]['spectrum']=[0.,1.]+[0.]*30
assert compare(base,changed)['distributions'][0]['mean_spectral_cosine']==0
changed=deepcopy(base);changed['note_audio'][0].update(onset_frame=-1,length_frames=0,peak=0,rms=0,spectrum=[0.]*32)
assert compare(base,changed)['per_note'][0]['spectral_cosine'] is None
assert compare(base,changed)['family_identity']=='UNKNOWN'
for change in [dict(audition_sha256='other'),dict(underruns=1),dict(native_state_verified=False),dict(note_audio=[]),dict(events_dispatched=1)]:
 changed=deepcopy(base);changed.update(change)
 try:compare(base,changed)
 except AssertionError:pass
 else:raise AssertionError('invalid receipt admitted')
changed=deepcopy(base);changed['note_audio'][0]['spectrum'][0]=float('nan')
try:compare(base,changed)
except AssertionError:pass
else:raise AssertionError('nonfinite fingerprint admitted')
# Swapping RR hits changes ordered similarity but preserves distribution.
a=deepcopy(base);b=deepcopy(base)
a['note_audio']=[deepcopy(note),deepcopy(note)];a['note_audio'][1].update(event_index=2,start_frame=48000,spectrum=[0.,1.]+[0.]*30)
b['note_audio']=deepcopy(a['note_audio']);b['note_audio'][0]['spectrum'],b['note_audio'][1]['spectrum']=b['note_audio'][1]['spectrum'],b['note_audio'][0]['spectrum']
assert compare(a,b)['distributions'][0]['mean_spectral_cosine']>0.999999
print('PASS: identity, silence, nonfinite, complete schedules, RR distributions')

from live_host import measured_status
assert measured_status(dict(base, family_observation=True))=='UNKNOWN'

left=dict(base,version='v1',plugin_sha256='20ff6b471069d6891d2847e72a4f863db5b50ce24265fda82d083b713b3496de',host_sha256='host',native_selection_sha256='source')
right=dict(base,version='v2',host_sha256='host',native_selection_sha256='source')
for change in [dict(version='v1'),dict(host_sha256='other'),dict(native_selection_sha256='other')]:
 try:compare_runs(left,dict(right,**change))
 except AssertionError:pass
 else:raise AssertionError('unmatched source/host/version admitted')
try:compare_runs(dict(left,plugin_sha256='other'),right)
except AssertionError:pass
else:raise AssertionError('unfrozen v1 admitted')
