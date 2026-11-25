# Plays one held C3 (key 60, CC1/CC11 full) of every Kontakt .nki/.nksn and UVI program under the library
# roots through kontra-inspect (KONTRA_INSPECT_BIN, a corpus or release build) and appends one JSON row
# per instrument to play.jsonl: diagnostics, unhandled script actions, unanswered engine pars, peak, RMS.
# Rows also carry load_s (read to on init) and each UI's build error or unsupported features.
# Resumable; 3 niced jobs. Then writes census-<date>.tsv next to it and prints failure classes ranked
# by instrument count. `--summary` only rewrites the table from play.jsonl; `--snapshot` also writes
# docs/parity/snapshots/corpus-census.tsv (pass counts per check, no library content) for the board.
# KONTRA_CENSUS_SCENARIO / KONTRA_CENSUS_OUT pick another scenario and output (census.faults.scenario.json:
# legato, repeats, pedal, velocities, keyswitch sweep 21-35, mod wheel, chord; rows then carry script faults).
# KONTRA_CENSUS_ROOTS (':'-separated) overrides the roots.
import json,os,subprocess,sys,concurrent.futures as cf,tempfile,time,collections,datetime
ART='/data/dev/workspace/PluginArtifacts/kontra-parity'
BIN=os.environ.get('KONTRA_INSPECT_BIN',os.path.join(ART,'kontra-inspect'))
OUT=os.environ.get('KONTRA_CENSUS_OUT',os.path.join(ART,'play.jsonl'))
SCENARIO=os.environ.get('KONTRA_CENSUS_SCENARIO',os.path.join(os.path.dirname(os.path.abspath(__file__)),'census.scenario.json'))
sys.path.insert(0,os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from kontra_libraries import KONTAKT,UVI
ROOTS=os.environ.get('KONTRA_CENSUS_ROOTS',f'{KONTAKT}:{UVI}').split(':')
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
    """A row for `p`. A silent C3 on a key the script leaves unmapped (outside its playable keys, as
    Kontakt colours them) is no failure: the row is replayed on the playable key nearest C3."""
    row=play(p,SCENARIO)
    keys=row.pop('playable_keys',None)
    if 'error' not in row and (row.get('rms') or 0)<SILENT and keys and 60 not in keys:
        key=min(keys,key=lambda k:abs(k-60))
        ev=json.load(open(SCENARIO))
        for e in ev['events']:
            if e.get('key')==60: e['key']=key
        fd,sc=tempfile.mkstemp(dir=os.path.dirname(OUT),suffix='.scenario.json')
        with os.fdopen(fd,'w') as f: json.dump(ev,f)
        try: row=play(p,sc); row.pop('playable_keys',None); row['key']=key
        finally: os.remove(sc)
    return row

def play(p,scenario):
    t=time.time()
    fd,tmp=tempfile.mkstemp(dir=os.path.dirname(OUT),suffix='.json'); os.close(fd)
    row={'path':p}
    try:
        r=subprocess.run(['nice','-n','19',BIN,'play',p,'--scenario',scenario,'--json',tmp],capture_output=True,timeout=300,env={**os.environ,'KONTRA_INSPECT_LEAN':'1'})
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
        row['load_s']=res['load_ms']/1e3 if res.get('load_ms') is not None else None
        row['ui']=res.get('ui_build',[])
        row['playable_keys']=res.get('playable_keys')
    except Exception as e:
        row['error']=str(e)[:200]
    finally:
        if os.path.exists(tmp): os.remove(tmp)
    row['s']=round(time.time()-t,1)
    return row

SAMPLE_DIAGS=('samples not found','sample-load')
# Fault counters that are no failure: Kontakt 8.13 runs on as KONTRA's VM does. A division by zero
# answers 0 (int / and mod) or inf (real /) (oracle probes 2026-11-12/14); an out-of-range write is
# dropped and a read gives 0 (#248).
BENIGN={'slot','divide_by_zero','first_divide_by_zero','out_of_range'}

def faulted(row):
    return any(isinstance(v,int) and k not in BENIGN and v>0 for f in row['faults'] for k,v in f.items())

def checks(row):
    """Per check, whether the row passes: loads, samples resolve, scripts run with no fault, every UI
    builds with nothing unsupported, the note sounds. None when the row predates the check."""
    if 'error' in row: return dict(loads=False,samples=False,script=False,ui=False,sound=False)
    ui=row.get('ui')
    return dict(loads=True,
        samples=not any(d.get('feature') in SAMPLE_DIAGS for d in row['diag']),
        script=not faulted(row)
            and not any(d.get('impact')=='Broken' and str(d.get('feature','')).startswith('script') for d in row['diag']),
        ui=None if ui is None else all('error' not in u and not u.get('unsupported') for u in ui),
        sound=(row.get('rms') or 0)>=SILENT)

def classes(row):
    """The failure classes a row shows: load error, silence, each audible diagnostic feature."""
    if 'error' in row: return ['load-error']
    c=[]
    if any(d.get('feature') in SAMPLE_DIAGS for d in row['diag']): c.append('samples-missing')
    for u in row.get('ui') or []:
        if 'error' in u: c.append('ui-error:'+u['error'][:60])
        c+=['ui-unsupported:'+k for k in u.get('unsupported',{})]
    if (row.get('rms') or 0)<SILENT: c.append('silent')
    for d in row['diag']:
        if d.get('impact')=='Audible': c.append('diag:'+str(d.get('feature')))
    c+=['unhandled:'+a for a,_ in row['unhandled']]
    c+=['par:'+str(p[0]) for p,_ in row['pars']]
    for f in row['faults']:
        c+=['fault:'+k for k,v in f.items() if isinstance(v,int) and k not in BENIGN and v>0]
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
    if '--snapshot' in sys.argv: snapshot(rows)

SNAPSHOT=os.path.join(os.path.dirname(os.path.abspath(__file__)),'../../docs/parity/snapshots/corpus-census.tsv')

def snapshot(rows):
    """Counts only, no library content: per format, how many instruments pass each check and all of them,
    and the load time's median and 95th percentile."""
    out=['format\tcheck\tpass\tinstruments']
    for fmt in ('Kontakt','UVI'):
        rs=[r for p,r in rows.items() if f'/{fmt}/' in p]
        cs=[checks(r) for r in rs]
        for k in ('loads','samples','script','ui','sound'):
            out.append(f'{fmt}\t{k}\t{sum(c[k] is True for c in cs)}\t{len(rs)}')
        out.append(f'{fmt}\tgreen\t{sum(all(c.values()) for c in cs)}\t{len(rs)}')
        t=sorted(r['load_s'] for r in rs if r.get('load_s') is not None)
        if t: out+=[f'{fmt}\tload_s_p50\t{t[len(t)//2]:.2f}\t{len(t)}',f'{fmt}\tload_s_p95\t{t[int(len(t)*.95)]:.2f}\t{len(t)}']
    open(SNAPSHOT,'w').write('\n'.join([out[0],*sorted(set(out[1:]))])+'\n')  # byte order, as the board requires
    print('->',os.path.normpath(SNAPSHOT))

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
