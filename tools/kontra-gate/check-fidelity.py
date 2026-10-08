from fidelity import family_match, slot_verdict
assert slot_verdict('unknown') == 'UNKNOWN'
assert slot_verdict({'enabled': 0, 'bypassed': 0}) == 'PASS'
assert slot_verdict({'enabled': 0, 'bypassed': 1}) == 'FAIL'
assert slot_verdict({'enabled': 2, 'bypassed': 0}) == 'FAIL'
assert slot_verdict({'enabled': False, 'bypassed': 0}) == 'UNKNOWN'
oracle = {'basis':'native-host', 'complete':True, 'families':['sus:dyn2:offset48:forward'],
          'minimum_repeats':32, 'rr':{'sus:dyn2:offset48:forward':{'probabilities':{'a':0.5,'b':0.5},'tolerance':0}}}
def takes(rr):
    return [[{'family':'sus:dyn2:offset48:forward', 'rr':r}] for r in rr]
assert family_match(takes(['b','a']*16), oracle)['status'] == 'PASS'
assert family_match(takes(['a']*32), oracle)['reason'] == 'round-robin-distribution-mismatch'
assert family_match(takes(['a','b','a','c']*8), oracle)['reason'] == 'round-robin-outside-native-support'
assert family_match([[{'family':'sus:dyn3:offset48:forward'}]]*32, oracle)['status'] == 'FAIL'
assert family_match(takes(['a','b']), oracle)['status'] == 'UNKNOWN'
assert family_match(takes(['a','b'])*2, {'basis':'v2-selection','complete':True})['status'] == 'UNKNOWN'
assert family_match([[{'family':'sus:dyn2:offset48:forward'}]*2]*32, oracle)['status'] == 'FAIL'
print('slot and independent family/distribution checks passed')

assert family_match(takes(['a','b']*16),dict(oracle,rr={'absent':{'probabilities':{'a':1},'tolerance':0}}))['status']=='UNKNOWN'
