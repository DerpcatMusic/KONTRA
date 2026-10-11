"""Source/archive custody only. Never imports, builds, or executes product code."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
from bs4 import BeautifulSoup  # Existing audit environment; no installation.

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
HANDOFF = Path('/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff')
BASE = '2a243bcfa2262abb789265aa0951fd2a85854023'
SOURCE = '3cc0f94ea625715028897d4611c0d477ccfed724'
H = lambda data: hashlib.sha256(data).hexdigest()

def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args])

def emit(name, value):
    (HERE / name).write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')

def blob(ref, path):
    return git('show', f'{ref}:{path}')

assert git('rev-parse', f'{BASE}^{{tree}}').decode().strip() == 'c9504d7133090d35117048895189f50fdfa8fb6a'
assert git('rev-parse', f'{SOURCE}^').decode().strip() == BASE
reqbytes = (HANDOFF / 'round10-service-requirements.json').read_bytes()
assert H(reqbytes) == '613adb5b09356b7616794e0db90b3b38a4d7ba4f54636f8e805c658c465009c7'
requirements = json.loads(reqbytes)
archives = {a['sha256']: a for a in json.loads((HANDOFF / 'official-docs/manifest.json').read_bytes()) if 'sha256' in a}
authorities = []
for section in requirements['requirements']['rpn']['authority']:
    archive = archives[section['document_sha256']]
    data = Path(archive['file']).read_bytes()
    assert H(data) == section['document_sha256']
    soup = BeautifulSoup(data, 'html.parser')
    headings = [h for h in soup.select('h1,h2,h3,h4,h5,h6') if h.get_text(' ', strip=True).replace('\u200b', '') == section['section']]
    assert len(headings) == 1
    text = headings[0].find_parent('section').get_text(' ', strip=True)
    assert text == section['excerpt'] and H(text.encode()) == section['excerpt_sha256']
    authorities.append({**section, 'archive_path': archive['file'], 'static_reextraction': 'MATCH_NOT_NATIVE_EXECUTION'})
emit('primary-authority.json', authorities)

# Exact spans selected by unique current symbol anchors, not historical line numbers.
anchors = [
 ('crates/sampler-ksp/src/lib.rs', 'pub fn bind_modules(', 35, 'binder/stage/instance owner'),
 ('crates/sampler-ksp/src/lib.rs', '        signals.extend(', 38, 'PGS/async unchanged plus typed receiver binding'),
 ('crates/sampler-ksp/src/lib.rs', '        .with_signal_programs(signals)?', 9, 'prepared consumer table installation'),
 ('crates/sampler-ksp/src/lib.rs', '            K::Rpn =>', 4, 'compiled callback classification'),
 ('crates/sampler-ksp/src/lib.rs', 'pub fn compile_initialized(', 18, 'public source loader compile boundary'),
 ('crates/sampler-ksp/src/lower.rs', 'pub fn host_slot(', 14, 'historical global RPN host slot mapping; new sysvar branch bypasses it'),
 ('crates/sampler-ksp/src/lower.rs', 'fn callback_type(', 22, 'internal callback discriminants'),
 ('crates/sampler-ksp/src/lower.rs', '            SysVar::CallbackType =>', 24, 'callback type and payload reads'),
 ('crates/sampler-ksp/src/lower.rs', '            SetRpn | SetNrpn =>', 24, 'typed command emission and Approximate policy'),
 ('crates/sampler-ksp/src/lower.rs', '            PgsSetKeyVal =>', 22, 'numeric PGS preservation'),
 ('crates/sampler-ksp/src/builtins.rs', 'pub mod cb {', 23, 'callback constants internal namespace'),
 ('crates/sampler-ksp/src/builtins.rs', '    ("$NI_CB_TYPE_INIT",', 18, 'valued symbolic callback constants'),
 ('crates/sampler-ksp/src/eval.rs', '            SetRpn | SetNrpn =>', 8, 'unchanged initialization unsupported gate'),
 ('crates/sampler-core/src/behavior.rs', '    SendParameter {', 11, 'typed send/read instructions'),
 ('crates/sampler-core/src/behavior.rs', 'pub(super) struct Continuation {', 24, 'private callback payload ownership'),
 ('crates/sampler-core/src/behavior.rs', '    pub(super) fn admit_plan_context(', 43, 'receiver admission and retained Plan owner'),
 ('crates/sampler-core/src/behavior.rs', '            Instruction::SendParameter {', 25, 'runtime instruction dispatch'),
 ('crates/sampler-core/src/behavior.rs', '    pub(super) fn behavior_room(', 41, 'bounded arena/refused admission preservation'),
 ('crates/sampler-core/src/behavior.rs', '    pub(super) fn queue_behavior(', 21, 'ready/deferred queue ownership'),
 ('crates/sampler-core/src/plan_programs.rs', 'pub enum ParameterKind {', 23, 'typed receiving table and payload'),
 ('crates/sampler-core/src/plan_programs.rs', '    pub fn with_parameter_programs(', 26, 'prepared validation and ascending order'),
 ('crates/sampler-core/src/plan_programs.rs', '    pub(super) fn send_parameter(', 97, 'actual plan-scoped runtime consumer'),
 ('crates/sampler-core/src/prepare.rs', '    pub(super) parameter_programs:', 1, 'prepared table field'),
 ('crates/sampler-core/src/prepare.rs', '        self.parameter_programs =', 1, 'table replacement reset'),
 ('crates/sampler-core/src/stages.rs', '            .parameter_programs', 7, 'orphan stage rejection'),
 ('crates/sampler-core/src/lib.rs', 'pub use plan_programs::', 1, 'public API export'),
 ('crates/sampler-core/src/ops.rs', '            Op::ReadHost { local, slot } =>', 20, 'unchanged global-host/async-slot reads; RPN no longer lowers here'),
 ('crates/sampler-core/src/script.rs', '    pub(super) fn behavior_write_script_cell(', 34, 'exact instance state consumer'),
 ('crates/sampler-core/src/controller_event.rs', '    pub(super) fn behavior_performance(', 27, 'existing performance ownership'),
 ('crates/sampler-core/src/plans.rs', '    pub fn poll_plan_update(', 53, 'generation replacement/old ownership retention'),
 ('crates/sampler-core/src/plans.rs', '    pub fn collect_retired_plans(', 43, 'retired handle generation fence'),
 ('crates/sampler-core/src/midi_object.rs', '    pub fn complete_midi(', 37, 'unchanged MIDI instance/job completion consumer'),
 ('crates/sampler-core/src/array_file.rs', '    pub fn complete_array_file(', 52, 'unchanged NKA typed completion consumer'),
 ('crates/sampler-kontakt/src/load.rs', '                    sampler_ksp::compile_initialized(', 3, 'actual Kontakt loader public source compile caller'),
 ('crates/sampler-kontakt/src/load.rs', '            sampler_ksp::bind_modules(compiled, plan)', 7, 'actual instrument lower-to-binder caller'),
 ('src/sound/v2.rs', '        let (runtime, control) = Runtime::with_plan_updates_and_note_capacity(', 8, 'production Runtime installation'),
 ('src/sound/v2.rs', '    fn take_effects(', 11, 'unchanged per-instance effect drain (RPN bypasses)'),
 ('src/plugin.rs', '        while let Some((slot, epoch, instance, effect)) = self.effects.pop()', 29, 'unchanged slot/epoch host sink'),
 ('src/plugin.rs', '            s.core.take_effects(slot,', 6, 'unchanged audio effect queue caller'),
]
paths = set(git('diff', '--name-only', BASE, SOURCE).decode().splitlines())
paths.update(p for p, _, _, _ in anchors)
files = []
for path in sorted(paths):
    after = blob(SOURCE, path)
    record = {'path': path, 'source_git_blob': git('rev-parse', f'{SOURCE}:{path}').decode().strip(), 'source_sha256': H(after)}
    exists = subprocess.run(['git', '-C', str(REPO), 'cat-file', '-e', f'{BASE}:{path}'], capture_output=True).returncode == 0
    if exists:
        before = blob(BASE, path)
        record.update(parent_git_blob=git('rev-parse', f'{BASE}:{path}').decode().strip(), parent_sha256=H(before), changed=before != after)
    else:
        record.update(parent_git_blob=None, parent_sha256=None, changed=True, parent_status='NEW_FILE')
    files.append(record)
spans = []
for path, needle, count, symbol in anchors:
    versions = {}
    for ref in [BASE, SOURCE]:
        data = blob(ref, path)
        lines = data.decode().splitlines(keepends=True)
        hits = [i for i, line in enumerate(lines) if needle in line]
        if not hits:
            versions[ref] = {'status': 'ABSENT_NEW_SYMBOL'}
            continue
        assert len(hits) == 1, (path, needle, hits)
        start = hits[0]
        end = min(start + count, len(lines))
        versions[ref] = {'start_line': start + 1, 'end_line': end, 'span_sha256': H(''.join(lines[start:end]).encode()), 'file_sha256': H(data), 'git_blob': git('rev-parse', f'{ref}:{path}').decode().strip()}
    spans.append({'path': path, 'symbol': symbol, 'anchor': needle, 'versions': versions})
# Re-trace exact historical RPN-service snippets where preserved; never transplant line numbers.
historical = json.loads((HANDOFF / 'round8-service-consumer-handoff.json').read_bytes())['services'][0]['immutable_source_pins']
retraces = []
for pin in historical:
    data = blob(pin['sha'], pin['path'])
    assert H(data) == pin['file_sha256']
    fragment = b''.join(data.splitlines(keepends=True)[pin['start_line']-1:pin['end_line']])
    assert H(fragment) == pin['span_sha256']
    current = blob(BASE, pin['path'])
    offset = current.find(fragment)
    row = {'historical': pin, 'parent_sha': BASE}
    if offset >= 0:
        assert current.find(fragment, offset + 1) == -1
        first = current[:offset].count(b'\n') + 1
        row.update(status='EXACT_SNIPPET_RETRACED', start_line=first, end_line=first + pin['end_line'] - pin['start_line'], parent_file_sha256=H(current))
    else:
        row.update(status='CHANGED_SNIPPET_USE_CURRENT_SYMBOL_TRACE', parent_file_sha256=H(current))
    retraces.append(row)
emit('source-trace.json', {'parent_sha': BASE, 'source_sha': SOURCE, 'source_tree': git('rev-parse', f'{SOURCE}^{{tree}}').decode().strip(), 'file_count': len(files), 'symbol_span_count': len(spans), 'files': files, 'spans': spans, 'historical_rpn_service_retraces': retraces, 'limit': 'Exact own source identities, not compiler/typechecker/runtime/native proof.'})

testpath = 'crates/sampler-ksp/tests/rpn.rs'
data = blob(SOURCE, testpath).decode()
tests = []
for match in re.finditer(r'#\[test\]\s*fn (\w+)', data):
    next_test = data.find('#[test]', match.end())
    end = next_test if next_test >= 0 else len(data)
    fragment = data[match.start():end]
    name = match.group(1)
    tests.append({'package': 'sampler-ksp', 'target': 'rpn', 'qualified_name': name, 'status': 'NOT_RUN', 'path': testpath, 'start_line': data[:match.start()].count('\n')+1, 'end_line': data[:end].count('\n'), 'span_sha256': H(fragment.encode()), 'fixture': 'crates/sampler-ksp/tests/fixtures/rpn-service-neighbors.ksp' if name.startswith('midi_and_nka') else f'{testpath}::{name} (inline synthetic KSP source)', 'required_future_result': 'Compile, execute, and independently record result at the authorized combined source SHA.'})
assert len(tests) == 14
emit('tests.json', {'source_sha': SOURCE, 'parent_sha': BASE, 'count': len(tests), 'execution': 'NOT_COMPILED_NOT_EXECUTED', 'future_package_targets': [{'package': 'sampler-ksp', 'target': 'rpn', 'qualified_names': [t['qualified_name'] for t in tests]}], 'tests': tests, 'heap_guard': 'Existing sampler-core/tests/support/mod.rs without_heap reused; NOT_RUN.', 'native_fixture': None, 'pcm_witness': 'nested_named_callback_constants_reach_pcm_through_public_compiler: own synthetic expected PCM [0.25;2], authored NOT_RUN.', 'loader_boundary': 'public compile_with invokes initialize/compile_initialized; production loader uses same compile_initialized and bind_modules. No format-file loader fixture executed.', 'no_extra_matrix': True, 'integration_prerequisite': 'Terminal independent review and root-assigned stage identity/setter lifecycle followup; do not integrate immutable 3cc unchanged.'})
emit('rea-current-document.json', json.loads((HANDOFF / 'ksp-rpn-consumer-next-rea.json').read_bytes()))
emit('static-checks.json', json.loads((HANDOFF / 'ksp-rpn-consumer-next-static.json').read_bytes()))
print(json.dumps({'files': len(files), 'symbol_spans': len(spans), 'historical_retraces': len(retraces), 'tests_not_run': len(tests), 'archives': len({a['document_sha256'] for a in authorities}), 'authority_sections': len(authorities)}))
