"""Synthetic RSS admission checks, without loading any plugin."""
from editor_rss import PHASES, summarize, author_state
from live_host import native_selection
from pathlib import Path
import struct
rows = [dict(phase=phase, sample=i, rss_kib=1024*(10+index), hwm_kib=20480,
             swap_kib=0, editor_children=int(phase in ('open','reopened')),
             width=1180 if phase in ('open','reopened') else 0,
             height=760 if phase in ('open','reopened') else 0)
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
