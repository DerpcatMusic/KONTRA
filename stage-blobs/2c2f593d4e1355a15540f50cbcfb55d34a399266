"""Bounded immutable source/custody/scalar checks. NEVER runs Rust or product code.

Run from this checkout with python3 docs/audit-2026-10-10/
ksp-rpn-deferred-fixture-fix/check-source.py. Uses installed BeautifulSoup only
for exact archived NI section extraction. Outputs owned audit/handback evidence.
The scalar model transcribes selected queue rules, not the complete Runtime.
"""
from collections import deque
from pathlib import Path
from urllib.parse import urlsplit
import hashlib
import json
import re
import shutil
import subprocess

SOURCE = 'c6cf5a3da69ed8c72541e5402376430711891501'
PARENT = '273504189d92656e4444552e1c5d8ff548f4c2c2'
ORIGINAL = '3cc0f94ea625715028897d4611c0d477ccfed724'
ROOT = Path(__file__).resolve().parents[3]
AUDIT = Path(__file__).resolve().parent
HANDOFF = Path('/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff')
SNAP = HANDOFF / 'ksp-rpn-deferred-fixture-fix-snapshot'
TEST = 'crates/sampler-ksp/tests/rpn.rs'
MIXED = 'preempted_sender_defers_receivers_fifo_and_preserves_numeric_pgs_controller_order'
PURE = 'preempted_sender_defers_receivers_fifo_without_notification_interleaving'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args])


def obj(rev, path):
    return git('show', f'{rev}:{path}')


def emit(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + '\n')


def test_bodies(data):
    s = data.decode()
    marks = list(re.finditer(r'#\[test\]\nfn (\w+)\(', s))
    return {m.group(1): s[m.start():marks[i + 1].start() if i + 1 < len(marks) else len(s)].rstrip().encode()
            for i, m in enumerate(marks)}


# Check exact immutable subject and exclusive scope before copying any evidence.
assert git('rev-parse', SOURCE + '^').decode().strip() == PARENT
assert git('diff', '--name-only', PARENT, SOURCE).decode().splitlines() == [TEST]
assert git('diff', '--check', PARENT, SOURCE) == b''
review = HANDOFF / 'ksp-rpn-lifecycle-successor-review.json'
assert digest(review.read_bytes()) == '885876bba032b2a2cd0dc04fe7379ebefcb74d98b282c4814c48c9ff23e336ac'
review_data = json.loads(review.read_bytes())
root_check = json.loads((HANDOFF / 'root-ksp-rpn-lifecycle-successor-review-check.json').read_bytes())
assert root_check['review_sha256'] == digest(review.read_bytes())
assert review_data['new_blocking_findings'][0]['id'] == 'F2'
assert review_data['historical_finding']  # F1 retained, not closed by this task
source = obj(SOURCE, TEST)
assert (ROOT / TEST).read_bytes() == source
current = test_bodies(source)
parent = test_bodies(obj(PARENT, TEST))
original = test_bodies(obj(ORIGINAL, TEST))
assert len(parent) == 20 and len(current) == 21
assert parent.keys() <= current.keys()
unchanged = [n for n in parent if parent[n] == current[n]]
original_same = [n for n in original if original[n] == current[n]]
assert len(unchanged) == 19 and len(original_same) == 13
assert [n for n in parent if parent[n] != current[n]] == [MIXED]
assert set(current) - set(parent) == {PURE}
assert b'if send { 13425 } else { 345 }' in current[MIXED]
assert b'if send { 12345 } else { 345 }' in parent[MIXED]
assert b'assert_eq!(cell(&rt, 2, 3), 12,' in current[PURE]

