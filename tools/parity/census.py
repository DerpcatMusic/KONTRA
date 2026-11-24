# Plays one held C3 (key 60, CC1/CC11 full) of every Kontakt .nki/.nksn and UVI program under the library
# roots through kontra-inspect (KONTRA_INSPECT_BIN, a corpus or release build) and appends one JSON row
# per instrument to play.jsonl: diagnostics, unhandled script actions, unanswered engine pars, peak, RMS.
# Resumable; 3 niced jobs. Then writes census-<date>.tsv next to it and prints failure classes ranked
# by instrument count. `--summary` only rewrites the table from play.jsonl.
# KONTRA_CENSUS_SCENARIO / KONTRA_CENSUS_OUT pick another scenario and output (census.faults.scenario.json:
# legato, repeats, pedal, velocities, keyswitch sweep 21-35, mod wheel, chord; rows then carry script faults).
# KONTRA_CENSUS_ROOTS (':'-separated) overrides the roots.
import json,os,subprocess,sys,concurrent.futures as cf,tempfile,time,collections,datetime
ART='/data/dev/workspace/PluginArtifacts/kontra-parity'
BIN=os.environ.get('KONTRA_INSPECT_BIN',os.path.join(ART,'kontra-inspect'))
OUT=os.environ.get('KONTRA_CENSUS_OUT',os.path.join(ART,'play.jsonl'))
SCENARIO=os.environ.get('KONTRA_CENSUS_SCENARIO',os.path.join(os.path.dirname(os.path.abspath(__file__)),'census.scenario.json'))
ROOTS=os.environ.get('KONTRA_CENSUS_ROOTS','/data/main/Libraries/Kontakt:/data/main/Libraries/UVI').split(':')
SILENT=1e-4  # RMS under -80 dBFS over the render

def instruments():
    files=[]
    for r in ROOTS:
        for d,_,fs in os.walk(r):
            for f in fs:
                p=os.path.join(d,f)
                if f.lower().endswith(('.nki','.nksn')): files.append(p)
                elif f.lower().endswith('.ufs'):
                    out=subprocess.run([BIN,'programs',p],capture_output=True,text=True).stdout.splitlines()
                    files+=[os.path.join(p,x) for x in out]
    return sorted(files)

def one(p):
    t=time.time()
    fd,tmp=tempfile.mkstemp(dir=os.path.dirname(OUT),suffix='.json'); os.close(fd)
    row={'path':p}
    try:
        r=subprocess.run(['nice','-n','19',BIN,'play',p,'--scenario',SCENARIO,'--json',tmp],capture_output=True,timeout=300,env={**os.environ,'KONTRA_INSPECT_LEAN':'1'})
        try: res=json.load(open(tmp))['result']
        except Exception: raise RuntimeError(r.stderr.decode(errors='replace').strip()[-200:] or f'exit {r.returncode}')
        row['diag']=res.get('diagnostics',[])
        row['unhandled']=res.get('unhandled_actions',[])
        row['pars']=res.get('unanswered_pars',[])
        row['faults']=res.get('script_faults',[])
        row['peak']=res['metrics']['peak']
        row['rms']=res['metrics'].get('rms')
        row['voices_left']=len(res.get('remaining_voices',[]))
        row['complete']=res.get('render_complete')
    except Exception as e:
        row['error']=str(e)[:200]
    finally:
        if os.path.exists(tmp): os.remove(tmp)
    row['s']=round(time.time()-t,1)
    return row

def classes(row):
    """The failure classes a row shows: load error, silence, each audible diagnostic feature."""
    if 'error' in row: return ['load-error']
    c=[]
    if (row.get('rms') or 0)<SILENT: c.append('silent')
    for d in row['diag']:
        if d.get('impact')=='Audible': c.append('diag:'+str(d.get('feature')))
    c+=['unhandled:'+a for a,_ in row['unhandled']]
    c+=['par:'+str(p[0]) for p,_ in row['pars']]
    for f in row['faults']:
        c+=['fault:'+k for k,v in f.items() if isinstance(v,int) and k!='slot' and v>0]
    return sorted(set(c))

def summary():
    rows={}
    for l in open(OUT):
        try: r=json.loads(l); rows[r['path']]=r
        except Exception: pass
    tsv=os.path.join(os.path.dirname(OUT),f'census-{datetime.date.today()}.tsv')
    with open(tsv,'w') as f:
        f.write('instrument\tloads\tsilent\trms\tpeak\tclasses\n')
        for p,r in sorted(rows.items()):
            loads='error' not in r
            f.write(f"{p}\t{int(loads)}\t{int(loads and (r.get('rms') or 0)<SILENT)}\t{r.get('rms','')}\t{r.get('peak','')}\t{','.join(classes(r))}\n")
    n=collections.Counter(c for r in rows.values() for c in classes(r))
    print(f'{len(rows)} instruments, {sum("error" not in r for r in rows.values())} load, {n["silent"]} silent -> {tsv}')
    for c,k in n.most_common(): print(f'{k:5d}  {c}')

if __name__=='__main__':
    os.makedirs(os.path.dirname(OUT),exist_ok=True)
    if '--summary' not in sys.argv:
        files=instruments()
        done=set()
        if os.path.exists(OUT):
            for l in open(OUT):
                try: done.add(json.loads(l)['path'])
                except Exception: pass
        todo=[f for f in files if f not in done]
        print(len(files),'instruments,',len(todo),'to do',flush=True)
        with cf.ThreadPoolExecutor(int(os.environ.get('KONTRA_CENSUS_JOBS','3'))) as ex, open(OUT,'a') as out:
            for row in ex.map(one,todo):
                out.write(json.dumps(row)+'\n');out.flush()
    summary()
