"""Compare stereo power of fresh v2 renders to pre-existing native captures.
Run with own cache path. Never writes audio or library payloads to the repository.
"""
import json, math, pathlib, struct, sys
import numpy as np

def wav(path):
    data=path.read_bytes(); offset=12; fmt=None
    while offset+8<=len(data):
        tag=data[offset:offset+4]; size=struct.unpack_from('<I',data,offset+4)[0]
        body=data[offset+8:offset+8+size]
        if tag==b'fmt ':
            fmt=struct.unpack_from('<HHIIHH',body)
            if fmt[0]==0xfffe:
                fmt=(struct.unpack_from('<I',body,24)[0],)+fmt[1:]
        if tag==b'data':
            code,ch,sr,_,_,bits=fmt
            dtype={(3,32):'<f4',(1,16):'<i2',(1,32):'<i4'}[code,bits]
            x=np.frombuffer(body,dtype=dtype).astype(np.float64).reshape(-1,ch)
            if code==1:x/=2**(bits-1)
            assert np.isfinite(x).all()
            return x,sr
        offset+=8+size+(size&1)
    raise ValueError('Missing WAV data')

def first_onset(x):
    a=np.abs(x).max(axis=1); found=np.flatnonzero(a>a.max()*10**(-50/20))
    return int(found[0]) if len(found) else 0

def power(x):return float(np.mean(x*x))

def db_ratio(a,b):return 10*math.log10(max(a,1e-20)/max(b,1e-20))

cache=pathlib.Path(sys.argv[1]); results=[]
for path in sorted(cache.glob('fresh-*.wav')):
    name=path.stem.removeprefix('fresh-')
    refdir=pathlib.Path('/home/derpcat/.cache/kontra-reference/probe')/name
    a,sr=wav(refdir/'kontakt.wav'); b,sr2=wav(path); assert sr==sr2
    ia,ib=first_onset(a),first_onset(b)
    # MIDI and native capture have the same grid; remove each file's first onset.
    grid=json.loads((refdir/'grid.json').read_text())['notes']; t0=grid[0]['t']
    errors=[]; attacks=[]; silent=0
    for n in grid:
        t=n['t']-t0
        sa=ia+round((t+.10)*sr); sb=ib+round((t+.10)*sr); size=round(.35*sr)
        xa=a[sa:sa+size]; xb=b[sb:sb+size]
        if len(xa)!=size or len(xb)!=len(xa):continue
        pa,pb=power(xa),power(xb)
        if pa<1e-10:continue
        if pb<1e-10:silent+=1
        errors.append(db_ratio(pb,pa))
        sa=ia+round(t*sr); sb=ib+round(t*sr); size=round(.02*sr)
        aa=a[sa:sa+size]; ab=b[sb:sb+size]
        attacks.append(db_ratio(power(ab)/max(pb,1e-20),power(aa)/pa))
    assert errors
    results.append({'name':name,'v2_baseline':'7e82b152','sample_rate':sr,'matched_grid_windows':len(errors),
        'native_audible_v2_silent':silent,'first_onset_delta_ms':1000*(ib-ia)/sr,
        'total_stereo_energy_db_ratio':db_ratio(float(np.sum(b*b)),float(np.sum(a*a))),
        'native_duration_seconds':len(a)/sr,'v2_duration_seconds':len(b)/sr,
        'steady_power_db_mean':float(np.mean(errors)),'steady_power_db_max_abs':max(map(abs,errors)),
        'relative_attack_db_mean_abs':float(np.mean(np.abs(attacks))),
        'sample_selection_not_isolated':True})
out=pathlib.Path(__file__).parent/'data/dsp-fresh-reference.json'
out.write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results,indent=2))
