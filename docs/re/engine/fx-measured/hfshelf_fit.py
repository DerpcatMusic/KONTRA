import numpy as np, glob, csv
FS=48000.0
PROBES=np.array([30,60,100,200,500,1000,2000,5000,10000,15000.])
def rbj_high_shelf_db(fc, G, S, f):
    A=10**(G/40); w0=2*np.pi*fc/FS; cs,sn=np.cos(w0),np.sin(w0)
    alpha=sn/2*np.sqrt((A+1/A)*(1/S-1)+2)
    b0=A*((A+1)+(A-1)*cs+2*np.sqrt(A)*alpha); b1=-2*A*((A-1)+(A+1)*cs); b2=A*((A+1)+(A-1)*cs-2*np.sqrt(A)*alpha)
    a0=(A+1)-(A-1)*cs+2*np.sqrt(A)*alpha; a1=2*((A-1)-(A+1)*cs); a2=(A+1)-(A-1)*cs-2*np.sqrt(A)*alpha
    z=np.exp(-1j*2*np.pi*f/FS); H=(b0+b1*z+b2*z*z)/(a0+a1*z+a2*z*z)
    return 20*np.log10(np.abs(H))
def nm(fun,x0,step,iters=600):
    n=len(x0); pts=[np.array(x0,float)]+[np.array(x0,float)+np.eye(n)[i]*step[i] for i in range(n)]
    vals=[fun(p) for p in pts]
    for _ in range(iters):
        o=np.argsort(vals); pts=[pts[i] for i in o]; vals=[vals[i] for i in o]
        c=np.mean(pts[:-1],axis=0); xr=c+(c-pts[-1]); fr=fun(xr)
        if fr<vals[0]:
            xe=c+2*(c-pts[-1]); fe=fun(xe)
            if fe<fr: pts[-1],vals[-1]=xe,fe
            else: pts[-1],vals[-1]=xr,fr
        elif fr<vals[-2]: pts[-1],vals[-1]=xr,fr
        else:
            xc=c+0.5*(pts[-1]-c); fc_=fun(xc)
            if fc_<vals[-1]: pts[-1],vals[-1]=xc,fc_
            else:
                pts=[pts[0]+0.5*(p-pts[0]) for p in pts]; vals=[fun(p) for p in pts]
    i=int(np.argmin(vals)); return pts[i], vals[i]
rows=[]
for line in open('/home/derpcat/.cache/kontra-fx-measure/hf_native_all.tsv'):
    c=line.rstrip('\n').split('\t'); rows.append((c[0],float(c[1]),float(c[2]),np.array([float(v) for v in c[3:13]])))
out=[]
for name,f,g,want in rows:
    mask=PROBES>=200
    def cost(p):
        fc,G=p
        if fc<20 or fc>20000: return 1e9
        model=rbj_high_shelf_db(fc,G,0.7,PROBES)
        return np.sqrt(np.mean((model[mask]-want[mask])**2))
    best=None
    for fc0 in (300,1000,3000,8000):
        for G0 in (6,18):
            p,v=nm(cost,[fc0,G0],[fc0*0.3,4])
            if best is None or v<best[1]: best=(p,v)
    out.append((name,f,g,best[0][0],best[0][1],best[1]))
    print(f"{name:34s} fc={best[0][0]:8.0f} G={best[0][1]:6.2f} rms200+={best[1]:.2f}")
np.save('/home/derpcat/.cache/kontra-fx-measure/fit/out.npy', np.array([(o[1],o[2],o[3],o[4],o[5]) for o in out]))
