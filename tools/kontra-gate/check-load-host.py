"""Receipt checks without loading a plugin or library."""
import json
from pathlib import Path
import tempfile
from load_host import SOURCE, receipts

with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    binary = tmp / 'artifact'; binary.write_bytes(b'fixture')
    build = dict(version='0.3.326', source_revision=SOURCE, profile='release', dirty=False)
    rows = [dict(format=f, artifact=str(binary), sha256='a'*64, build=build) for f in ['clap','vst3']]
    cli = dict(artifact=str(binary), sha256='b'*64, build=build)
    artifacts = tmp/'artifacts.json'; native = tmp/'cli.json'
    native.write_text(json.dumps(cli))
    artifacts.write_text(json.dumps(rows))
    assert receipts(artifacts,native)[1] == cli
    for bad in [dict(build, dirty=True), dict(build, source_revision='0'*40), dict(build, profile='ci'), dict(build, version='0.3.325')]:
        native.write_text(json.dumps(dict(cli,build=bad)))
        try:receipts(artifacts,native)
        except AssertionError:pass
        else:raise AssertionError('wrong artifact provenance accepted')
    native.write_text(json.dumps(dict(cli,sha256='x'*64)))
    try:receipts(artifacts,native)
    except AssertionError:pass
    else:raise AssertionError('invalid hash accepted')
print('PASS: exact release provenance, clean source, both plugin formats and hash schema')

# Exercise create-new state export and resume without loading a plugin or library.
import os
import sys
from unittest.mock import patch
import load_host

with tempfile.TemporaryDirectory() as root:
    root = Path(root)
    fixture = root/'artifact'; fixture.write_bytes(b'fixture')
    build = dict(version='0.3.326', source_revision=SOURCE, profile='release', dirty=False)
    rows = [dict(format=f, artifact=str(fixture), sha256='a'*64, build=build) for f in ['clap','vst3']]
    native = dict(artifact=str(fixture), sha256='a'*64, build=build)
    artifacts=root/'artifacts.json'; cli=root/'cli.json'
    artifacts.write_text(json.dumps(rows)); cli.write_text(json.dumps(native))
    request=root/'.cache/kontra-quiet-request';request.parent.mkdir()
    request.write_text(json.dumps(dict(owner='W8 release 0.3.326 load cells')))
    request.with_name('kontra-quiet-granted').touch()
    paths=[(name,str(fixture.with_suffix('.nki'))) for name in ['conflux','pacific','analog']]
    exports=[]; observed=[]
    def run(command, **kwargs):
        if 'export-multi-state' in command:
            destination=Path(command[-1]);destination.open('xb').write(b'native')
            exports.append(destination.name)
    def observe(*args, **kwargs):
        observed.append(args[-1])
        return dict(version=args[-1], status='MEASURED', contention='QUIET')
    out=root/'results'
    common=[patch.object(Path,'home',return_value=root),patch.object(load_host,'selected_paths',return_value=paths),
            patch.object(load_host,'sha',return_value='a'*64),patch.object(load_host,'v1_state',return_value=b'v1'),
            patch.object(load_host.subprocess,'run',side_effect=run),patch.object(load_host,'observe',side_effect=observe),
            patch.dict(os.environ,KONTRA_QUIET_OWNER='1',KONTRA_GATE_REQUIRE_QUIET='1')]
    from contextlib import ExitStack
    with ExitStack() as stack:
        for mock in common:stack.enter_context(mock)
        stack.enter_context(patch.object(sys,'argv',['load_host',str(artifacts),str(cli),str(fixture),str(out),'--run']))
        load_host.main()
    assert len(set(exports))==len(exports)==4 and len(observed)==6
    saved=json.loads((out/'load-cells.json').read_text());saved['cells']=saved['cells'][:2]
    (out/'load-cells.json').write_text(json.dumps(saved));exports.clear();observed.clear()
    with ExitStack() as stack:
        for mock in common:stack.enter_context(mock)
        stack.enter_context(patch.object(sys,'argv',['load_host',str(artifacts),str(cli),str(fixture),str(out),'--run','--resume']))
        load_host.main()
    assert len(observed)==4 and len(json.loads((out/'load-cells.json').read_text())['cells'])==6
print('PASS: unique create-new state files and preserved two-cell resume without duplicate auditions')
