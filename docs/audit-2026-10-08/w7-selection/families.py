import json, collections, re, csv, sys
from pathlib import Path
for name in sys.argv[1:]:
    source=Path(name); d=json.loads(source.read_text()); tallies=collections.defaultdict(collections.Counter)
    names=list(d['notes'][0]['engines']); native=next((n for n in ['native','kontakt','native_b'] if n in names),names[-1])
    agree=total=0
    with source.with_name(source.stem+'-family.tsv').open('w') as f:
        w=csv.writer(f,delimiter='\t');w.writerow(['note','key','velocity','engine','family','rr','native_family','family_agrees'])
        for n in d['notes']:
            def split(e):
                picks=e.get('matches',[])
                if not picks:return ('unidentified','')
                s=Path(picks[0]['asset']).stem; rr=re.search(r'RR(\d+)',s)
                return (re.sub(r'RR\d+_\d+$','',s),rr.group(1) if rr else '')
            nf,_=split(n['engines'][native])
            for engine,e in n['engines'].items():
                family,rr=split(e); match=family==nf
                w.writerow([n['i'],n['key'],n['vel'],engine,family,rr,nf,int(match)])
                tallies[(engine,family)][rr]+=1
                if engine!=native:agree+=match;total+=1
    print(source.stem,agree,total)
    with source.with_name(source.stem+'-rr.tsv').open('w') as f:
        w=csv.writer(f,delimiter='\t');w.writerow(['engine','family','rr','count'])
        for (engine,family),counts in sorted(tallies.items()):
            for rr,count in sorted(counts.items()):w.writerow([engine,family,rr,count])
