#!/usr/bin/env python3
"""Bounded source custody and independent scalar models; never execute Rust/KSP."""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import re
import subprocess

PARENT = 'd247e0550b7ce2019d837d7e5c391ee85bb8c687'
CODE = '1cdac230b0b51e28db5843ae1a45d02865228e0b'
HISTORICAL = 'd057af736d48cdbc0af0fca7293756b16a00a252'
FROZEN = '2a243bcfa2262abb789265aa0951fd2a85854023'
ROOT = Path(__file__).resolve().parents[1]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args])


def raw(sha, path):
    return git('show', f'{sha}:{path}')


def pin(sha, path, start=1, end=None):
    data = raw(sha, path)
    lines = data.splitlines(keepends=True)
    end = len(lines) if end is None else end
    return dict(sha=sha, path=path, blob=git('rev-parse', f'{sha}:{path}').decode().strip(),
                start_line=start, end_line=end, file_sha256=digest(data),
                span_sha256=digest(b''.join(lines[start-1:end])))


def body(data, name):
    start = data.index(('fn ' + name + '(').encode())
    opening = data.index(b'{', start)
    level, end = 1, opening + 1
    while level:
        level += (data[end] == 123) - (data[end] == 125)
        end += 1
    return data[start:end]


# Independent models of the observed old rule and intended retained-state rule.
# These functions do NOT interpret source, KSP, Store or Runtime.
def project(requests, request, corrected):
    command, ui, prop, index, value = request
    if command == 'attach':
        requests[:] = [r for r in requests if not (r[1] == ui and r[0] in ('attach', 'set'))]
    elif command == 'set':
        if corrected:
            if requests and requests[-1] == request:
                return
            requests[:] = [r for r in requests if not (
                r[0:3] == request[0:3] and (prop != 'Table' or r[3] == index))]
        else:
            found = next((i for i in range(len(requests)-1, -1, -1)
                          if requests[i][0:4] == request[0:4]), None)
            if found is not None:
                requests[found] = request
                return
    requests.append(request)


def decoded(requests, ui, prop, index=0):
    value = -1 if prop == 'Highlight' else 0
    for r in requests:
        if r[1] != ui:
            continue
        if r[0] == 'attach':
            value = -1 if prop == 'Highlight' else 0
        elif r[0] == 'set' and r[2] == prop and (prop != 'Table' or r[3] == index):
            value = r[3] if prop == 'Highlight' else r[4]
    return value


def index_ok(prop, index):
    return (0 <= index < 65536 if prop == 'Table' else
            -1 <= index < 65536 if prop == 'Highlight' else index == 0)


