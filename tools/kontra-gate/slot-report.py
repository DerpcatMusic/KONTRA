#!/usr/bin/env python3
"""Aggregate stable metadata receipts; target losses keep actual per-route outcomes."""
from collections import defaultdict
import json
from pathlib import Path
import sys
from fidelity import SLOT_METRICS, slot_count
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "kontra-scan"))
from grouped_diagnostics import aggregate


def summarize(receipts):
    slots, targets = defaultdict(lambda:dict(enabled=0,bypassed=0)), defaultdict(lambda:dict(enabled=0,bypassed=0))
    counts={metric:dict(enabled=0,bypassed=0) for metric in SLOT_METRICS}
    complete=bool(receipts); target_complete=bool(receipts); rows=[]
    for receipt in receipts:
        programs=receipt.get('programs',[])
        item_complete = bool(programs) and all(p.get('loaded',True) is True and p.get('dsp_slots',{}).get('complete') is True and
            all(slot_count(p.get('dsp_slots',{}).get('counts',{}).get(k)) is not None for k in SLOT_METRICS) for p in programs)
        complete &= item_complete
        item_counts={k:dict(enabled=0,bypassed=0) for k in counts}
        for program in programs:
            inventory=program.get('dsp_slots',{})
            for key in SLOT_METRICS:
                count=slot_count(inventory.get('counts',{}).get(key))
                if count is None: continue
                for state in ('enabled','bypassed'):
                    counts[key][state]+=count[state];item_counts[key][state]+=count[state]
            for slot in inventory.get('slots',[]):
                if slot['disposition']=='dropped':
                    slots[slot['kind'],slot['module'],slot['reason']]['enabled' if slot['enabled'] else 'bypassed']+=1
                target_complete &= 'targets' in slot
                for target in slot.get('targets',[]):
                    if target['disposition']=='dropped':
                        targets[target['parameter'],target['reason']]['enabled' if target['enabled'] else 'bypassed']+=1
        rows.append(dict(path=receipt['path'],engine_sha256=receipt.get('engine_sha256'),counts=item_counts if item_complete else None,programs=len(programs)))
    slot_rows=[dict(kind=k,module=m,reason=r,**c) for (k,m,r),c in slots.items()]
    target_rows=[dict(parameter=p,reason=r,**c) for (p,r),c in targets.items()]
    rank=lambda r:(-r['enabled'],-r['bypassed'],r.get('parameter',r.get('kind','')),r.get('module') or '',r['reason'])
    return dict(items=len(receipts),complete=complete,target_complete=target_complete and complete,counts=counts,
                rows=sorted(rows,key=lambda r:r['path']),slots=sorted(slot_rows,key=rank),targets=sorted(target_rows,key=rank),
                diagnostic_groups=aggregate(receipts).report())


if __name__=='__main__':
    receipts=[json.loads(p.read_text()) for p in sorted(Path(sys.argv[1]).glob('*.json'))]
    result=summarize(receipts)
    if len(sys.argv)>2:
        sidecar=Path(sys.argv[2]); sidecar.write_text(json.dumps({'schema':'grouped-diagnostics-v1','locations':aggregate(receipts).sidecar()},separators=(',',':'))+'\n')
        result['diagnostic_locations_sidecar']=str(sidecar)
    print(json.dumps(result,indent=2))