# Copy selected immutable Git bytes, then pin full files and connected spans.
selected = {
    'crates/sampler-core/src/plan_programs.rs': [(143, 241, 'send_parameter complete admission/deferred branch'), (245, 285, 'signal_programs complete scheduling')],
    'crates/sampler-core/src/behavior.rs': [(1126, 1173, 'start/admit plan context and private payload'), (1356, 1462, 'queue_behavior and drain_behavior complete bodies'), (1652, 1705, 'yielded_plan/yield_behavior/resume_yielded'), (2480, 2503, 'SendParameter/ReadParameter/Signal VM dispatch')],
    'crates/sampler-core/src/render.rs': [(42, 45, 'public block fuel setter'), (171, 211, 'render_inner yielded snapshot ownership')],
    'crates/sampler-core/src/controller_event.rs': [(51, 157, 'controller dispatch/admission')],
    'crates/sampler-ksp/src/lower.rs': [(159, 245, 'callback-type grouping and entry start'), (2695, 2706, 'PGS write/Signal suppression in PGS callback')],
    'crates/sampler-ksp/src/lib.rs': [(614, 646, 'public binder stage/instance owners'), (738, 808, 'signal/parameter binding and final installation')],
    'crates/sampler-core/tests/support/mod.rs': [(1, 59, 'without_heap allocation/deallocation guard')],
    TEST: [(1, 98, 'public compiler/binder Runtime fixtures and cell helpers'), (534, 720, 'preserved F1 lifecycle witnesses')],
    'crates/sampler-ksp/tests/fixtures/rpn-service-neighbors.ksp': [(1, 21, 'unchanged synthetic service fixture')],
    'crates/sampler-ksp/Cargo.toml': [(1, 19, 'actual sampler-ksp package/edition/dependencies')],
}
files = []
spans = []
for path, ranges in selected.items():
    data = obj(SOURCE, path)
    assert data == obj(PARENT, path) or path == TEST
    own = SNAP / 'source' / path
    own.parent.mkdir(parents=True, exist_ok=True)
    own.write_bytes(data)
    assert own.read_bytes() == data
    blob = git('rev-parse', f'{SOURCE}:{path}').decode().strip()
    files.append({'path': path, 'sha': SOURCE, 'blob': blob, 'sha256': digest(data), 'bytes': len(data), 'own_path': str(own)})
    lines = data.splitlines(keepends=True)
    for start, end, symbol in ranges:
        assert 1 <= start <= end <= len(lines)
        spans.append({'path': path, 'symbol': symbol, 'start_line': start, 'end_line': end,
                      'sha': SOURCE, 'blob': blob, 'file_sha256': digest(data),
                      'span_sha256': digest(b''.join(lines[start - 1:end]))})
for label, rev in [('parent-rpn.rs', PARENT), ('original-rpn.rs', ORIGINAL)]:
    (SNAP / label).write_bytes(obj(rev, TEST))

# Every full test and embedded fixture is pinned. Names are harness names;
# sampler-ksp::rpn::<name> is only a human inventory label, not a Rust module.
tests = []
for name, body in current.items():
    at = source.index(body)
    start = source[:at].count(b'\n') + 1
    end = start + body.count(b'\n')
    fixture_strings = re.findall(r'"((?:\\.|[^"\\])*)"', body.decode(), re.S)
    fixtures = [s for s in fixture_strings if re.search(r'\bon (init|note|rpn|nrpn|controller|pgs_changed)\b', s)]
    tests.append({'package': 'sampler-ksp', 'target': 'rpn', 'harness_name': name,
                  'inventory_label': 'sampler-ksp::rpn::' + name, 'status': 'NOT_RUN_NOT_COMPILED',
                  'path': TEST, 'sha': SOURCE, 'blob': files[[f['path'] for f in files].index(TEST)]['blob'],
                  'start_line': start, 'end_line': end, 'body_sha256': digest(body),
                  'body_digest_rule': 'UTF8 from #[test] through final closing brace, no trailing newline',
                  'fixture_strings': [{'sha256': digest(s.encode()), 'bytes': len(s.encode()), 'text': s} for s in fixtures],
                  'heap_guard': b'support::without_heap' in body, 'pcm_assertions_present': b'audio[' in body,
                  'byte_identical_to_parent': name in unchanged,
                  'byte_identical_to_original': name in original_same})
emit(AUDIT / 'tests.json', tests)
emit(AUDIT / 'source-custody.json', {'source_sha': SOURCE, 'parent_sha': PARENT, 'files': files, 'spans': spans})