def seed_ok(stream):
    owned = {32768: '$w', 32769: '$Imported'}
    sources, attached = {27, 91}, set()
    for command, args in stream:
        if command not in ('attach_zone', 'set_ui_wf_property'):
            continue
        if not args:
            return False
        ui = next((ui for ui, name in owned.items() if
                   (type(args[0]) is int and args[0] == ui)
                   or (type(args[0]) is str and args[0] == name)), None)
        if ui is None:
            return False
        if command == 'attach_zone':
            if len(args) != 3 or not all(type(v) is int for v in args[1:]):
                return False
            if args[1] <= 0 or args[1] not in sources:
                return False
            attached.add(ui)
        else:
            if (len(args) != 4 or ui not in attached or type(args[1]) is not str
                    or args[1] not in ('Cursor', 'Flags', 'MidiStart', 'Highlight', 'Table')
                    or not all(type(v) is int for v in args[2:]) or not index_ok(args[1], args[2])):
                return False
    return True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--handoff', type=Path, required=True)
    h = parser.parse_args().handoff
    assert git('rev-parse', CODE + '^').decode().strip() == PARENT
    assert git('rev-parse', PARENT + '^{tree}').decode().strip() == '6807036133b3f681ad88904f5e0dd4e218201fae'
    assert git('rev-parse', HISTORICAL + '^{tree}').decode().strip() == '607095193e54a124ea7b632ec0f09c1044a9dea8'
    assert digest((h/'waveform-runtime-source-review.json').read_bytes()) == 'cfad205e62c53994bf6fbd43d07be0ece0026114c728f3672cf989c0880388bc'
    changed = git('diff', '--name-only', PARENT, CODE).decode().splitlines()
    expected = ['crates/sampler-core/src/ops.rs', 'crates/sampler-core/src/waveform.rs',
                'crates/sampler-core/tests/waveform.rs', 'crates/sampler-ksp/src/waveform.rs',
                'crates/sampler-ksp/tests/waveform_runtime.rs']
    assert changed == expected
    for p in changed:
        assert (ROOT/p).read_bytes() == raw(CODE, p)
    ready = json.loads((h/'waveform-runtime-ready.json').read_bytes())
    preserved = []
    for t in ready['test_catalog']:
        p, name = t['path'], t['name']
        before, after = body(raw(PARENT, p), name), body(raw(CODE, p), name)
        assert before == after
        preserved.append(dict(path=p, name=name, body_sha256=digest(after), status='BYTE_IDENTICAL_NOT_RUN'))
    # The adjacent typed-seed test makes the reviewer catalog 23, not the producer's 22.
    name = 'typed_seed_meter_and_waveform_addresses_reach_ir'
    p = 'crates/sampler-ksp/tests/ui.rs'
    assert body(raw(PARENT, p), name) == body(raw(CODE, p), name)
    preserved.append(dict(path=p, name=name, body_sha256=digest(body(raw(CODE, p), name)), status='BYTE_IDENTICAL_NOT_RUN'))
    assert len(preserved) == 23
    for p in ready['unchanged_shared_files']:
        assert raw(CODE, p['path']) == raw(PARENT, p['path'])
    for p in ['crates/sampler-ksp/src/eval.rs', 'crates/sampler-ksp/src/ui.rs',
              'crates/sampler-ksp/src/lib.rs', 'crates/sampler-ksp/src/lower.rs',
              'crates/sampler-core/src/lib.rs']:
        assert raw(CODE, p) == raw(PARENT, p)
    for p in ['crates/sampler-core/tests/waveform.rs', 'crates/sampler-ksp/tests/waveform_runtime.rs']:
        assert raw(CODE, p).startswith(raw(PARENT, p))
    primary = []
    auth = json.loads((h/'round11-zone-ui-consumer-requirements.json').read_bytes())['requirements']['waveform']['authority']
    for e in auth:
        if 'zone-commands' in e['url']:
            continue
        archive = Path(e['archive'])
        if not archive.is_absolute():
            archive = h/'official-docs'/archive.name
        excerpt = Path(e['owned_excerpt_file'])
        assert digest(archive.read_bytes()) == e['document_sha256']
        assert digest(excerpt.read_bytes()) == e['excerpt_sha256']
        assert excerpt.read_text() == e['excerpt']
        primary.append(dict(url=e['url'], section=e['section'], archive_path=str(archive),
                            archive_sha256=e['document_sha256'], excerpt_path=str(excerpt),
                            excerpt_sha256=e['excerpt_sha256'], authority='PRIMARY_SPEC_ONLY', native='UNKNOWN'))
    assert len(primary) == 5
    # Tie finite models to the exact changed source mechanisms, without claiming execution.
    ksp = raw(CODE, 'crates/sampler-ksp/src/waveform.rs').decode()
    core = raw(CODE, 'crates/sampler-core/src/waveform.rs').decode()
    ops = raw(CODE, 'crates/sampler-core/src/ops.rs').decode()
    assert 'model.requests.retain(|r| !same_address(r))' in ksp and 'Some(Property::Table)' in ksp
    assert ksp.index('for request in &init.model.requests') < ksp.index('store.push')
    assert '.validate_index(*slice)?' in ksp and 'attached[index]' in ksp
    assert 'i32::try_from(zone)' in core and 'zone <= 0' in core and 'source_key(zone as i32)' not in core
    dispatch = ops[ops.index('            Op::Waveform { action, args, local, services } => {'):]
    assert dispatch.index('args.checked_add((count - 1) as u16)') < dispatch.index('self.reg(') < dispatch.index('waveform::admit')
    old_ksp = raw(PARENT, 'crates/sampler-ksp/src/waveform.rs').decode()
    assert 'r.args.get(1..3) == request.args.get(1..3)' in old_ksp
    assert 'validate_index' not in old_ksp
    checks = []
    for indices in ([3, 4, 3], [-1, 3, -1]):
        old, new = [], []
        for index in indices:
            request = ('set', 32768, 'Highlight', index, index)
            project(old, request, False); project(new, request, True)
        assert decoded(old, 32768, 'Highlight') != indices[-1]
        assert decoded(new, 32768, 'Highlight') == indices[-1]
        checks.append(dict(finding='WF-R1', input=indices, historical=decoded(old,32768,'Highlight'), corrected=decoded(new,32768,'Highlight')))
    for indices in itertools.product([-1, 0, 3, 4, 65535], repeat=4):
        requests = []
        for index in indices:
            project(requests, ('set', 32768, 'Highlight', index, index), True)
        assert len(requests) == 1 and decoded(requests,32768,'Highlight') == indices[-1]
    requests = []
    stream = [('attach',32768,None,27,3), ('set',32768,'Highlight',3,3),
              ('set',32768,'Table',3,77), ('other',0,None,0,5),
              ('set',32769,'Highlight',9,9), ('set',32768,'Table',4,88),
              ('set',32768,'Table',3,99), ('set',32768,'Highlight',4,4),
              ('set',32768,'Highlight',3,3)]
    for request in stream:
        project(requests, request, True)
    assert decoded(requests,32768,'Highlight') == 3 and decoded(requests,32769,'Highlight') == 9
    assert decoded(requests,32768,'Table',3) == 99 and decoded(requests,32768,'Table',4) == 88
    assert [r for r in requests if r[0] == 'other' or r[1] == 32769] == [stream[3],stream[4]]
    project(requests, ('attach',32768,None,91,11), True)
    assert decoded(requests,32768,'Highlight') == -1 and decoded(requests,32768,'Table',3) == 0
    assert decoded(requests,32769,'Highlight') == 9
    base = [('attach_zone',[32768,27,3])]
    negatives = [[32768,'Highlight',i,1] for i in [-2,65536,2147483647]]
    negatives += [[32768,'Table',i,77] for i in [-1,65536]]
    negatives += [[32768,'Cursor',1,99], [32768,4,3,1], [32768,'Unknown',0,1],
                  [32768,'Highlight','bad',1], [32768,'Highlight',3,'bad'],
                  [32768,'Highlight',3], [32768,'Highlight',3,1,9],
                  [32770,'Highlight',3,1], ['$Missing','Highlight',3,1], [32768.0,'Highlight',3,1]]
    for args in negatives:
        assert not seed_ok(base + [('set_ui_wf_property', args)])
    for zone in [-1,0,28]:
        assert not seed_ok([('attach_zone',[32768,zone,3])])
    assert not seed_ok([('set_ui_wf_property',[32768,'Highlight',3,1])])
    assert not seed_ok(base + [('set_ui_wf_property',[32768,'Highlight',65536,1]),('attach_zone',[32768,91,11])])
    for index in [-1,0,65535]:
        assert seed_ok([('attach_zone',['$Imported',91,0]),('set_ui_wf_property',['$Imported','Highlight',index,1])])
    for index in [0,65535]:
        assert seed_ok(base + [('set_ui_wf_property',[32768,'Table',index,77])])
    checks.append(dict(finding='WF-R2', historical_cached_highlight_65536='PROMOTED_BY_SOURCE_RULE', corrected='REJECTED_BY_MODEL', negative_schema_cases=len(negatives)+5, accepted_boundaries=[-1,0,65535]))
    zones = []
    for zone in [(1<<32)+27, (1<<63)-1, 0, -27, -1, 27, (1<<31)-1]:
        markers = {0,-27,-1,27,(1<<31)-1}
        narrowed = (zone+(1<<31))%(1<<32)-(1<<31)
        old = zone >= 0 and narrowed in markers
        new = 0 < zone <= (1<<31)-1 and zone in markers
        assert new == (zone in [27,(1<<31)-1])
        zones.append(dict(zone=zone,historical_admitted=old,corrected_admitted=new))
    checks.append(dict(finding='WF-R3', cases=zones))
    windows = []
    for action,count,edge,overflow in [('Set',4,65532,65533),('Get',3,65533,65534),('Attach',3,65533,65535)]:
        assert edge+count-1 == 65535 and overflow+count-1 > 65535
        windows.append(dict(action=action,count=count,valid_edge=edge,overflow_start=overflow,mathematical_last=overflow+count-1,wrapped_last=(overflow+count-1)%65536,corrected='REJECT_BEFORE_READ'))
    checks.append(dict(finding='WF-R4', cases=windows))
    paths = sorted(set(t['path'] for t in preserved))
    catalog = []
    for p in paths:
        data = raw(CODE,p)
        for m in re.finditer(rb'(?m)^fn (waveform_\w+|typed_seed_meter_and_waveform_addresses_reach_ir)\(',data):
            name = m.group(1).decode()
            start = data[:m.start()].count(b'\n') + 1
            catalog.append(dict(path=p,name=name,start_line=start,
                                end_line=start+body(data,name).count(b'\n'),
                                body_sha256=digest(body(data,name)),execution='NOT_RUN',typecheck='NOT_RUN',
                                feature='cache' if name.startswith('waveform_cache') else 'default'))
    assert len(catalog) == 35
    print(json.dumps(dict(status='SOURCE_READY_FOR_FRESH_REVIEW',kind='SOURCE_CUSTODY_AND_INDEPENDENT_SCALAR_MODELS_NOT_EXECUTION',
        parent=PARENT,parent_tree=git('rev-parse',PARENT+'^{tree}').decode().strip(),code=CODE,
        code_tree=git('rev-parse',CODE+'^{tree}').decode().strip(),historical_source=HISTORICAL,frozen=FROZEN,
        changed_full_file_pins=[pin(CODE,p) for p in changed],parent_full_file_pins=[pin(PARENT,p) for p in changed],
        focused_source_pins=[pin(CODE,'crates/sampler-core/src/ops.rs',1756,1793),
                            pin(CODE,'crates/sampler-core/src/waveform.rs',93,117),
                            pin(CODE,'crates/sampler-ksp/src/waveform.rs',18,38),
                            pin(CODE,'crates/sampler-ksp/src/waveform.rs',43,119)],
        complete_subject_rust_pins=[pin(CODE,p['path']) for p in ready['source_test_pins'] if p['path'].endswith('.rs')],
        exact_diff_sha256=digest(git('diff','--no-ext-diff','--binary',PARENT,CODE)),
        unchanged_shared_files=ready['unchanged_shared_files'],preserved23=preserved,test_catalog=catalog,
        primary_pairs=primary,checks=checks,highlight_four_write_model_cases=625,
        immutable_inputs=[dict(path=str(h/p),sha256=digest((h/p).read_bytes())) for p in [
            'waveform-runtime-source-review.md','waveform-runtime-source-review.json',
            'root-waveform-runtime-source-review-check.json','waveform-runtime-ready.md','waveform-runtime-ready.json']],
        checker_sha256=digest(Path(__file__).read_bytes()),rust_tests='NOT_RUN',rust_typechecks='NOT_RUN',native='UNKNOWN',
        cpu_and_ram_below_both_v1_and_kontakt='UNACHIEVED'),indent=2))


if __name__ == '__main__':
    main()
