#!/usr/bin/env python3
"""Resumable small-shard native-slot census; launch each shard with kontakto-heavy."""
import argparse, hashlib, json, subprocess, time
from pathlib import Path
p=argparse.ArgumentParser()
p.add_argument('--engine',required=True);p.add_argument('--list',required=True);p.add_argument('--out',required=True)
p.add_argument('--start',type=int,default=0);p.add_argument('--count',type=int,default=5)
a=p.parse_args();out=Path(a.out);out.mkdir(parents=True,exist_ok=True)
items=[line.split('\t',1)[-1] for line in Path(a.list).read_text().splitlines() if line and not line.startswith('#')]
engine_sha=hashlib.sha256(Path(a.engine).read_bytes()).hexdigest()
shard_started=time.monotonic()
for item in items[a.start:a.start+a.count]:
    # Leave at most one 80-second worker after 200 seconds to keep FIFO shards under five minutes.
    if time.monotonic()-shard_started>=200:
        print('Yielded native-reader shard for the next FIFO job',flush=True)
        break
    if (Path.home()/'.cache/kontra-quiet-request').exists():
        print('Paused for quiet request before next native-reader worker',flush=True)
        break
    target=out/(hashlib.sha256(item.encode()).hexdigest()+'.json')
    if target.exists() and json.loads(target.read_text()).get('engine_sha256')==engine_sha: continue
    try:
        run=subprocess.run([a.engine,item],capture_output=True,timeout=80,text=True)
        # Never persist stderr, which can contain native private details.
        result=json.loads(run.stdout) if run.returncode==0 else {'programs':[], 'error':'native-slot-census-failed', 'exit':run.returncode}
    except (subprocess.TimeoutExpired,json.JSONDecodeError):
        result={'programs':[], 'error':'native-slot-census-timeout-or-invalid-output'}
    result['path']=item.split('::',1)[0]
    result['diagnostic_item_index']=int(hashlib.sha256(item.encode()).hexdigest()[:16],16)
    result['engine_sha256']=engine_sha
    target.write_text(json.dumps(result,separators=(',',':'))+'\n')
    counts={key:{k:sum(program['dsp_slots']['counts'][key][k] for program in result['programs']) for k in ('enabled','bypassed')}
            for key in ('fx_slots_dropped','filter_slots_dropped','mod_slots_dropped')} if result['programs'] and all(p.get('dsp_slots',{}).get('complete') for p in result['programs']) else 'UNKNOWN'
    print(json.dumps({'item':Path(item.split('::')[0]).name,'counts':counts}),flush=True)
