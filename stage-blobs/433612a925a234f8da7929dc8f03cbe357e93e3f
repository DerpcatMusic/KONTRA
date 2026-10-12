#!/usr/bin/env python3
"""Publish identifier/widget/sigil incidence from current shared scanner metrics only."""
from collections import defaultdict, Counter
import csv, hashlib, importlib.util, json, sys
from pathlib import Path
root=Path(sys.argv[1] if len(sys.argv)>1 else str(Path.home()/'.cache/kontra-scan'))
spec=importlib.util.spec_from_file_location('scanner',Path(__file__).with_name('kontra_scan.py'));s=importlib.util.module_from_spec(spec);spec.loader.exec_module(s)
revisions={v:hashlib.sha256((root/'bin'/('kontra-scan-'+v)).read_bytes()).hexdigest() for v in ['v1','v2']}
records={v:{} for v in revisions}
for v in records:
 for file in (root/'results'/v/'cache').glob('*.json'):
  r=json.loads(file.read_text())
  if r.get('revision')==revisions[v] and file.stem==s.signature(r['path'],r['revision']):records[v][r['path']]=r
rs=[r for r in records['v2'].values() if '::' not in r['path']]
whitelist=Path(__file__).with_name('ui-symbols.txt').read_text().splitlines()
lexical=sum(r.get('metadata',{}).get('symbol_whitelist_count')==len(whitelist) for r in rs)
parsed=sum(isinstance(r.get('metadata',{}).get('slots'),list) for r in rs)
slots=[(r,x) for r in rs for x in r.get('metadata',{}).get('slots',[])]
a=defaultdict(set);b=defaultdict(set);programs=defaultdict(set);bp=defaultdict(set);ac=Counter();bc=Counter();widgets=defaultdict(set);wc=Counter();sigils=Counter();sigil_items=defaultdict(set);sigil_scopes=defaultdict(set)
for r,x in slots:
 scope=(r['path'],x.get('owner'),x.get('program_index'));target=b if x.get('bypassed') else a;scopes=bp if x.get('bypassed') else programs;counts=bc if x.get('bypassed') else ac
 for name,n in x.get('symbols',{}).items():target[name].add(r['path']);scopes[name].add(scope);counts[name]+=n
 if x.get('saved_histogram_complete',x.get('saved_table_integrity') in ['decoded','absent']):
  for name,n in x.get('raw_saved_entries_by_sigil',{}).items():sigils[name]+=n;sigil_items[name].add(r['path']);sigil_scopes[name].add(scope)
for r in rs:
 for p in r.get('programs',[]):
  for view in p.get('views',[]):
   for name,n in view.get('kinds',{}).items():
    if n:widgets[name].add(r['path']);wc[name]+=n
out=root/'results/v2/symbol-aggregates.tsv';tmp=out.with_suffix('.tmp');out.parent.mkdir(parents=True,exist_ok=True)
headers=['identifier','kind','instruments','program_owner_scopes','active_instruments','bypassed_instruments','active_occurrences','bypassed_occurrences','nki_active','nkm_active','initialized_widget_instruments','initialized_widgets','coverage_attempted','coverage_metadata_parsed','coverage_lexical_measured','raw_slot_records','raw_slots_decoded','scanner_sha256']
with tmp.open('w') as f:
 w=csv.writer(f,delimiter='\t',lineterminator='\n');w.writerow(headers)
 for name in whitelist:
  w.writerow([name,'public-ui-identifier',len(a[name]|b[name]),len(programs[name]|bp[name]),len(a[name]),len(b[name]),ac[name] if lexical==len(rs) else 'unknown',bc[name] if lexical==len(rs) else 'unknown',sum(p.lower().endswith('.nki') for p in a[name]),sum(p.lower().endswith('.nkm') for p in a[name]),len(widgets[name]),wc[name],len(rs),parsed,lexical,len(slots),sum(x.get('raw_category')!='decode_failed' for _,x in slots),revisions['v2']])
 for sigil in ['$','~','%','?','@','!','empty','other']:
  w.writerow(['sigil:'+sigil,'raw-saved-entry',len(sigil_items[sigil]),len(sigil_scopes[sigil]),'unknown','unknown',sigils[sigil],'unknown','unknown','unknown','n/a','n/a',len(rs),parsed,lexical,len(slots),sum(x.get('raw_category')!='decode_failed' for _,x in slots),revisions['v2']])
tmp.replace(out)
# This is an Original-UI regression list, separate from importer admission failures.
missing=root/'v1ok-v2missing.tsv';tmp=missing.with_suffix('.tmp')
with tmp.open('w') as f:
 w=csv.writer(f,delimiter='\t',lineterminator='\n');w.writerow(['id','path','resource_class'])
 for path,r in sorted(records['v1'].items()):
  v2=records['v2'].get(path)
  if r.get('ui')=='original-ok' and v2 and v2.get('ui') in ['missing-images','error','blank','budget-hit']:
   views=[v for p in v2.get('programs',[]) for v in p.get('views',[])]
   kind='image' if any(v.get('missing_images',0) for v in views) else 'unknown'
   w.writerow([path,path.split('::',1)[0],kind])
tmp.replace(missing)
print(f'{len(rs)}/834 Kontakt metadata attempted; {parsed} parsed; aggregates: {out}')
