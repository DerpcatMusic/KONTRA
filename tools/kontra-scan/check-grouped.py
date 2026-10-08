"""Failing-first contract: counts, ranking, bounded locations, full sidecar."""
from grouped_diagnostics import Collector, receipt_events, aggregate
import json
from pathlib import Path
c=Collector()
for i in range(40):
 c.add('ModTarget','TargetsDropped','pan',True,dict(path='/Libraries/Kontakt/A/a.nki',program=0,group=i,slot=1))
c.add('ModTarget','TargetsDropped','pan',False,dict(path='/Libraries/Kontakt/B/b.nki',program=1,group=9,slot=1))
c.add('ModTarget','SourceNotExecuted','pan',True,dict(path='/Libraries/Kontakt/A/a.nki',program=0,group=0,slot=1))
g=c.report(); assert len(g)==2
assert (g[0]['enabled'],g[0]['bypassed'],g[0]['libraries'],len(g[0]['locations']),g[0]['locations_total'])==(40,1,2,16,41)
assert len(c.sidecar())==42
r={'path':'/Libraries/Kontakt/A/a.nki','programs':[{'program':0,'dsp_slots':{'complete':True,'slots':[{'kind':'mod','scope':'group 208 internal','slot':0,'module':'LFO:5','enabled':True,'disposition':'dropped','reason':'SourceNotExecuted','targets':[{'ordinal':2,'enabled':False,'parameter':'pan','reason':'SourceNotExecuted','disposition':'dropped'}]}]}}]}
c=Collector()
for event in receipt_events(r): c.add(*event)
assert {x['key']['subject'] for x in c.report()}=={'LFO:5','pan'}
assert all(x['locations'][0]['group']==208 for x in c.report())
fixture=json.loads(Path(__file__).with_name('grouped-contract.json').read_text())
c=Collector()
for event in fixture['events']: c.add(**event)
assert c.report()==fixture['groups']
# Bank members retain separate locations without leaking member names.
bank=[dict(path='/Libraries/UVI/A.ufs::PRIVATE_ONE',programs=r['programs']),dict(path='/Libraries/UVI/A.ufs::PRIVATE_TWO',programs=r['programs'])]
c=aggregate(bank)
assert c.report()[0]['locations_total']==2
assert all('::' not in row['location']['path'] for row in c.sidecar())
assert 'PRIVATE_' not in json.dumps(c.sidecar())
# Unsafe target identifiers collapse to a typed unknown, rather than exposing text.
c.add('ModTarget','TargetsDropped','secret authored text',True,dict(path='/Libraries/UVI/A.ufs',program=0))
assert any(g['key']['subject']=='UnknownSubject' for g in c.report())
slots=r['programs'][0]['dsp_slots']['slots']
slots.append(dict(slots[0],scope='group 208 external'))
c=aggregate([r])
assert next(g for g in c.report() if g['key']['kind']=='ModTarget')['locations_total']==2, 'internal/external slots must keep distinct physical locations'
print('grouped diagnostics contract PASS')
