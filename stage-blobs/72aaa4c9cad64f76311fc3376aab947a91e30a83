import importlib.util
from pathlib import Path
s=importlib.util.spec_from_file_location('slot_report',Path(__file__).with_name('slot-report.py'))
m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
fixture={'path':'instrument','programs':[{'dsp_slots':{'complete':True,'counts':{
 'fx_slots_dropped':{'enabled':0,'bypassed':0},'filter_slots_dropped':{'enabled':0,'bypassed':0},'mod_slots_dropped':{'enabled':1,'bypassed':0}},
 'slots':[{'kind':'mod','module':'Constant','reason':'TargetsDropped','disposition':'dropped','enabled':True,'targets':[
  {'parameter':'filterCutoff','module':'Filter:52','enabled':True,'disposition':'approximated','reason':'NativeLawUnverified'},
  {'parameter':'eqGain1','module':'EQ:v91','enabled':True,'disposition':'dropped','reason':'TargetsDropped'},
  {'parameter':'pan','module':None,'enabled':False,'disposition':'dropped','reason':'SavedBypassNotInstantiated'}]}]}}]}
r=m.summarize([fixture]);assert r['target_complete'] and r['complete']
assert r['targets']==[{'parameter':'eqGain1','reason':'TargetsDropped','enabled':1,'bypassed':0},
 {'parameter':'pan','reason':'SavedBypassNotInstantiated','enabled':0,'bypassed':1}]
assert r['counts']['mod_slots_dropped']=={'enabled':1,'bypassed':0}
fixture['programs'][0]['dsp_slots']['slots'][0].pop('targets')
assert not m.summarize([fixture])['target_complete']
assert not m.summarize([])['complete']
print('stable slot and per-target tally checks passed')
bad={'path':'broken','programs':[{'loaded':False,'dsp_slots':{'complete':True,'counts':{},'slots':[]}}]}
r=m.summarize([bad]);assert not r['complete'] and r['rows'][0]['counts'] is None
print('incomplete receipt cannot masquerade as a zero-drop item')
