"""Separate metadata/digest checks; NOT a build, product test, or native receipt."""
from pathlib import Path
import copy
import hashlib
import json
import re
import subprocess

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
SOURCE = 'e6710e4e04c2404123a730d5b7977afa6a68c679'
BASE = 'b557602b540ab2e2b45875d4ec1fec1e38e8e587'
H = lambda b: hashlib.sha256(b).hexdigest()
def git(*args):
    return subprocess.check_output(['git','-C',str(REPO),*args])
def read(name):
    return json.loads((HERE/name).read_bytes())
bundle = {'trace':read('source-trace.json'), 'tests':read('tests.json'), 'authority':read('primary-authority.json')}
def validate(b):
    trace, tests = b['trace'], b['tests']
    assert trace['source_sha'] == tests['source_sha'] == SOURCE
    assert trace['parent_sha'] == tests['parent_sha'] == BASE
    assert trace['source_tree'] == git('rev-parse',SOURCE+'^{tree}').decode().strip()
    assert trace['parent_tree'] == git('rev-parse',BASE+'^{tree}').decode().strip()
    files = {f['path']:f for f in trace['files']}
    assert trace['file_count'] == len(files) == 20
    assert trace['symbol_records'] == len(trace['spans']) == 27
    assert trace['versioned_spans'] == sum(len(s['versions']) for s in trace['spans']) == 54
    blobs = {}
    for f in files.values():
        for ref,v in f['versions'].items():
            data = git('show',ref+':'+f['path'])
            blobs[ref,f['path']] = data
            assert H(data) == v['sha256']
            assert git('rev-parse',ref+':'+f['path']).decode().strip() == v['git_blob']
            assert v['lines'] == len(data.splitlines())
    for s in trace['spans']:
        for ref,v in s['versions'].items():
            data = blobs[ref,s['path']]
            assert 1 <= v['start_line'] <= v['end_line'] <= len(data.splitlines())
            assert H(b''.join(data.splitlines(keepends=True)[v['start_line']-1:v['end_line']])) == v['span_sha256']
    path = 'crates/sampler-ksp/tests/rpn.rs'
    data = blobs[SOURCE,path]
    expected = re.findall(r'#\[test\]\s*fn (\w+)',data.decode())
    assert tests['count'] == len(tests['tests']) == len(expected) == 20
    assert tests['required_future_package_targets'] == [{'package':'sampler-ksp','target':'rpn','exact_harness_filters':expected}]
    for t,name in zip(tests['tests'],expected):
        assert (t['package'],t['target'],t['harness_filter'],t['status'],t['source_sha'],t['path']) == ('sampler-ksp','rpn',name,'NOT_RUN',SOURCE,path)
        assert t['inventory_qualified_name'] == 'sampler-ksp::rpn::'+name
        assert t['compile_status'] == 'NOT_COMPILED_NOT_TYPECHECKED'
        assert H(b''.join(data.splitlines(keepends=True)[t['start_line']-1:t['end_line']])) == t['span_sha256']
    assert len(b['authority']) == 7
    for a in b['authority']:
        assert H(Path(a['own_archive_path']).read_bytes()) == a['document_sha256']
        assert H(a['excerpt'].encode()) == a['excerpt_sha256']
    return True
validate(bundle)
mutations = [
 ('source_pin',lambda b:b['trace'].__setitem__('source_sha',BASE)),
 ('source_blob_digest',lambda b:b['trace']['files'][0]['versions'][SOURCE].__setitem__('sha256','0'*64)),
 ('symbol_span_digest',lambda b:b['trace']['spans'][0]['versions'][SOURCE].__setitem__('span_sha256','0'*64)),
 ('false_test_pass_credit',lambda b:b['tests']['tests'][0].__setitem__('status','PASS')),
 ('archive_digest',lambda b:b['authority'][0].__setitem__('document_sha256','0'*64)),
]
rejected = []
for name,mutate in mutations:
    changed = copy.deepcopy(bundle)
    mutate(changed)
    try:
        validate(changed)
    except AssertionError:
        rejected.append(name)
    else:
        raise AssertionError('corrupted custody accepted: '+name)
report = {'state':'STATIC_DIGEST_VERIFIED_NOT_PRODUCT_VALIDATION','source_sha':SOURCE,'parent_sha':BASE,
          'source_files':20,'versioned_blobs':40,'symbol_records':27,'versioned_symbol_spans':54,
          'full_test_spans':20,'tests':'NOT_RUN','compile':'NOT_COMPILED_NOT_TYPECHECKED',
          'archives':4,'sections':7,'synthetic_custody_corruptions_rejected':rejected,
          'native':'UNKNOWN','cpu_and_ram_below_both':'UNACHIEVED',
          'limit':'Digests and metadata only. Source clear requires fresh independent review; Rust/heap/PCM/native results absent.'}
(HERE/'verification.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
