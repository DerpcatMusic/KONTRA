"""Synthetic RSS admission checks, without loading any plugin."""
from editor_rss import PHASES, summarize, author_state, selected_loads
from live_host import native_selection
from pathlib import Path
import struct
rows = [dict(phase=phase, sample=i, rss_kib=1024*(10+index), hwm_kib=20480,
             swap_kib=0, editor_children=int(phase in ('open','reopened')),
             width=1180 if phase in ('open','reopened') else 0,
             height=760 if phase in ('open','reopened') else 0,
             parent_width=1180, parent_height=760, clap_width=1180, clap_height=760)
        for index, phase in enumerate(PHASES) for i in range(10)]
result = summarize(rows)
assert result['open_delta_mib'] == 1 and result['close_delta_mib'] == 2 and result['reopen_delta_mib'] == 3
for bad in (rows[:-1], list(reversed(rows)), [dict(r, editor_children=0) for r in rows],
            [dict(r, rss_kib=True) for r in rows], [dict(r, hwm_kib=0) for r in rows]):
    try: summarize(bad)
    except AssertionError: pass
    else: raise AssertionError('incomplete/invisible/invalid RSS admitted')
print('PASS: four phases; rejection of missing/reordered/invisible/invalid samples')

# Minimal native template; Big Screen's two embedded programs survive state framing.
template=b'OAST\x01\0\0\0'+bytes(8)+struct.pack('<IQQ',0,0,4)+bytes(4)
state, programs=author_state(template,Path('Big Screen.nkm'))
parts, order=native_selection(state)
assert programs==[0,1] and order==(0,1)
assert [struct.unpack('<I',part[1])[0] for part in parts]==[0,1]
print('PASS: full two-part native multi identity')

def row(path,program,status): return dict(event='load_finished',path=path,program=program,data={'status':status})
assert selected_loads([row('other',0,'loaded')],Path('selected'),[0]) == {}
assert selected_loads([row('selected',0,'partial'),row('selected',0,'loaded')],Path('selected'),[0,1]) == {0:'loaded'}
assert selected_loads([row('selected',0,'loaded'),row('selected',1,'partial')],Path('selected'),[0,1]) == {0:'loaded',1:'partial'}
print('PASS: selected source and unique-program readiness; duplicate loads cannot admit incomplete multi')

assert summarize([dict(r,width=1181,height=761,parent_width=1181,parent_height=761,clap_width=1181,clap_height=761) if r['editor_children'] else r for r in rows])
for changes in (dict(parent_width=1181),dict(clap_height=761),dict(width=1182,parent_width=1182,clap_width=1182)):
    try: summarize([dict(r,**changes) if r['editor_children'] else r for r in rows])
    except AssertionError: pass
    else: raise AssertionError('clipped or oversized viewport admitted')
print('PASS: compositor +1 admitted; parent/child/CLAP mismatch and +2 rejected')
