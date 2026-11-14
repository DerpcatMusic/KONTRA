# Loads every Kontakt .nki and every UVI program through `kontra-inspect load` (KONTRA_INSPECT_BIN) and
# ranks the `ignored …` diagnostics by instruments. Output: OUT/load.jsonl (resumable), then the table.
# usage: ignored-census.py OUT_DIR [top]
import json,os,subprocess,sys,tempfile,collections,concurrent.futures as cf
BIN=os.environ['KONTRA_INSPECT_BIN']; OUT=sys.argv[1]; TOP=int(sys.argv[2]) if len(sys.argv)>2 else 15
os.makedirs(OUT,exist_ok=True); path=os.path.join(OUT,'load.jsonl')
jobs=[]
for d,_,fs in os.walk('/data/main/Libraries/Kontakt'):
    jobs+=[('kontakt',os.path.join(d,f)) for f in fs if f.lower().endswith('.nki')]
for d,_,fs in os.walk('/data/main/Libraries/UVI'):
    for f in fs:
        if f.lower().endswith('.ufs'):
            b=os.path.join(d,f)
            r=subprocess.run([BIN,'programs',b],capture_output=True,text=True)
            jobs+=[('uvi',b+'/'+l.strip()) for l in r.stdout.splitlines() if l.strip().lower().endswith('.uvip')]
jobs.sort(); done=set()
if os.path.exists(path):
    for l in open(path):
        try: done.add(json.loads(l)['path'])
        except Exception: pass
todo=[j for j in jobs if j[1] not in done]
print(len(jobs),'instruments,',len(todo),'to do',flush=True)
def one(j):
    kind,p=j; row={'kind':kind,'path':p}
    tmp=tempfile.mktemp(dir=OUT,suffix='.json')
    try:
        subprocess.run(['nice','-n','19',BIN,'load',p,'--json',tmp],capture_output=True,timeout=300)
        row['diag']=[d for d in json.load(open(tmp))['result']['diagnostics'] if d['feature'].startswith('ignored ')]
    except Exception as e: row['error']=str(e)[:200]
    finally:
        if os.path.exists(tmp): os.remove(tmp)
    return row
with cf.ThreadPoolExecutor(3) as ex, open(path,'a') as out:
    for row in ex.map(one,todo): out.write(json.dumps(row)+'\n'); out.flush()
rows=[json.loads(l) for l in open(path)]
n=collections.Counter(); first={}; tot=collections.Counter()
for r in rows:
    for d in r.get('diag',[]):
        k=(r['kind'],d['feature']); n[k]+=1; first.setdefault(k,d['detail'])
print(sum('error' in r for r in rows),'errors of',len(rows))
for (k,f),c in n.most_common(TOP): print(c,k,f,first[(k,f)])