# Exact archived primary documents are requirements only, never runtime results.
from bs4 import BeautifulSoup
primary = json.loads((HANDOFF / 'ksp-rpn-lifecycle-successor-review-snapshot/reference/selected-authority.json').read_bytes())
for record in primary:
    src = HANDOFF.parent / record['own_archive_path']
    data = src.read_bytes()
    assert digest(data) == record['archive_sha256'] == record['document_sha256']
    own = SNAP / 'reference' / src.name
    own.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, own)
    soup = BeautifulSoup(data, 'html.parser')
    section = soup.find(id=urlsplit(record['url']).fragment)
    assert section is not None
    if section.name != 'section':
        section = section.find_parent('section')
    excerpt = section.get_text(separator=' ', strip=True)
    assert digest(excerpt.encode()) == record['excerpt_sha256']
    assert excerpt == record['excerpt']
    record['previous_owned_archive_path'] = str(src)
    record['own_archive_path'] = str(own)
    record['fresh_reextraction'] = 'MATCH_SPEC_ONLY_NOT_NATIVE'
emit(AUDIT / 'primary-authority.json', primary)
receipt = HANDOFF / 'ksp-rpn-lifecycle-successor-review-snapshot/rea-current-document.json'
shutil.copyfile(receipt, SNAP / 'reference/rea-current-document-inherited.json')
emit(AUDIT / 'native-limit.json', {'native': 'UNKNOWN', 'new_native_operation': False,
    'inherited_read_only_receipt': str(receipt), 'receipt_sha256': digest(receipt.read_bytes()),
    'result': 'target_unavailable', 'target_opened': False,
    'ordering_12345_or_13425': 'Neither is a vendor claim', 'timing_echo_scheduler': 'UNKNOWN'})

# Model derives parameters from exact fixture strings and checks connected rules.
def strings_for(name):
    return next(t['fixture_strings'] for t in tests if t['harness_name'] == name)


mixed_text = '\n'.join(s['text'] for s in strings_for(MIXED))
pure_text = '\n'.join(s['text'] for s in strings_for(PURE))
assert 'on pgs_changed' in mixed_text and 'on controller' in mixed_text
assert 'on pgs_changed' not in pure_text and 'on controller' not in pure_text
assert 'set_rpn(60,16383)' in pure_text
assert 'pgs_set_key_val(GO,0,1)' in mixed_text
for text in [mixed_text, pure_text]:
    assert 'while ($i<32)' in text
assert re.findall(r'pgs_get_key_val\(LOG,0\)\*10\+(\d)', mixed_text) == ['5', '1', '3', '2', '4']
assert re.findall(r'pgs_get_key_val\(LOG,0\)\*10\+(\d)', pure_text) == ['1', '2']
plan_programs = obj(SOURCE, 'crates/sampler-core/src/plan_programs.rs').decode()
behavior = obj(SOURCE, 'crates/sampler-core/src/behavior.rs').decode()
lower = obj(SOURCE, 'crates/sampler-ksp/src/lower.rs').decode()
assert 'let i = if deferred { offset } else { count - 1 - offset };' in plan_programs
assert '.any(|&y| self.yielded_plan(y) == Some(plan));' in plan_programs
assert 'let requeued = self.yielded.len() - (pending - done - 1);' in behavior
assert '.rev()\n                .take(requeued)' in behavior
assert 'if self.callback_type != b::cb::PGS_CHANGED' in lower


