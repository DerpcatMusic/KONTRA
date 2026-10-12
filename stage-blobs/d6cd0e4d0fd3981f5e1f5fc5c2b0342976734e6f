"""Grouped diagnostics v1: typed identifiers and numeric/path locations only."""
import json
import hashlib
import re
from pathlib import Path

LOCATION_CAP = 16
IDENTIFIER = re.compile(r'(?:[A-Za-z_][A-Za-z_0-9:().-]{0,127}|@MIDI CC [0-9]{1,3}|@PitchBend|@VoiceParam (?:Key|Velocity))\Z')
LOCATION_FIELDS = ('path','program','group','slot','zone','target','script_slot','line','module_slot','item','callback_index','native_slot','rack','bus','node')


def library(path):
    path=path.split('::',1)[0]
    for marker in ('/Libraries/Kontakt/','/Libraries/UVI/'):
        if marker in path: return path.split(marker,1)[1].split('/',1)[0]
    return str(Path(path).parent)


class Collector:
    def __init__(self): self.groups={}
    def add(self,kind,reason,subject,enabled,location,count=1):
        assert all(IDENTIFIER.fullmatch(s) for s in (kind,reason)), 'untyped diagnostic key'
        if not isinstance(subject,str) or not IDENTIFIER.fullmatch(subject):subject='UnknownSubject'
        if subject.startswith('@MIDI CC ') and int(subject[9:])>=128:subject='UnknownSubject'
        assert type(count) is int and count>=0
        location={k:v for k,v in location.items() if k in LOCATION_FIELDS and v is not None}
        assert isinstance(location.get('path'),str)
        location['path']=location['path'].split('::',1)[0]
        assert all(type(v) is int and v>=0 for k,v in location.items() if k!='path')
        key=(kind,reason,subject); state='enabled' if enabled is True else 'bypassed' if enabled is False else 'unknown'
        group=self.groups.setdefault(key,dict(enabled=0,bypassed=0,unknown=0,locations={},libraries=set()))
        group[state]+=count;group['libraries'].add(library(location['path']))
        identity=json.dumps(location,sort_keys=True,separators=(',',':'))
        row=group['locations'].setdefault(identity,dict(location=location,enabled=0,bypassed=0,unknown=0));row[state]+=count
    def report(self):
        rows=[]
        for key,g in self.groups.items():
            rows.append(dict(schema='grouped-diagnostics-v1',key=dict(zip(('kind','reason','subject'),key)),
                enabled=g['enabled'],bypassed=g['bypassed'],unknown=g['unknown'],
                impact_rank=1 if g['enabled'] else 2 if g['bypassed'] else 3,
                libraries=len(g['libraries']),locations=[v['location'] for v in list(g['locations'].values())[:LOCATION_CAP]],
                locations_total=len(g['locations'])))
        return sorted(rows,key=lambda r:(r['impact_rank'],-r['enabled'],-r['libraries'],-r['bypassed'],tuple(r['key'].values())))
    def sidecar(self):
        return [dict(key=dict(zip(('kind','reason','subject'),key)),**v) for key,g in sorted(self.groups.items()) for v in g['locations'].values()]


def receipt_events(receipt):
    path=receipt['path']
    for pi,p in enumerate(receipt.get('programs',[])):
        base=dict(path=path,program=p.get('native_program',p.get('program',pi)),item=receipt.get('diagnostic_item_index'))
        for native_slot,s in enumerate(p.get('dsp_slots',{}).get('slots',[])):
            loc=dict(base,slot=s['slot'],native_slot=native_slot);scope=s.get('scope','')
            roles={'insert':0,'send':1,'main':2,'internal':3,'external':4}
            words=[w.lower() for w in scope.split() if w.lower() in roles]
            if words:loc['rack']=roles[words[-1]]
            node=re.fullmatch(r'(?:Program|Layer|Keygroup|connection):([0-9]+)',scope)
            if node:loc['node']=int(node[1])
            for field in ('group','zone','bus'):
                match=re.search(r'\b'+field+r' (\d+)\b',scope)
                if match:loc[field]=int(match[1])
            if s['disposition']!='implemented':
                yield ({'fx':'FxSlot','filter':'FilterSlot','mod':'ModSource'}[s['kind']],s['reason'],s['module'],s['enabled'],loc,1)
            for t in s.get('targets',[]):
                if t['disposition']!='implemented':
                    yield ('ModTarget',t['reason'],t['parameter'],t['enabled'],dict(loc,target=t['ordinal'],module_slot=t.get('module_slot')),1)
        for slot in p.get('ksp',{}).get('slots',[]):
            loc=dict(base,script_slot=slot['wire_slot'])
            for kind,fault in [('KspCompileFault',slot.get('compile_fault')),('KspInitFault',slot.get('init',{}).get('fault')),('KspPersistenceFault',slot.get('persistence_changed',{}).get('fault'))]:
                if fault:
                    yield (kind,fault.get('category','UnknownReason'),fault.get('builtin') or 'Callback',None if kind=='KspCompileFault' else True,dict(loc,line=fault.get('line')),1)
        for fault in p.get('ksp_runtime_faults',[]):
            yield ('KspFault',fault.get('core_error') or 'FuelExhausted','Callback',True,dict(base,callback_index=fault['program']),1)
        for phase in ('init','runtime'):
            count=(p.get('lua') or {}).get(phase+'_faults',0)
            if type(count) is int and count>0: yield ('Lua'+phase.title()+'Fault','RuntimeFault','Callback',True,base,count)
        # Historical caches do not identify source slots or bypass state.
        for feature,count in p.get('script_errors',{}).items():
            match=re.fullmatch(r'script (Unsupported|Approximate): ([A-Za-z_][A-Za-z_0-9]*)',feature)
            if match:
                reason='UnknownCommand' if match[1]=='Unsupported' else 'NativeLawUnverified'
                yield ('KspBuiltin',reason,match[2],None,base,count)


def aggregate(receipts):
    c=Collector()
    for r in sorted(receipts,key=lambda r:r['path']):
        # Stable numeric receipt identity distinguishes bank members without names.
        item=r.get('diagnostic_item_index',int(hashlib.sha256(r['path'].encode()).hexdigest()[:16],16))
        r=dict(r,diagnostic_item_index=item)
        for event in receipt_events(r):c.add(*event)
    return c
