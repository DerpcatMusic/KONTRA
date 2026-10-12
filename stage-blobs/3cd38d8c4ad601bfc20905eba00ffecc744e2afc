"""Independent immutable-source custody checks; no product build or execution."""
from pathlib import Path
import copy
import hashlib
import json
import re
import subprocess
from bs4 import BeautifulSoup

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
SOURCE = '3cc0f94ea625715028897d4611c0d477ccfed724'
BASE = '2a243bcfa2262abb789265aa0951fd2a85854023'
H = lambda value: hashlib.sha256(value).hexdigest()

def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args])

def read(name):
    return json.loads((HERE / name).read_bytes())

blobs = {}
def blob(ref, path):
    key = (ref, path)
    if key not in blobs:
        blobs[key] = git('show', f'{ref}:{path}')
    return blobs[key]

bundle = {'trace': read('source-trace.json'), 'tests': read('tests.json'), 'authority': read('primary-authority.json')}

def validate(value):
    trace, tests = value['trace'], value['tests']
    assert trace['parent_sha'] == BASE and trace['source_sha'] == SOURCE
    assert trace['source_tree'] == git('rev-parse', f'{SOURCE}^{{tree}}').decode().strip()
    assert git('rev-parse', f'{SOURCE}^').decode().strip() == BASE
    assert trace['file_count'] == len(trace['files'])
    for file in trace['files']:
        path = file['path']
        assert file['source_sha256'] == H(blob(SOURCE, path))
        assert file['source_git_blob'] == git('rev-parse', f'{SOURCE}:{path}').decode().strip()
        if file['parent_git_blob'] is not None:
            assert file['parent_sha256'] == H(blob(BASE, path))
            assert file['parent_git_blob'] == git('rev-parse', f'{BASE}:{path}').decode().strip()
    assert trace['symbol_span_count'] == len(trace['spans'])
    for span in trace['spans']:
        for ref, location in span['versions'].items():
            if location.get('status') == 'ABSENT_NEW_SYMBOL':
                assert span['anchor'] not in blob(ref, span['path']).decode()
                continue
            data = blob(ref, span['path'])
            lines = data.decode().splitlines(keepends=True)
            start, end = location['start_line'], location['end_line']
            assert 1 <= start <= end <= len(lines)
            assert span['anchor'] in lines[start-1]
            assert location['span_sha256'] == H(''.join(lines[start-1:end]).encode())
            assert location['file_sha256'] == H(data)
            assert location['git_blob'] == git('rev-parse', f'{ref}:{span["path"]}').decode().strip()
    path = 'crates/sampler-ksp/tests/rpn.rs'
    data = blob(SOURCE, path).decode()
    expected = re.findall(r'#\[test\]\s*fn (\w+)', data)
    assert tests['source_sha'] == SOURCE and tests['parent_sha'] == BASE
    assert tests['execution'] == 'NOT_COMPILED_NOT_EXECUTED'
    assert tests['count'] == 14 == len(tests['tests'])
    assert [t['qualified_name'] for t in tests['tests']] == expected
    for test in tests['tests']:
        assert (test['package'], test['target'], test['status'], test['path']) == ('sampler-ksp', 'rpn', 'NOT_RUN', path)
        start = data.index('#[test]', data.index('fn ' + test['qualified_name']) - 12)
        next_test = data.find('#[test]', data.index('fn ' + test['qualified_name']) + len(test['qualified_name']))
        end = next_test if next_test >= 0 else len(data)
        assert test['span_sha256'] == H(data[start:end].encode())
        assert test['start_line'] == data[:start].count('\n') + 1
        assert test['end_line'] == data[:end].count('\n')
        fixture = 'crates/sampler-ksp/tests/fixtures/rpn-service-neighbors.ksp' if test['qualified_name'].startswith('midi_and_nka') else f'{path}::{test["qualified_name"]} (inline synthetic KSP source)'
        assert test['fixture'] == fixture
    assert tests['future_package_targets'] == [{'package': 'sampler-ksp', 'target': 'rpn', 'qualified_names': expected}]
    assert len(value['authority']) == 7
    for section in value['authority']:
        data = Path(section['archive_path']).read_bytes()
        assert H(data) == section['document_sha256']
        headings = [h for h in BeautifulSoup(data, 'html.parser').select('h1,h2,h3,h4,h5,h6') if h.get_text(' ', strip=True).replace('\u200b', '') == section['section']]
        assert len(headings) == 1
        text = headings[0].find_parent('section').get_text(' ', strip=True)
        assert text == section['excerpt']
        assert H(text.encode()) == section['excerpt_sha256']
    return True

validate(bundle)
mutations = [
 ('source_commit', lambda b: b['trace'].__setitem__('source_sha', BASE)),
 ('source_tree', lambda b: b['trace'].__setitem__('source_tree', '0'*40)),
 ('file_digest', lambda b: b['trace']['files'][0].__setitem__('source_sha256', '0'*64)),
 ('git_blob', lambda b: b['trace']['files'][0].__setitem__('source_git_blob', '0'*40)),
 ('source_span_digest', lambda b: next(v for s in b['trace']['spans'] for v in s['versions'].values() if 'span_sha256' in v).__setitem__('span_sha256', '0'*64)),
 ('test_name', lambda b: b['tests']['tests'][0].__setitem__('qualified_name', 'unregistered_test')),
 ('test_credit', lambda b: b['tests']['tests'][0].__setitem__('status', 'PASS')),
 ('fixture_swap', lambda b: b['tests']['tests'][0].__setitem__('fixture', 'foreign-bank-script.ksp')),
 ('archive_digest', lambda b: b['authority'][0].__setitem__('document_sha256', '0'*64)),
 ('excerpt_corruption', lambda b: b['authority'][0].__setitem__('excerpt', b['authority'][0]['excerpt'] + ' corrupt')),
]
rejected = []
for name, mutate in mutations:
    changed = copy.deepcopy(bundle)
    mutate(changed)
    try:
        validate(changed)
    except AssertionError:
        rejected.append(name)
    else:
        raise AssertionError(f'Corruption escaped: {name}')
assert len(rejected) == 10
report = {'state': 'STATIC_CUSTODY_VERIFIED_NOT_PRODUCT_VALIDATION', 'source_sha': SOURCE, 'parent_sha': BASE, 'source_files': len(bundle['trace']['files']), 'symbol_span_records': len(bundle['trace']['spans']), 'versioned_spans': sum('span_sha256' in v for s in bundle['trace']['spans'] for v in s['versions'].values()), 'historical_retraces': len(bundle['trace']['historical_rpn_service_retraces']), 'tests_not_run': 14, 'immutable_archives': 4, 'sections_reextracted': 7, 'corruption_cases_rejected': rejected, 'native': 'UNKNOWN', 'cpu_and_ram_below_both': 'UNACHIEVED', 'limit': 'Hash/metadata/source/scalar checks only; not Rust compilation, execution, native behavior, or performance evidence.'}
(HERE / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
