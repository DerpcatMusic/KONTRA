import numpy as np, sys
sys.path.insert(0,'.')
from hfshelf_fit import rbj_high_shelf_db, PROBES
KN=np.linspace(0,1,11)
C5D08=np.array([-20,-18,-11,-6,-2,0,2,6,11,18,20.])
# measured mid-freq corner per gain knob (f = 0.5 normalised), plus inferred g=0 and g=0.25 rows
PTS_G=[0.0,0.25,0.5,0.75,0.875,0.9,0.95,1.0]; PTS_FC=[3800,2538,2400,2584,3162,3655,4101,4509]
FC_MID=np.interp(KN,PTS_G,PTS_FC)
print("FC_MID knots:", [round(v) for v in FC_MID])
rows=[l.rstrip('\n').split('\t') for l in open('/home/derpcat/.cache/kontra-fx-measure/hf_native_all.tsv')]
def law(f,g):
    G=np.interp(g,KN,C5D08); fc=max(np.interp(g,KN,FC_MID)*2*f,100.0)
    return fc,G
e_all=[];e_hi=[]
for c in rows:
    f,g=float(c[1]),float(c[2]); want=np.array([float(v) for v in c[3:13]])
    fc,G=law(f,g); m=rbj_high_shelf_db(fc,G,0.55,PROBES)
    e_all.append(np.sqrt(np.mean((m[PROBES>=200]-want[PROBES>=200])**2)))
    e_hi.append(np.max(np.abs(m[PROBES>=2000]-want[PROBES>=2000])))
e_all=np.array(e_all); e_hi=np.array(e_hi)
print(f"200Hz+ rms: median {np.median(e_all):.2f} max {e_all.max():.2f}; 2kHz+ max abs err: median {np.median(e_hi):.2f} max {e_hi.max():.2f}, rows over 1dB {(e_hi>1).sum()}/{len(e_hi)}")
for c,e in zip(rows,e_hi):
    if e>1: print(f"  {c[0]:34s} err2k+ {e:.2f}")