def model(send, notifications, reverse=False, tail_rule=True):
    # Initial fuel exhaustion has already queued sender and live younger work.
    # All work belongs to the same retained plan. Replenished fuel is enough
    # for each finite handler to finish or enter its intentional long wait.
    q = deque(['sender', 'younger'] + (['controller'] if notifications else []))
    log = 0
    go = False
    logged = [False, False]
    serial = 0
    passes = []
    executions = []
    checkpoint = None
    receiver_order = []
    second_receiver_log = None
    def signal():
        nonlocal serial
        if notifications:
            serial += 1
            q.extend([f'pgs1#{serial}', f'pgs2#{serial}'])
    for block in range(20):
        if not q:
            break
        pending = len(q)
        events = []
        for done in range(pending):
            item = q.popleft()
            requeued = len(q) - (pending - done - 1)
            # Every newly queued callback here has live same-plan ownership.
            # Newly queued PGS handlers count just like parameter receivers.
            if tail_rule and requeued:
                q.append(item)
                events.append({'id': item, 'blocked_by_new_same_plan_tail': requeued})
                continue
            executions.append(item)
            if item == 'sender':
                assert any(y in ['younger', 'controller'] for y in q)
                if send:
                    q.extend(['rpn2', 'rpn1'] if reverse else ['rpn1', 'rpn2'])
                if notifications:
                    go = True
                    signal()
            elif item.startswith('rpn'):
                digit = int(item[-1])
                receiver_order.append(digit)
                log = log * 10 + digit
                if digit == 2 and not notifications:
                    second_receiver_log = log  # exact new fixture's $log read
                signal()
                # wait(100000): callback becomes waiting, not yielded here.
            elif item.startswith('pgs'):
                i = int(item[3]) - 1
                if go and not logged[i]:
                    logged[i] = True
                    log = log * 10 + (3 + i)
                    # PgsChanged suppresses re-signal in the exact lowerer.
            elif item == 'controller':
                log = log * 10 + 5
                signal()
            events.append({'id': item, 'log_after': log})
        if block == 0:
            checkpoint = list(q)
        passes.append({'block': block, 'pending_snapshot': pending, 'events': events,
                       'queue_after': list(q), 'log': log})
    assert not q, 'finite scalar fixture must drain its yielded queue'
    return {'send': send, 'pgs_bindings': notifications, 'final_log': log,
            'receiver_execution_digits': receiver_order, 'second_receiver_log': second_receiver_log,
            'sender_checkpoint_queue': checkpoint,
            'executions': executions, 'passes': passes}


no_send = model(False, True)
interleaved = model(True, True)
fifo = model(True, False)
assert no_send['final_log'] == 345
assert interleaved['final_log'] == 13425
assert interleaved['receiver_execution_digits'] == [1, 2]
assert fifo['final_log'] == fifo['second_receiver_log'] == 12 and fifo['receiver_execution_digits'] == [1, 2]
assert fifo['sender_checkpoint_queue'] == ['rpn1', 'rpn2', 'younger']
assert interleaved['sender_checkpoint_queue'] == ['rpn1', 'rpn2', 'pgs1#1', 'pgs2#1', 'younger', 'controller']
reverse = model(True, False, reverse=True)
missing_tail = model(True, True, tail_rule=False)
assert reverse['final_log'] == 21 != fifo['final_log']
assert reverse['second_receiver_log'] == 2 != fifo['second_receiver_log']
assert missing_tail['final_log'] != interleaved['final_log']
assert interleaved['final_log'] != 12345
emit(AUDIT / 'scalar-trace.json', {
    'authority': 'PYTHON_SCALAR_SELECTED_SOURCE_RULES_ONLY', 'rust_execution': False,
    'source_sha': SOURCE, 'test_status': 'NOT_RUN_NOT_COMPILED',
    'results': {'no_send_baseline': no_send, 'pgs_controller_interleaving': interleaved, 'pure_receiver_fifo': fifo},
    'negative_controls': {'reversed_fifo_final_log': reverse['final_log'],
        'reversed_fifo_second_receiver_log': reverse['second_receiver_log'],
        'removed_tail_rule_yields': missing_tail['final_log'], 'old_12345_rejected': True},
    'assumptions': ['All modeled live callbacks share one retained PlanId',
        'Fuel8192 lets each finite fixture handler finish/wait without extra preemption',
        'The source-derived checkpoint follows deliberate public fuel1 exhaustion with live younger work',
        'Long intentional waits do not expire within the fixtures1024-sample observation window',
        'No additional plan starts/listeners/automation exist in these synthetic fixtures'],
    'limitations': 'No Rust compilation, KSP compilation, complete VM, allocation measurement, PCM observation or native behavior is modeled.'})

# Carry the exact prior neighbor selection to root, rehashing all source files.
# This is selection custody, not a renewed semantic review or execution request.
future = review_data['future_validation']
future['package_targets'][0]['exact_harness_names'] = list(current)
for neighbor in future['focused_existing_neighbors']:
    data = obj(SOURCE, neighbor['path'])
    assert digest(data) == neighbor['file_sha256']
    assert re.search(r'fn\s+' + re.escape(neighbor['harness_name']) + r'\s*\(', data.decode())
    neighbor['source_sha'] = SOURCE
    neighbor['blob'] = git('rev-parse', f"{SOURCE}:{neighbor['path']}").decode().strip()
    neighbor['status'] = 'NOT_RUN_NOT_COMPILED_IN_THIS_ROUND'
