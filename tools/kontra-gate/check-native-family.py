import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent.parent/"kontra-scan"))
from native_family import compare

def fixture():
    return {"basis":"native-reader", "format":"kontakt", "script_driven":[],
        "program":{"low_key":0,"high_key":127,"low_velocity":1,"high_velocity":127,"default_switch":24,"group_solo":False},
        "groups":[{"id":i,"muted":False,"soloed":False,"reverse":False,"release":False,"channel":-1,
            "criteria":[{"mode":1,"key_min":24,"key_max":24,"next":0},{"mode":2,"controller":1,"cc_min":80,"cc_max":127,"next":0},{"mode":3,"cycle":i+1,"next":0}]} for i in range(2)],
        "zones":[{"id":i+1,"group":i,"low_key":60,"high_key":60,"low_velocity":64,"high_velocity":127,"start":48,"end":0,"frames":480,"mod_range":0,"loops":[]} for i in range(2)],
        "audition":{"key":60,"velocity":100,"switch":24,"cc":{"1":100,"11":127},"channel":0}}

def takes(ids):
    return [{"attack":[{"key":60,"trigger":"Attack","parent_event":None,"suppressed":False,
            "started":[{"source_zone":i,"frame":48,"direction":"Forward","loops":[]}]}],"release":[]} for i in ids]
native=fixture()
assert compare(native,takes([2,1]*16))["verdict"] == "MATCH"
native['audition']['held_key']=61
assert compare(native,takes([2,1]*16))['reason'] == 'multi-note-native-oracle-requires-capture'
native['audition'].pop('held_key')
assert compare(native,takes([1]*32))["reason"] == "round-robin-distribution-mismatch"
assert compare(native,takes([3]*32))["reason"] == "source-outside-native-candidates"
bad=takes([1,2]*16);bad[0]["attack"][0]["started"][0]["frame"]=0
assert compare(native,bad)["reason"] == "sample-start-mismatch"
bad=takes([1,2]*16);bad[0]["attack"][0]["started"][0]["direction"]="Reverse"
assert compare(native,bad)["reason"] == "direction-mismatch"
assert compare(native,takes([1,2]))["status"] == "UNKNOWN"
native["script_driven"]=["allow_group"]
assert compare(native,[])["verdict"] == "SCRIPT_DRIVEN"
assert compare(native,[])["script_driven_count"] == 1
native=fixture();native["groups"][0]["criteria"][1]["cc_min"]=110
assert compare(native,takes([1]*32))["reason"] == "source-outside-native-candidates"
native=fixture();native["zones"][1]["low_velocity"]=101
assert compare(native,takes([2]*32))["reason"] == "source-outside-native-candidates"
native=fixture();native["groups"][0]["criteria"]=[{"mode":0,"next":1},{"mode":1,"key_min":24,"key_max":24}]
ambiguous=compare(native,takes([1]*32))
assert ambiguous['verdict'] == 'UNKNOWN'
assert ambiguous['reason'] == 'native-start-interior-always-requires-capture'
# Interior Always has no documented native law under any join. Its presence
# cannot turn an otherwise complete 32-take receipt into MATCH or MISMATCH.
for join in [0,1,2]:
 native=fixture();native['groups'][0]['criteria']=[{'mode':0,'next':join},{'mode':1,'key_min':24,'key_max':24}]
 assert compare(native,takes([1]*32))['verdict'] == 'UNKNOWN'
native=fixture();native['groups'][0]['criteria']=[{'mode':1,'key_min':24,'key_max':24,'next':0},{'mode':0,'next':1},{'mode':2,'controller':1,'cc_min':0,'cc_max':127}]
assert compare(native,takes([1]*32))['reason'] == 'native-start-interior-always-requires-capture'
# The documented terminal marker and its incoming join are not predicates.
# Ordinary key/controller/cycle conditions still establish the same support.
native=fixture()
for group in native['groups']:
 group['criteria'][-1]['next']=1
 group['criteria'].append({'mode':0,'next':2})
assert compare(native,takes([2,1]*16))['verdict'] == 'MATCH'
# No criteria and a sole Always both mean ordinary unconditional admission.
for rows in [[],[{'mode':0,'next':1}]]:
 native=fixture();native['groups']=native['groups'][:1];native['zones']=native['zones'][:1]
 native['groups'][0]['criteria']=rows
 assert compare(native,takes([1]*32))['verdict'] == 'MATCH'
native=fixture();native["zones"][0]["loops"]=[{"slot":2,"mode":2,"start":80,"length":40,"count":0,"alternating":True,"tuning":1.0}]
assert compare(native,takes([1,2]*16))["reason"] == "loop-mismatch"
print("native candidate, cursor, cycle and script boundary checks passed")
