"""Pinned source/archive/scalar custody only. Never compile or run product code."""
from pathlib import Path
from collections import deque
import hashlib
import json
import re
import subprocess
from bs4 import BeautifulSoup

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
ROOT = Path('/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff')
SNAP = ROOT / 'ksp-rpn-lifecycle-fix-snapshot'
BASE = 'b557602b540ab2e2b45875d4ec1fec1e38e8e587'
SOURCE = 'e6710e4e04c2404123a730d5b7977afa6a68c679'
ORIGINAL = '3cc0f94ea625715028897d4611c0d477ccfed724'
INTEGRATION = '2a243bcfa2262abb789265aa0951fd2a85854023'
H = lambda b: hashlib.sha256(b).hexdigest()
def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args])
def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')
def blob(ref, path):
    return git('show', ref + ':' + path)
def copy(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
def mask(s):
    return re.sub(r'"(?:\\.|[^"\\])*"|//[^\n]*|/\*[\s\S]*?\*/',
                  lambda m: ''.join('\n' if c == '\n' else ' ' for c in m[0]), s)
def function(s, name):
    m = re.search(r'\bfn ' + re.escape(name) + r'\s*(?:\(|<)', s)
    assert m, name
    cleaned = mask(s)
    start = s.rfind('\n', 0, m.start()) + 1
    opening = cleaned.index('{', m.end())
    depth = 1
    end = opening + 1
    while depth:
        depth += (cleaned[end] == '{') - (cleaned[end] == '}')
        end += 1
    end += (s[end:end+1] == '\n')
    return {'start_line': s[:start].count('\n') + 1,
            'end_line': s[:end].count('\n'), 'span_sha256': H(s[start:end].encode())}

assert git('rev-parse', SOURCE + '^').decode().strip() == BASE
assert git('rev-parse', BASE + '^').decode().strip() == ORIGINAL
assert git('rev-parse', ORIGINAL + '^').decode().strip() == INTEGRATION
assert git('rev-parse', INTEGRATION + '^{tree}').decode().strip() == 'c9504d7133090d35117048895189f50fdfa8fb6a'
changed = git('diff', '--name-only', BASE, SOURCE).decode().splitlines()
assert changed == ['crates/sampler-core/src/controller_event.rs', 'crates/sampler-core/src/plan_programs.rs',
                   'crates/sampler-core/src/prepare.rs', 'crates/sampler-core/src/stages.rs', 'crates/sampler-ksp/tests/rpn.rs']
paths = changed + [
 'crates/sampler-ksp/src/lib.rs', 'crates/sampler-ksp/src/lower.rs', 'crates/sampler-ksp/src/builtins.rs',
 'crates/sampler-ksp/tests/fixtures/rpn-service-neighbors.ksp', 'crates/sampler-core/src/behavior.rs',
 'crates/sampler-core/src/lib.rs', 'crates/sampler-core/src/render.rs', 'crates/sampler-core/src/script_params.rs',
 'crates/sampler-core/src/prepare/selection.rs', 'crates/sampler-core/src/gate.rs',
 'crates/sampler-core/tests/support/mod.rs', 'crates/sampler-core/tests/note_stages.rs',
 'crates/sampler-core/tests/controller_stages.rs', 'crates/sampler-kontakt/src/load.rs', 'src/sound/v2.rs']
files = []
for path in paths:
    versions = {}
    for ref in [BASE, SOURCE]:
        data = blob(ref, path)
        copy(SNAP / ref / path, data)
        versions[ref] = {'git_blob': git('rev-parse', ref + ':' + path).decode().strip(),
                         'sha256': H(data), 'lines': len(data.splitlines())}
    files.append({'path': path, 'changed': path in changed, 'versions': versions})
functions = {
 'crates/sampler-core/src/stages.rs': ['with_stages'],
 'crates/sampler-core/src/plan_programs.rs': ['with_parameter_programs', 'send_parameter', 'signal_programs'],
 'crates/sampler-core/src/prepare.rs': ['with_programs', 'with_release_program'],
 'crates/sampler-core/src/controller_event.rs': ['with_controller_programs', 'dispatch_controller'],
 'crates/sampler-core/src/behavior.rs': ['admit_plan_context', 'behavior_stage', 'queue_behavior', 'drain_behavior', 'resume_yielded'],
 'crates/sampler-core/src/render.rs': ['set_behavior_block_fuel', 'render_inner'],
 'crates/sampler-ksp/src/lib.rs': ['bind_modules', 'compile_initialized'],
 'crates/sampler-ksp/src/lower.rs': ['callback_type', 'sys'],
 'crates/sampler-core/src/prepare/selection.rs': ['trigger_inner'],
 'crates/sampler-core/tests/support/mod.rs': ['without_heap'],
}
# The exact generated-note source-slot assignment is recorded as a bounded line
# citation below; do not guess its enclosing function name.
functions.pop('crates/sampler-core/src/prepare/selection.rs')
spans = []
for path, names in functions.items():
    for name in names:
        spans.append({'path': path, 'symbol': name,
                      'versions': {ref: function(blob(ref, path).decode(), name) for ref in [BASE, SOURCE]}})
for path, start, end, label in [
 ('crates/sampler-kontakt/src/load.rs', 848, 855, 'public initialized compiler production loader'),
 ('crates/sampler-kontakt/src/load.rs', 1002, 1016, 'actual production lower hook -> bind_modules'),
 ('src/sound/v2.rs', 3361, 3383, 'production Runtime install and block budget8192'),
 ('crates/sampler-core/src/prepare/selection.rs', 291, 303, 'generated-note creator physical source slot'),
 ('crates/sampler-core/src/script_params.rs', 821, 828, 'EventInfo::Source and physical origin queries'),
 ('crates/sampler-ksp/src/lower.rs', 2737, 2752, 'typed parameter sender lowering'),
 ('crates/sampler-ksp/src/builtins.rs', 620, 638, 'internal symbolic callback discriminants'),
]:
    versions = {}
    for ref in [BASE, SOURCE]:
        data = blob(ref, path)
        selected = b''.join(data.splitlines(keepends=True)[start-1:end])
        versions[ref] = {'start_line': start, 'end_line': end, 'span_sha256': H(selected)}
    spans.append({'path': path, 'symbol': label, 'versions': versions})
trace = {'parent_sha': BASE, 'source_sha': SOURCE, 'source_tree': git('rev-parse', SOURCE+'^{tree}').decode().strip(),
         'parent_tree': git('rev-parse', BASE+'^{tree}').decode().strip(), 'files': files, 'spans': spans,
         'file_count': len(files), 'symbol_records': len(spans), 'versioned_spans': 2*len(spans),
         'limit': 'Pinned source custody and selected source trace, not Rust execution/native/performance.'}
save(HERE / 'source-trace.json', trace)

path = 'crates/sampler-ksp/tests/rpn.rs'
s = blob(SOURCE, path).decode()
old = blob(BASE, path).decode()
old_names = re.findall(r'#\[test\]\s*fn (\w+)', old)
names = re.findall(r'#\[test\]\s*fn (\w+)', s)
assert len(old_names) == 14 and len(names) == 20 and names[:14] == old_names
strengthened = 'parameter_binding_rejects_invalid_stage_and_duplicate_kind_and_resets_with_programs'
for name in old_names:
    a, b = function(old, name), function(s, name)
    assert (a['span_sha256'] == b['span_sha256']) == (name != strengthened), name
tests = []
for name in names:
    span = function(s, name)
    body = ''.join(s.splitlines(keepends=True)[span['start_line']-1:span['end_line']])
    literals = re.findall(r'"(?:\\.|[^"\\])*"', body)
    fixtures = [{'sha256_of_rust_literal_bytes': H(v.encode()), 'kind': 'inline Rust literal / synthetic source or assertion'} for v in literals]
    if name.startswith('midi_and_nka'):
        fixtures.append({'path': 'crates/sampler-ksp/tests/fixtures/rpn-service-neighbors.ksp',
                         'sha256': H(blob(SOURCE, 'crates/sampler-ksp/tests/fixtures/rpn-service-neighbors.ksp'))})
    if name.startswith('preempted_sender'):
        fixtures.append({'kind': 'format-generated synthetic sender', 'cases': ['send=false', 'send=true'],
                         'public_block_budget': '1 -> queue younger work -> 8192; assert admitted receivers pc0'})
    if name.startswith('deferred_fanout'):
        fixtures.append({'kind': 'inline synthetic deferred pressure', 'capacities': [3,5],
                         'public_block_budget': '1 -> queue younger work -> 8192; first/second-message atomic fault'})
    if name.startswith('sparse_physical'):
        fixtures.append({'kind': 'inline synthetic sparse source slots', 'slots': [4,0,2],
                         'sender_instance': 1, 'separate_note_inputs': [60,61]})
    tests.append({'package': 'sampler-ksp', 'target': 'rpn', 'harness_filter': name,
                  'inventory_qualified_name': 'sampler-ksp::rpn::'+name,
                  'source_sha': SOURCE, 'path': path, **span, 'status': 'NOT_RUN',
                  'compile_status': 'NOT_COMPILED_NOT_TYPECHECKED', 'fixtures': fixtures,
                  'inherited': name in old_names, 'strengthened': name == strengthened})
map_ = {'source_sha': SOURCE, 'parent_sha': BASE, 'count': len(tests), 'tests': tests,
        'required_future_package_targets': [{'package': 'sampler-ksp', 'target': 'rpn', 'exact_harness_filters': names}],
        'authorization': 'NONE. Following central combined offline/release -j1 cycle only after fresh review and new authorization.'}
save(HERE / 'tests.json', map_)
save(SNAP / 'test-map.json', map_)

reference = []
for name in ['round10-service-requirements.md','round10-service-requirements.json',
             'round8-service-consumer-handoff.md','round8-service-consumer-handoff.json',
             'rpn-callback-constants-next.md','ksp-rpn-consumer-next.md','ksp-rpn-consumer-next.json',
             'ksp-rpn-consumer-source-review.md','ksp-rpn-consumer-source-review.json',
             'ksp-rpn-lifecycle-fix-rea.json']:
    data = (ROOT / name).read_bytes()
    copy(SNAP / 'reference' / name, data)
    reference.append({'path': name, 'sha256': H(data)})
assert H((ROOT/'round10-service-requirements.json').read_bytes()) == '613adb5b09356b7616794e0db90b3b38a4d7ba4f54636f8e805c658c465009c7'
authority = json.loads((REPO / 'docs/audit-2026-10-10/ksp-rpn-consumer-next/primary-authority.json').read_bytes())
assert len(authority) == 7
archives = {}
for section in authority:
    data = Path(section['archive_path']).read_bytes()
    assert H(data) == section['document_sha256']
    own_path = SNAP/'reference'/Path(section['archive_path']).name
    copy(own_path, data)
    headings = [h for h in BeautifulSoup(data, 'html.parser').select('h1,h2,h3,h4,h5,h6')
                if h.get_text(' ', strip=True).replace('\u200b', '') == section['section']]
    assert len(headings) == 1
    text = headings[0].find_parent('section').get_text(' ', strip=True)
    assert text == section['excerpt'] and H(text.encode()) == section['excerpt_sha256']
    section['own_archive_path'] = str(own_path)
    section['status'] = 'FRESH_STATIC_REEXTRACTION_MATCH_NOT_NATIVE_EXECUTION'
    archives[str(own_path)] = H(data)
save(HERE/'primary-authority.json', authority)
save(SNAP/'reference/primary-authority.json', authority)
copy(HERE/'rea-current-document.json', (ROOT/'ksp-rpn-lifecycle-fix-rea.json').read_bytes())
save(HERE/'input-custody.json', {'inputs': reference, 'archives': archives,
     'snapshot': str(SNAP), 'parent_snapshot': 'Git object copies of exactb557 retrieved after source edits; never mutable producer files.'})

# Independent scalar model of the lifecycle policy and capacity arithmetic.
# This is NOT a runtime simulation/receipt and supplies no Rust PASS credit.
ordinary = [(None,None,0),(None,None,1),(None,None,2)]
rotated = [ordinary[2],ordinary[0],ordinary[1]]
assert len(ordinary) == len(rotated) and 1 < len(rotated)  # old guard admits wrong schema
assert ordinary != rotated                             # new installed-table guard rejects
owners = {0:0,1:1,2:2,3:1,4:2}
def valid_binding(stage, program, prior):
    owner = owners[program]
    return all(owners[p] == owner for p in stage if p is not None) and all(owners[p] == owner for p in prior)
assert valid_binding(ordinary[1],3,[]) and not valid_binding(rotated[1],3,[])
assert valid_binding(rotated[2],3,[]) and valid_binding((None,None,None),3,[])
assert not valid_binding((None,None,None),3,[4])
assert [cap-2 >= 2 for cap in [3,5]] == [False,True]
assert 5-2-2 < 2 # second fanout insufficient; no partial admission
# Independent bounded FIFO order sketch: emitted fanout before subsequently
# emitted PGS callbacks; younger controller proceeds after deferred callbacks.
q = deque(['rpn1','rpn2','pgs1','pgs2','younger_note','controller'])
log, logged = '', set()
while q:
    op = q.popleft()
    if op.startswith('rpn'):
        log += op[-1]
        q.extend(['pgs1','pgs2'])
    elif op.startswith('pgs') and op not in logged:
        logged.add(op)
        log += str(2+int(op[-1]))
    elif op == 'controller':
        log += '5'
assert log == '12345'
checks = {'state': 'STATIC_CUSTODY_AND_SCALAR_ONLY', 'source_sha': SOURCE,
          'changed_paths': changed, 'source_files': len(files), 'git_blobs_copied': 2*len(files),
          'symbol_records': len(spans), 'versioned_spans': 2*len(spans), 'tests_not_run': len(tests),
          'retained_test_names':14, 'unchanged_full_test_bodies':13, 'strengthened_original_tests':1,
          'new_tests':6, 'archives':len(archives), 'primary_sections_reextracted':len(authority),
          'scalar_checks':['three-module changed schema','wrong in-range owner','empty route explicit assignment',
                           'mixed peer owner rejection','capacity3 first refusal','capacity5 second refusal','FIFO sketch12345'],
          'scalar_limit':'Independent arithmetic/order sketch only. Actual deferred=true/public binder/runtime/heap/PCM witnesses NOT_RUN.',
          'rustfmt':'PARSE_FORMAT_ONLY', 'git_whitespace':'STATIC_ONLY', 'native':'UNKNOWN',
          'performance':'CPU AND RAM below BOTH v1/Kontakt UNACHIEVED'}
save(HERE/'static-checks.json', checks)
save(SNAP/'manifest.json', trace)
print(json.dumps(checks, indent=2))