future['fresh_source_review_required_first'] = True
future['scope'] = 'Selection inputs only; no per-lane run, extra matrix, current-batch append or build authorization.'
emit(AUDIT / 'future-selection.json', future)

# Syntax-only parse. Output remains uninstalled; this never rewrites Rust source.
r = subprocess.run(['rustfmt', '--edition', '2024', '--emit', 'stdout', str(ROOT / TEST)], capture_output=True)
assert r.returncode == 0
assert (ROOT / TEST).read_bytes() == source
custody_negatives = {'one_byte_source_mutation_rejected': digest(source + b'!') != digest(source),
    'one_byte_excerpt_mutation_rejected': digest((primary[0]['excerpt'] + '!').encode()) != primary[0]['excerpt_sha256'],
    'wrong_parent_rejected': git('rev-parse', SOURCE + '^').decode().strip() != ORIGINAL}
assert all(custody_negatives.values())
input_names = ['ksp-rpn-lifecycle-successor-review.md', 'ksp-rpn-lifecycle-successor-review.json',
    'root-ksp-rpn-lifecycle-successor-review-check.json', 'ksp-rpn-lifecycle-fix.md', 'ksp-rpn-lifecycle-fix.json',
    'round10-service-requirements.md', 'round10-service-requirements.json',
    'round8-service-consumer-handoff.md', 'round8-service-consumer-handoff.json',
    'rpn-callback-constants-next.md', 'ksp-rpn-consumer-next.md', 'ksp-rpn-consumer-next.json',
    'ksp-rpn-consumer-source-review.md', 'ksp-rpn-consumer-source-review.json']
inputs = []
for name in input_names:
    data = (HANDOFF / name).read_bytes()
    if name.endswith('.json'):
        json.loads(data)
    own = SNAP / 'inputs' / name
    own.parent.mkdir(parents=True, exist_ok=True)
    own.write_bytes(data)
    inputs.append({'path': str(HANDOFF / name), 'bytes': len(data), 'sha256': digest(data), 'own_path': str(own)})
assert next(i['sha256'] for i in inputs if i['path'].endswith('/round10-service-requirements.json')) == '613adb5b09356b7616794e0db90b3b38a4d7ba4f54636f8e805c658c465009c7'
emit(AUDIT / 'checks.json', {
    'authority': 'SOURCE_CUSTODY_SYNTAX_SCALAR_ONLY', 'source_sha': SOURCE,
    'script_sha256': digest(Path(__file__).read_bytes()), 'exclusive_source_diff': [TEST],
    'retained_parent_names': len(parent), 'current_test_count': len(current),
    'byte_identical_parent_bodies': unchanged, 'byte_identical_original_bodies': original_same,
    'f1_bodies_unchanged': [n for n in unchanged if 'stage' in n or 'parameter' in n],
    'full_file_count': len(files), 'selected_span_count': len(spans),
    'primary_archives': len({p['archive_sha256'] for p in primary}), 'primary_sections': len(primary),
    'rustfmt_syntax': {'exit': r.returncode, 'stdout_sha256': digest(r.stdout), 'stderr': r.stderr.decode(),
        'source_unchanged': True, 'compiled': False, 'typechecked': False},
    'scalar_expected_logs': [no_send['final_log'], interleaved['final_log'], fifo['final_log']],
    'scalar_negative_controls': 3, 'custody_negative_controls': custody_negatives,
    'inputs': inputs, 'tests': 'ALL_21_NOT_RUN_NOT_COMPILED', 'heap': 'UNOBSERVED', 'pcm': 'UNOBSERVED',
    'native': 'UNKNOWN', 'cpu_and_ram_below_both': 'UNACHIEVED', 'build_authorized': False})
print(json.dumps({'source': SOURCE, 'files': len(files), 'spans': len(spans), 'tests_not_run': len(current),
    'models': [no_send['final_log'], interleaved['final_log'], fifo['final_log']], 'syntax_only_exit': r.returncode,
    'native': 'UNKNOWN', 'status': 'SOURCE_READY_FOR_FRESH_REVIEW'}, indent=2))
