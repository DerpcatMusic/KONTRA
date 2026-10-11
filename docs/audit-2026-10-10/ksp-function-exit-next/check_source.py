"""No-build source/spec/model audit. Never imports or executes product code.
Run with Python and an existing BeautifulSoup installation. No installation step.
"""
from pathlib import Path
import argparse
import hashlib
import json
import subprocess
from bs4 import BeautifulSoup

BASE = '2a243bcfa2262abb789265aa0951fd2a85854023'
SOURCE = '360b14f698051d5300de7a8da96bbba746cd4eb3'
TESTS = 'c8c0f610ebf8d49cab921d5b235094f032aaa995'
ROOT = Path(__file__).resolve().parents[3]
H = lambda b: hashlib.sha256(b).hexdigest()

def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args])

def data(sha, path):
    return git('show', sha + ':' + path)

def pin(sha, path, start, end, role):
    b = data(sha, path)
    lines = b.splitlines(keepends=True)
    assert 1 <= start <= end <= len(lines)
    return dict(sha=sha, path=path, git_blob=git('rev-parse', sha + ':' + path).decode().strip(),
                file_sha256=H(b), start_line=start, end_line=end,
                span_sha256=H(b''.join(lines[start-1:end])), role=role)

def source_contract(e, l):
    call = e.split('            StmtKind::Call(f) => {', 1)[1].split('            StmtKind::Builtin(Builtin::Exit', 1)[0]
    assert call.index('self.depth -= 1;') < call.index('flow?;')
    assert 'return Ok(Flow::Exit)' not in call
    assert 'StmtKind::Builtin(Builtin::Exit, _) => return Ok(Flow::Exit)' in e
    assert l.count('let mut g = Gen {') == l.count('in_function: false,') == 2
    assert 'g.in_function = true;\n            g.block(&function.body)?;\n            g.in_function = false;\n            g.emit(I::Op(Op::Return))?;' in l
    branch = l.split('            Exit => {', 1)[1].split('            Continue => {', 1)[0]
    fun, callback = branch.split('} else {', 1)
    assert 'if self.in_function' in fun and 'self.emit(I::Op(Op::Return))?' in fun
    assert 'self.forward()' not in fun and 'I::End' not in fun
    assert 'self.forward()?;\n                    self.emit(I::End)?;' in callback
    assert 'for s in body {\n            self.span = s.span;\n            self.stmt(s)?;' in l

