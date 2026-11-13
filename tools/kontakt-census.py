# Plays one note (CC1/CC11 full) of every .nki under /data/main/Libraries/Kontakt through kontra-inspect
# (KONTRA_INSPECT_BIN, a corpus or release build) and appends one JSON row per instrument to play.jsonl:
# diagnostics, unhandled script actions, unanswered engine pars, peak. Resumable; 3 niced jobs.
# KONTRA_CENSUS_SCENARIO / KONTRA_CENSUS_OUT pick another scenario and output (kontakt-census.faults.scenario.json:
# legato, repeats, pedal, velocities, keyswitch sweep 21-35, mod wheel, chord; rows then carry script faults).
# census.json scenario: cc1=127, cc11=127, key 60 vel 100 at frame 2000, off at 50000.
import json,os,subprocess,sys,concurrent.futures as cf,tempfile,time
BIN=os.environ.get('KONTRA_INSPECT_BIN','/data/dev/workspace/PluginArtifacts/kontakt-census-run/inspect-census')
OUT=os.environ.get('KONTRA_CENSUS_OUT','/data/dev/workspace/PluginArtifacts/kontakt-census-run/play.jsonl')
SCENARIO=os.environ.get('KONTRA_CENSUS_SCENARIO',os.path.join(os.path.dirname(os.path.abspath(__file__)),'kontakt-census.scenario.json'))
roots=['/data/main/Libraries/Kontakt']
files=[]
for r in roots:
    for d,_,fs in os.walk(r):
        for f in fs:
            if f.lower().endswith('.nki'): files.append(os.path.join(d,f))
files.sort()
done=set()
if os.path.exists(OUT):
    for l in open(OUT):
        try: done.add(json.loads(l)['path'])
        except Exception: pass
todo=[f for f in files if f not in done]
print(len(files),'instruments,',len(todo),'to do',flush=True)
def one(p):
    t=time.time()
    tmp=tempfile.mktemp(dir='/data/dev/workspace/PluginArtifacts/kontakt-census-run',suffix='.json')
    row={'path':p}
    try:
        r=subprocess.run(['nice','-n','19',BIN,'play',p,'--scenario',SCENARIO,'--json',tmp],capture_output=True,timeout=300,env={**os.environ,'KONTRA_INSPECT_LEAN':'1'})
        res=json.load(open(tmp))['result']
        row['diag']=res.get('diagnostics',[])
        row['unhandled']=res.get('unhandled_actions',[])
        row['pars']=res.get('unanswered_pars',[])
        row['faults']=res.get('script_faults',[])
        row['peak']=res['metrics']['peak']
        row['voices_left']=len(res.get('remaining_voices',[]))
        row['complete']=res.get('render_complete')
    except Exception as e:
        row['error']=str(e)[:200]
    finally:
        if os.path.exists(tmp): os.remove(tmp)
    row['s']=round(time.time()-t,1)
    return row
with cf.ThreadPoolExecutor(3) as ex, open(OUT,'a') as out:
    for row in ex.map(one,todo):
        out.write(json.dumps(row)+'\n');out.flush()
print('done',flush=True)
