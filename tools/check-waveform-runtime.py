#!/usr/bin/env python3
"""Independent transaction model + immutable source/primary-doc pins. No Rust execution."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

BASE = 'd1fb71954431d858f4437d1fb8492db820d68f18'
FROZEN = '2a243bcfa2262abb789265aa0951fd2a85854023'

def digest(raw):
    return hashlib.sha256(raw).hexdigest()

def git(*args):
    return subprocess.check_output(['git', *args])

def pin(sha, path, start=1, end=None):
    raw = git('show', f'{sha}:{path}')
    lines = raw.splitlines(keepends=True)
    end = len(lines) if end is None else end
    assert 1 <= start <= end <= len(lines)
    return dict(sha=sha, path=path, blob=git('rev-parse', f'{sha}:{path}').decode().strip(),
        start_line=start, end_line=end, file_sha256=digest(raw),
        span_sha256=digest(b''.join(lines[start-1:end])), inspection='Exact full inclusive LF span; source ONLY')

def named_pin(sha, path, first, next_marker):
    raw = git('show', f'{sha}:{path}').decode()
    a = raw.index(first)
    b = raw.index(next_marker, a + len(first))
    start = raw[:a].count('\n') + 1
    end = raw[:b].count('\n')
    return pin(sha, path, start, end)

# Independent specification-level model. It does not interpret Rust or KSP.
# All policies beyond the NI signature/units are labelled owned in the data.
def model_case(case, policy):
    fields = ['Cursor', 'Flags', 'MidiStart', 'Highlight']
    sources, ids = set(policy['physical_sources']), set(policy['waveform_ui_ids'])
    state = {(ui, 'Zone', 0): 27 for ui in ids}
    for ui in ids:
        state.update({(ui, p, 0): v for p, v in zip(fields, [case.get('initial_cursor', 0), 3, 60, -1])})
    for address, value in case.get('initial_table', {}).items():
        ui, index = map(int, address.split(':'))
        state[ui, 'Table', index] = value
    # Equivalent finite key budget, not an allocation or CPU measurement.
    capacity = len(state) + case['headroom']
    effects = [('other',)] * case.get('fill_outbox', 0)
    results = []
    for op in case['operations']:
        before, pending = dict(state), list(effects)
        result = 'accepted'
        command = op[0]
        if command == 'other':
            effects.append(('other',))
        else:
            ui = op[1]
            if ui not in ids:
                result = 'InvalidInput'
            elif command == 'attach':
                zone, flags = op[2:4]
                if zone not in sources or zone <= 0:
                    result = 'InvalidInput'
                elif len(effects) >= policy['outbox_limit']:
                    result = 'Capacity'
                else:
                    state = {k:v for k,v in state.items() if not (k[0] == ui and k[1] == 'Table')}
                    state.update({(ui,p,0):v for p,v in zip(['Zone'] + fields, [zone,0,flags,60,-1])})
                    effects.append(('attach',ui,zone,flags))
            else:
                prop, index = op[2:4]
                valid = prop in fields + ['Table'] and (
                    0 <= index < policy['table_index_limit'] if prop == 'Table' else
                    -1 <= index < policy['table_index_limit'] if prop == 'Highlight' else index == 0)
                if not valid:
                    result = 'InvalidInput'
                else:
                    key = (ui,prop,index if prop == 'Table' else 0)
                    if command == 'get':
                        result = state.get(key,0)
                    else:
                        value = op[4]
                        value = min(127,max(0,value)) if prop == 'MidiStart' else index if prop == 'Highlight' else value
                        effect = ('set',ui,prop,index,value)
                        coalesce = bool(effects and effects[-1][:4] == effect[:4])
                        if (key not in state and len(state) == capacity) or (not coalesce and len(effects) >= policy['outbox_limit']):
                            result = 'Capacity'
                        else:
                            state[key] = value
                            if coalesce:
                                effects[-1] = effect
                            else:
                                effects.append(effect)
        if result in ['InvalidInput','Capacity']:
            assert state == before and effects == pending, 'Rejected mutation was not atomic'
        results.append(result)
    table = {f'{ui}:{index}':value for (ui,p,index),value in state.items() if p == 'Table'}
    assert results == case['results'], (case['name'],results,case['results'])
    assert len(effects) == case['effects'], (case['name'],effects)
    if 'table' in case:
        assert table == case['table'], (case['name'],table)
    for field, prop in [('cursor','Cursor'),('zone','Zone')]:
        if field in case:
            assert state[32768,prop,0] == case[field]
    return dict(name=case['name'], results=results, effects=len(effects), table=table, passed=True)

def function_bytes(raw, name):
    first = raw.index(f'fn {name}(')
    a = raw.index('{', first)
    depth = 1
    b = a + 1
    # The original five function bodies are byte-identical, including strings;
    # matching balanced braces suffices for these frozen no-brace-string fixtures.
    while depth:
        depth += (raw[b] == '{') - (raw[b] == '}')
        b += 1
    return raw[first:b]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--handoff', type=Path, required=True)
    args = parser.parse_args()
    data = Path('docs/evidence/waveform-runtime-cases.json')
    cases = json.loads(data.read_bytes())
    results = [model_case(c,cases['policy']) for c in cases['cases']]
    for c in cases['projection_cases']:
        assert (c['expected'] == c['actual']) == c['accepted']
    official = json.loads((args.handoff/'round11-zone-ui-consumer-requirements.json').read_bytes())
    pairs = []
    for e in official['requirements']['waveform']['authority']:
        if 'zone-commands' in e['url']:
            continue
        archive = Path(e['archive'])
        if not archive.is_absolute():
            archive = args.handoff/'official-docs'/archive.name
        excerpt = Path(e['owned_excerpt_file'])
        assert digest(archive.read_bytes()) == e['document_sha256']
        assert digest(excerpt.read_bytes()) == e['excerpt_sha256']
        assert excerpt.read_text() == e['excerpt']
        pairs.append({k:e[k] for k in ['url','section','document_sha256','excerpt_sha256','authority','applicability','owned_excerpt_file']})
    assert len(pairs) == 5
    head = git('rev-parse','HEAD').decode().strip()
    changed = git('diff','--name-only',BASE,head).decode().splitlines()
    for path in changed:
        if path.endswith('.rs'):
            assert Path(path).read_bytes() == git('show',f'{head}:{path}'), ('Uncommitted source',path)
    source = {p:Path(p).read_text() for p in [
        'crates/sampler-core/src/ops.rs', 'crates/sampler-core/src/waveform.rs',
        'crates/sampler-ksp/src/eval.rs', 'crates/sampler-ksp/src/lower.rs',
        'crates/sampler-ksp/src/lib.rs', 'crates/sampler-ksp/src/waveform.rs']}
    ops = source['crates/sampler-core/src/ops.rs']
    dispatch = ops[ops.index('            Op::Waveform {'):ops.index('            Op::Emit {')]
    assert dispatch.index('waveform::admit') < dispatch.index('return Err(Error::Capacity)') < dispatch.index('waveform::commit') < dispatch.index('self.ops.effects.push_back(effect)')
    assert 'e.plan == plan && e.instance == instance && e.service == service' in dispatch
    assert 'self.ops.effects.back()' in dispatch and 'self.ops.effects.back_mut()' in dispatch
    assert 'e.count == 4' in dispatch
    oldops = git('show',f'{BASE}:crates/sampler-core/src/ops.rs').decode()
    original = lambda s: s[s.index('            Op::Store {'):s.index('            Op::SharedStore {')]
    assert original(oldops) == original(ops), 'Legacy Store runtime semantics changed'
    oldset = oldops[oldops.index('    fn set(&mut self, key:'):oldops.index('/// One script instance')]
    newset = ops[ops.index('    pub(crate) fn set(&mut self, key:'):ops.index('/// One script instance')]
    assert oldset.replace('    fn set(', '    pub(crate) fn set(') == newset
    core = source['crates/sampler-core/src/waveform.rs']
    assert 'store.clear_waveform_table(ui)' in core and 'for (key, _) in initial(ui)' in core
    assert 'store.get(source_key(args[1])) != Some(1)' in core
    assert 'store.get(symbol_key(args[1])).and_then(Property::from_address)' in core
    lower = source['crates/sampler-ksp/src/lower.rs']
    lowering = lower[lower.index('    fn waveform('):lower.index('    fn emit_effect(')]
    assert lowering.count('self.effect_args(') == 1 and 'self.arg(' not in lowering
    assert 'Op::Waveform' in lowering and 'emit_prepared_effect' not in lowering
    lib = source['crates/sampler-ksp/src/lib.rs']
    assert 'effect.plan != plan || effect.instance != Some(instance)' in lib
    assert 'let store_capacity = store.len() + 4096;' in lib
    assert 'waveform::seed(&hir, &init, &environment, &mut store)' in lib
    baseline_lib = git('show', f'{BASE}:crates/sampler-ksp/src/lib.rs').decode()
    bind = function_bytes(baseline_lib, 'bind_modules')
    assert bind == function_bytes(lib, 'bind_modules'), 'Reserved bind_modules callback region changed'
    # New init/getter authority; initial operand decoder remains unchanged.
    ui = 'crates/sampler-ksp/src/ui.rs'
    old_ui = git('show',f'{BASE}:{ui}').decode()
    new_ui = Path(ui).read_text()
    assert old_ui[old_ui.index('    for request in &model.requests {',old_ui.index('fn waveform(')):] == new_ui[new_ui.index('    for request in &model.requests {',new_ui.index('fn waveform(')):]
    original_five = [
        'waveform_four_operand_values_and_slice_indices_are_distinct',
        'waveform_slice_index_bounds_do_not_alias_value_or_allocate_from_value',
        'waveform_requests_do_not_cross_widget_source_identity',
        'waveform_decoder_rejects_malformed_and_unknown_property_requests',
        'waveform_setter_requires_four_operands_despite_vendor_getter_example']
    path = 'crates/sampler-ksp/tests/waveform.rs'
    old_tests = git('show',f'{BASE}:{path}').decode()
    current_tests = Path(path).read_text()
    preserved = []
    for name in original_five:
        previous = function_bytes(old_tests,name)
        assert previous == function_bytes(current_tests,name)
        preserved.append(dict(name=name,sha256=digest(previous.encode()),byte_identical=True))
    assert '#[ignore' not in current_tests
    assert 'UNIMPLEMENTED:' not in Path('crates/sampler-ksp/tests/ui_callbacks.rs').read_text()
    # Actual changed consumers and tests, not guessed graph coordinates.
    pins = [
        named_pin(head,'crates/sampler-core/src/ops.rs','            Op::Waveform {','            Op::Emit {'),
        named_pin(head,'crates/sampler-core/src/ops.rs','    pub(crate) fn can_set(', '/// One script instance'),
        named_pin(head,'crates/sampler-ksp/src/eval.rs','    fn waveform(', '    fn builtin('),
        named_pin(head,'crates/sampler-ksp/src/lower.rs','    fn waveform(', '    fn emit_effect('),
        named_pin(head,'crates/sampler-ksp/src/lib.rs','    pub fn apply_ui_effect_for(', '    /// [`Script::ui`]'),
        named_pin(head,'crates/sampler-ksp/src/lib.rs','    if matches!(service, "attach_zone"', '    if let Some(rest)'),
        named_pin(head,'crates/sampler-ksp/src/lib.rs','    let mut store = Vec::new();', '    let mut text_properties'),
        pin(head,ui,715),
    ]
    for p in ['crates/sampler-core/src/waveform.rs','crates/sampler-ksp/src/waveform.rs',
              'crates/sampler-core/tests/waveform.rs','crates/sampler-ksp/tests/waveform.rs',
              'crates/sampler-ksp/tests/waveform_runtime.rs','crates/sampler-ksp/tests/ui_callbacks.rs']:
        pins.append(pin(head,p))
    pins.extend([pin(head,'crates/sampler-ksp/tests/ui.rs',330,381),
        pin(head,'src/plugin.rs',1727,1795),pin(head,'src/sound/mod.rs',259,275),
        pin(head,'crates/sampler-core/src/ops.rs',929,968)])
    untouched = ['crates/sampler-core/src/behavior.rs','crates/sampler-core/src/stages.rs',
        'crates/sampler-core/src/plan_programs.rs','crates/sampler-core/src/prepare.rs',
        'crates/sampler-core/src/controller_event.rs','crates/sampler-ksp/src/init_cache.rs',
        'src/plugin.rs','src/sound/mod.rs','src/sound/waveform.rs','src/ui/render_art.rs']
    unchanged = []
    for p in untouched:
        assert git('show',f'{BASE}:{p}') == git('show',f'{head}:{p}'), ('Shared file changed',p)
        unchanged.append(pin(head,p))
    catalog = []
    for p in ['crates/sampler-core/tests/waveform.rs','crates/sampler-ksp/tests/waveform.rs',
              'crates/sampler-ksp/tests/waveform_runtime.rs','crates/sampler-ksp/tests/ui_callbacks.rs']:
        for name in re.findall(r'fn (waveform_\w+)\(',Path(p).read_text()):
            catalog.append(dict(path=p,name=name,rust_execution='NOT_RUN'))
    inputs = [dict(path=str(args.handoff/p),sha256=digest((args.handoff/p).read_bytes()))
        for p in ['waveform-operands-ready.md','waveform-operands-ready.json',
            'round11-zone-ui-consumer-requirements.md','round11-zone-ui-consumer-requirements.json',
            'round12-loop-sample-ui-consumer-requirements.md','round12-loop-sample-ui-consumer-requirements.json']]
    print(json.dumps(dict(status='SOURCE_AND_INDEPENDENT_MODEL_CHECKS_ONLY',code_sha=head,base=BASE,
        frozen=FROZEN,model_cases=results,projection_cases=cases['projection_cases'],
        official_archive_excerpt_pairs=pairs,original_five=preserved,source_test_pins=pins,
        unchanged_bind_modules_sha256=digest(bind.encode()),
        unchanged_shared_files=unchanged,test_catalog=catalog,inputs=inputs,
        data_sha256=digest(data.read_bytes()),rust_tests='NOT_RUN',typechecks='NOT_RUN',
        native_runtime='UNKNOWN',cpu_and_ram_below_both_v1_and_kontakt='UNACHIEVED'),indent=2))

if __name__ == '__main__':
    main()