def abstract_model(fuel, wait, mutant=None):
    # Deliberately hand-authored abstract instructions, NOT emitted KSP bytecode.
    # Blocks retain one program/owner/instance and a return-PC stack across yields.
    code = [('call', 7), ('call', 7), ('inc', 'caller'), ('forward',), ('end',),
            ('inc', 'dead'), ('end',),
            ('call', 13), ('inc', 'parent'), ('pitch', 60), ('return',),
            ('inc', 'dead'), ('return',),
            ('inc', 'leaf'), ('pitch', 61)]
    if wait:
        code.append(('wait',))
    code += [('return',), ('inc', 'dead'), ('return',)]
    pc = 0
    frames = []
    cells = dict(leaf=0, parent=0, caller=0, dead=0)
    delivered = []
    pitch = 60
    snapshots = []
    ended = False
    identity = ('old-plan', 'note', 'instance-1', 'original-program')
    deferred_child = None
    deferred_seen = []
    # The no-wait case also checks that Return is not a deferred flush barrier.
    for step in range(200):
        if ended:
            break
        if step % fuel == 0:
            snapshots.append((pc, tuple(frames), identity))
        ins = code[pc]
        pc += 1
        op = ins[0]
        if op == 'call':
            frames.append(pc)
            pc = ins[1]
        elif op == 'inc':
            cells[ins[1]] += 1
        elif op == 'pitch':
            pitch = ins[1]
            if pitch == 61 and not wait:
                deferred_child = 64
            if pitch == 60 and deferred_child is not None:
                deferred_child = 60
        elif op in ('forward', 'wait'):
            if not delivered:
                delivered.append(pitch)
            if deferred_child is not None:
                deferred_seen.append(deferred_child)
                deferred_child = None
            if op == 'wait':
                snapshots.append((pc, tuple(frames), identity))
        elif op == 'return':
            if mutant == 'callback-stop':
                ended = True
            else:
                if mutant == 'function-forward' and not delivered:
                    delivered.append(pitch)
                if mutant == 'return-flush' and deferred_child is not None:
                    deferred_seen.append(deferred_child)
                    deferred_child = None
                assert frames
                pc = frames.pop()
        elif op == 'end':
            ended = True
        else:
            raise AssertionError(op)
    assert ended and not frames
    assert cells == dict(leaf=2, parent=2, caller=1, dead=0)
    assert delivered == ([61] if wait else [60])
    assert all(s[2] == identity for s in snapshots)
    if wait:
        assert any(len(s[1]) == 2 for s in snapshots)
    else:
        assert deferred_seen == [60]
    return dict(fuel=fuel, wait=wait, cells=cells, delivered=delivered,
                snapshots=len(snapshots), native='UNKNOWN', kind='ABSTRACT_MODEL_ONLY')

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--handoff', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    epath = 'crates/sampler-ksp/src/eval.rs'
    lpath = 'crates/sampler-ksp/src/lower.rs'
    tpath = 'crates/sampler-ksp/tests/functions.rs'
    e = data(TESTS, epath).decode()
    l = data(TESTS, lpath).decode()
    t = data(TESTS, tpath).decode()
    source_contract(e, l)
    assert git('rev-parse', SOURCE + '^').decode().strip() == BASE
    assert git('rev-parse', TESTS + '^').decode().strip() == SOURCE
    assert set(git('diff', '--name-only', BASE, SOURCE).decode().splitlines()) == {epath, lpath}
    assert git('diff', '--name-only', SOURCE, TESTS).decode().splitlines() == [tpath]
    # Reconstruct the entire base files, not just regex-check edited statements.
    original_call = '                if let Flow::Exit = flow? {\n                    return Ok(Flow::Exit);\n                }'
    replaced_call = '                // Function exit resumes its caller; errors still propagate after depth restoration.\n                flow?;'
    assert e.replace(replaced_call, original_call) == data(BASE, epath).decode()
    restored = l.replace('            in_function: false,\n', '')
    restored = restored.replace('            g.in_function = true;\n', '').replace('            g.in_function = false;\n', '')
    restored = restored.replace('    /// Function bodies share callback context, but exit only returns to their caller.\n    in_function: bool,\n', '')
    scoped = '                if self.in_function {\n                    self.emit(I::Op(Op::Return))?;\n                } else {\n                    self.forward()?;\n                    self.emit(I::End)?;\n                }'
    restored = restored.replace(scoped, '                self.forward()?;\n                self.emit(I::End)?;')
    assert restored == data(BASE, lpath).decode()
    old_tests = data(BASE, tpath).decode()
    expected_prefix = old_tests.replace('assert_eq!(rt.note_cell(a, 0), Ok(23));', 'assert_eq!(rt.note_cell(a, 0), Ok(123));')
    assert t.startswith(expected_prefix) and len(old_tests.splitlines()) == 260
    names = [x['name'] for f in json.loads((args.handoff / 'ksp-function-exit-syntax.json').read_text())['files'] for x in f['tests'] if x['name'] != 'function_exit_plan']
    assert len(names) == 7 and all('#[test]\nfn ' + n + '(' in t for n in names)
    assert '#[ignore' not in t[len(expected_prefix):]
    syntax = json.loads((args.handoff / 'ksp-function-exit-syntax.json').read_text())
    for f in syntax['files']:
        assert not f['has_error'] and not f['errors']
        assert f['sha256'] == H(data(TESTS, f['path']))
    spec = next(s for s in json.loads((args.handoff / 'round15-selected-section-index.json').read_text()) if s['section'] == 'exit')
    archive = Path(spec['archive']).read_bytes()
    assert H(archive) == spec['document_sha256'] == '9d71af86f192abff29804487e0c00d7b7b84b9fa191d04d4477f38b893cff0a2'
    soup = BeautifulSoup(archive, 'html.parser')
    section = soup.find(id='exit')
    assert section.get_text(' ', strip=True) == spec['excerpt']
    assert H(spec['excerpt'].encode()) == 'b8395df4c13071247dcf0c80d8b644181f9ed246a0e6b77d0f1dbb82f60474e4'
    assert Path(spec['owned_excerpt_file']).read_text() == spec['excerpt']
    quote = 'If used within a function, exit only exits the function, but not the entire callback.'
    assert quote in spec['excerpt']
    assert spec['url'] == 'https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands#exit'
    rejected = []
    mutations = {
        'old_callback_stop_eval': (data(BASE, epath).decode(), l),
        'old_callback_stop_lower': (e, data(BASE, lpath).decode()),
        'missing_scope_initializer': (e, l.replace('in_function: false,', '', 1)),
        'missing_scope_enter': (e, l.replace('g.in_function = true;', '', 1)),
        'missing_scope_reset': (e, l.replace('g.in_function = false;', '', 1)),
        'function_end': (e, l.replace('if self.in_function {\n                    self.emit(I::Op(Op::Return))?;', 'if self.in_function {\n                    self.emit(I::End)?;', 1)),
        'function_forward': (e, l.replace('if self.in_function {', 'if self.in_function {\n                    self.forward()?;', 1)),
        'callback_no_forward': (e, l.replace('} else {\n                    self.forward()?;\n                    self.emit(I::End)?;', '} else {\n                    self.emit(I::End)?;', 1)),
    }
    for name, (me, ml) in mutations.items():
        try:
            source_contract(me, ml)
        except (AssertionError, ValueError):
            rejected.append(name)
        else:
            raise AssertionError('mutation accepted: ' + name)
    models = [abstract_model(f, w) for f in [1, 2, 3, 1024] for w in [False, True]]
    for mutant in ['callback-stop', 'function-forward', 'return-flush']:
        try:
            abstract_model(1, False, mutant)
        except AssertionError:
            rejected.append('model_' + mutant)
        else:
            raise AssertionError('model mutation accepted: ' + mutant)
    spans = []
    requested = {
        epath: [(222, 240, 'Flow and evaluator scope'), (244, 339, 'init/persistence block callers'), (382, 452, 'recursive block/stmt and nearest Call consumer'), (1548, 1580, 'synchronous async-completion block caller')],
        lpath: [(150, 270, 'program caller/function emission and call patching'), (379, 450, 'listener_driver second Gen initializer'), (481, 500, 'private scope metadata'), (587, 604, 'context forwarding'), (663, 745, 'full statement validation/lowering and Call placeholder'), (950, 975, 'expression builtin caller'), (1546, 1570, 'scoped Exit consumer'), (2008, 2026, 'unchanged wait-time forwarding'), (3750, 3770, 'menu builtin caller'), (3814, 3839, 'menu builtin caller')],
        'crates/sampler-ksp/src/lib.rs': [(998, 1058, 'initialize_inner producer'), (1165, 1262, 'Unit/program/listener_driver callers'), (614, 789, 'public multi-module bind and original instance association')],
        'crates/sampler-ksp/src/sema.rs': [(50, 180, 'full callback/function admission'), (252, 261, 'no unreachable-source skip')],
        'crates/sampler-core/src/ops.rs': [(1175, 1193, 'existing Call/Return bounded frames')],
        'crates/sampler-core/src/behavior.rs': [(949, 973, 'continuation program PC frames owner'), (1164, 1173, 'original owner plan resolution'), (1352, 1439, 'deferred flush only wait/end/fault'), (1503, 1527, 'optimized frame consumer'), (1638, 1652, 'preemption retains same continuation'), (2548, 2571, 'callback End vs wait'), (2748, 2765, 'intentional wait same id/frames')],
        'crates/sampler-core/src/stages.rs': [(94, 139, 'actual note-stage forwarded and attack-pending guards')],
        'crates/sampler-core/src/prepare/selection.rs': [(358, 384, 'actual attack pending guard')],
        'crates/sampler-core/src/release.rs': [(480, 496, 'actual release Pending to Forwarded guard')],
        'crates/sampler-core/src/controller_event.rs': [(306, 364, 'actual controller pending-cleared guard')],
        'crates/sampler-core/src/script.rs': [(110, 145, 'plan/program/instance scalar observation')],
        'crates/sampler-core/tests/support/mod.rs': [(1, 72, 'existing heap guard reused')],
    }
    for path, blocks in requested.items():
        for start, end, role in blocks:
            spans.append(pin(TESTS, path, start, end, role))
    for f in syntax['files']:
        for item in f['tests']:
            spans.append(pin(TESTS, f['path'], item['start_line'], item['end_line'], item['name']))
    spans += [pin(BASE, epath, 438, 445, 'reserved base Call'), pin(BASE, tpath, 173, 173, 'old conflicting test expectation'), pin(TESTS, tpath, 173, 173, 'corrected expectation')]
    for start, end in [(188, 204), (218, 225), (379, 395), (477, 490), (1545, 1550)]:
        spans.append(pin(BASE, lpath, start, end, 'reserved base span'))
    report = dict(base=BASE, ordered_source_test_commits=[SOURCE, TESTS], test_tree=git('rev-parse', TESTS + '^{tree}').decode().strip(),
                  status='SOURCE_READY_FOLLOWING_BATCH_ONLY', authored_rust_tests='NOT_RUN_NOT_COMPILED',
                  syntax=syntax, full_original_source_reconstructed=True, old_test_prefix_preserved_except_explicit_expectation=True,
                  relevant_callers_method='NO_CHECKOUT_GRAFT_GRAPH; rg exact base/current Git bytes before edits',
                  source_spans=spans, primary=spec, primary_quote=quote,
                  primary_only_contract='function-vs-callback exit; not native recursion/wait/reentrancy/error/frame-limit law',
                  model_checks=models, rejected_negative_controls=rejected,
                  test_filters=[dict(package='sampler-ksp', target='functions', top_level_name=n, status='NOT_RUN_NOT_COMPILED') for n in names],
                  preserved_existing_test_filters=['shared_function_waits_inherit_each_callback_and_return_without_an_extra_forward', 'nested_function_branches_loops_and_exit_keep_polyphonic_state_and_caller_targets', 'invalid_functions_and_expansion_pressure_fail_during_compilation'],
                  input_hashes={name:H((args.handoff/name).read_bytes()) for name in ['round15-string-consumer-requirements.md', 'round15-string-consumer-requirements.json', 'scout-round15-result.md', 'root-roadmap-round15-checks.json']},
                  native='UNKNOWN', cpu_and_ram_below_both_v1_and_kontakt='UNACHIEVED',
                  limitations=['WASM grammar parse is not typecheck', 'abstract model is not product execution', 'Rust tests uncompiled/unrun', 'no native receipt for changed behavior', 'no measured CPU or RAM comparison'],
                  executed='Python evidence/model assertions and Node WASM grammar parsing only; no Cargo/rustc/clippy/build/test executable/host/native/Wine/reader/protected/scheduler/server/install/publication/nested work')
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(spans=len(spans), models=len(models), rejected=len(rejected), status=report['status'])))

if __name__ == '__main__':
    main()
