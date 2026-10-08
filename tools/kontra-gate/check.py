#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import tempfile
import sys
sys.path.insert(0, str(Path(__file__).parent.parent / "kontra-scan"))

spec = importlib.util.spec_from_file_location('gate', Path(__file__).with_name('gate.py'))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)
assert gate.verdict([]) == 'UNKNOWN'
assert gate.verdict(['PASS', 'UNKNOWN']) == 'UNKNOWN'
assert gate.verdict(['PASS', 'FAIL', 'UNKNOWN']) == 'FAIL'
assert gate.compare(10, 10) == 'FAIL'  # strictly beats, ties do not pass
assert gate.compare(None, 0) == 'UNKNOWN'
assert gate.compare(10, 9) == 'PASS'
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    run = root / 'run'; run.mkdir()
    gate.write_json(run / 'manifest.json', {'sha': 'abc'})
    gate.append_ledger(root, run, 'FAIL')
    gate.append_ledger(root, run, 'FAIL')
    assert len((root / 'ledger.tsv').read_text().splitlines()) == 2
assert 'secret' not in json.dumps(gate.redact({'message': 'secret', 'secret': 4, 'count': 2}))
assert gate.redact({'count': 2}) == {'count': 2}
print('gate checks passed')
import adapters
assert adapters.CPU_V1 == Path.home() / '.cache/kontra-scan/cpu-v1/bin/cpu-audit-v1'
adapters.HEAVY = Path('/usr/bin/env')
assert adapters.QUIET_REQUEST == Path.home() / '.cache/kontra-quiet-request'
adapters.QUIET_REQUEST = Path('/proc/self/kontra-test-quiet-absent')
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    records, witness, raw = adapters.capture(['/usr/bin/python3', '-c', 'import sys; print("secret"); print("{\\"block\\":64}"); print("secret",file=sys.stderr)'], root / 'capture')
    assert records == [{'block': 64}] and witness['returncode'] == 0
    assert 'secret' in raw  # runtime-only output for extracting numeric witnesses
    assert 'secret' not in (root / 'capture/plugin-diagnostics.json').read_text()
    root.joinpath('items.tsv').write_text('')
    assert adapters.cpu(root, root)['status'] == 'UNKNOWN'
    assert adapters.gestures(root, root)['status'] == 'UNKNOWN'
    assert adapters.host(root, root)['status'] == 'UNKNOWN'
    records, witness, _ = adapters.capture(['/usr/bin/python3', '-c', 'import os,json; print(json.dumps({"cache":os.environ["XDG_CACHE_HOME"]}))'], root / 'cpu-env', {'XDG_CACHE_HOME': '/dev/null'})
    assert witness['returncode'] == 0 and records == [{'cache': '/dev/null'}]
print('owner adapter checks passed')
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp); (root / 'examples').mkdir()
    root.joinpath('examples/cpu_audit.rs').write_text('fixture')
    binary = root / 'cpu-audit-v1'; binary.write_bytes(b'fixture')
    receipt = {'sha256': adapters.CPU_V1_SHA256, 'source_sha': '0cb7a8a0b4d43086596a64c77320caa1b26d6d98',
               'adapter_commit': '42a0ae9103b1a1cc31a93b3c08e5b86462ccfcad', 'equivalence': 'PASS'}
    root.joinpath('BUILD.json').write_text(json.dumps(receipt))
    saved_binary, saved_hash, saved_capture = adapters.CPU_V1, adapters.CPU_V1_SHA256, adapters.capture
    adapters.CPU_V1 = binary
    adapters.capture = lambda *args, **kwargs: ([], {'returncode': 1}, '')  # never build or benchmark in this check
    try:
        assert adapters.cpu(root, root)['reason'] == 'frozen-v1-cpu-adapter-integrity-failed'
        receipt['sha256'] = adapters.CPU_V1_SHA256 = adapters.hashlib.sha256(binary.read_bytes()).hexdigest()
        root.joinpath('BUILD.json').write_text(json.dumps(receipt))
        admitted = adapters.cpu(root, root)
        assert admitted['reason'] == 'v2-cpu-adapter-build-failed' and admitted['v1_adapter'] == receipt
        root.joinpath('BUILD.json').write_text('{}')
        assert adapters.cpu(root, root)['reason'] == 'frozen-v1-cpu-adapter-integrity-failed'
    finally:
        adapters.CPU_V1, adapters.CPU_V1_SHA256, adapters.capture = saved_binary, saved_hash, saved_capture
print('frozen CPU adapter receipt and digest checks passed')
from evidence import Capture
with tempfile.TemporaryDirectory() as tmp:
    out = Path(tmp)
    capture = Capture(out, {})
    report = capture.root / 'reports'
    report.joinpath('signal-trace.json').write_text('{"nodes":[{"id":1,"gain":0.5}],"edges":[],"blocks":[]}')
    report.joinpath('signal-trace.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg"/>')
    capture.finish()
    assert out.joinpath('signal-trace.json').exists()
    assert out.joinpath('signal-trace.svg').exists()
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    before, after = root / 'before', root / 'after'
    for run, peak in [(before, 0.2), (after, 0.4)]:
        run.mkdir(); (run/'metrics.json').write_text('{"axes":{},"metrics":[]}')
        item = run/'v2/cold/items/signature'; item.mkdir(parents=True)
        cache = run/'v2/cold/cache'; cache.mkdir()
        cache.joinpath('signature.json').write_text('{"path":"fixture"}')
        item.joinpath('plugin-diagnostics.json').write_text('{"files":[]}')
        item.joinpath('signal-trace.json').write_text(json.dumps({'schema':1,'graph':{'nodes':[{'id':7,'kind':'master','processor':'sum'}]},'records':[{'node':7,'frames':64,'enabled':True,'output':{'peak':[peak,peak],'rms':[peak,peak],'dc':[0,0]}}],'complete':True,'dropped':0}))
    gate.diff(after, before)
    result=after.joinpath('diff.md').read_text()
    assert '/node/7/master/sum/coherent/peak/0' in result and '0.2 | 0.4' in result
print('signal trace retention and per-stage diff checks passed')
assert gate.compare(0, 0, optimum=0) == 'PASS'
assert gate.compare(1, 1, optimum=0) == 'FAIL'
import importlib.util
import os
spec = importlib.util.spec_from_file_location('scanner_gate_test', Path(__file__).parent.parent/'kontra-scan/kontra_scan.py')
scanner = importlib.util.module_from_spec(spec); spec.loader.exec_module(scanner)
with tempfile.TemporaryDirectory(prefix='kontra-gate-cache-', dir='/dev/shm') as tmp:
    root=Path(tmp); engine=root/'kontra-scan-v1'
    engine.write_text('#!/usr/bin/env python3\nimport os,json\nfrom pathlib import Path\np=Path(os.environ["XDG_CACHE_HOME"])/"marker"\nhit=p.exists()\np.write_text("fixture")\nprint(json.dumps({"loads":"yes","load_ms":1 if hit else 100,"cache_hit":hit,"scan_active": "KONTRA_SCAN_ACTIVE" in os.environ,"programs":[]}))\n')
    engine.chmod(0o755)
    saved=dict(os.environ)
    try:
        os.environ.update(KONTRA_GATE_CAPTURE='1',KONTRA_GATE_PRODUCT_CACHE_ROOT=tmp,KONTRA_GATE_VERSION='v1',KONTRA_GATE_CACHE_CONDITION='cold')
        cold=scanner.probe(engine,'fixture',root/'out-cold',1,False)
        assert cold['load_ms']==100 and cold['product_cache']['before_files']==0 and not cold['scan_active']
        os.environ['KONTRA_GATE_CACHE_CONDITION']='product-warm'
        warm=scanner.probe(engine,'fixture',root/'out-warm',1,False)
        assert warm['load_ms']==1 and warm['product_cache']['before_files']==1
        os.environ['KONTRA_GATE_CACHE_CONDITION']='os-warm'
        repeat=scanner.probe(engine,'fixture',root/'out-repeat',1,False)
        assert repeat['load_ms']==100 and repeat['product_cache']['before_files']==0
        for p in (root/'v1/cold').glob('*.loaded'):p.unlink()
        os.environ['KONTRA_GATE_CACHE_CONDITION']='product-warm'
        resumed=scanner.probe(engine,'fixture',root/'out-resume',1,False)
        assert resumed['load_ms']==1 and (root/'out-resume/reprime/plugin-diagnostics.json').exists()
    finally:
        os.environ.clear();os.environ.update(saved)
print('empty cache, enabled warm reload, resume re-prime and zero-optimum checks passed')

# Each condition/item uses its own receipt; a global Conflux PASS proves no other cell.
with tempfile.TemporaryDirectory() as tmp:
    run = Path(tmp)
    item = adapters.hashlib.sha256(b'fixture').hexdigest()
    record = {'item_sha256': item, 'condition': 'cold', 'status': 'PASS', 'passed': 2, 'total': 2, 'coverage_complete': True, 'programs': [0], 'faults': 0}
    assert adapters.gesture_cell([record], item, 'cold')['status'] == 'PASS'
    assert adapters.gesture_cell([record], item, 'product-warm')['status'] == 'UNKNOWN'
    assert adapters.gesture_cell([dict(record, passed=1)], item, 'cold')['status'] == 'FAIL'
    assert adapters.gesture_cell([dict(record, coverage_complete=False)], item, 'cold')['status'] == 'UNKNOWN'
    assert adapters.gesture_cell([dict(record, faults=1)], item, 'cold')['status'] == 'FAIL'
print('per-item gesture receipt checks passed')

# A quiet request must prevent submitting a heavy job, including untimed gestures.
from unittest.mock import patch
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp); (root / '.cache').mkdir()
    (root / '.cache/kontra-quiet-request').touch()
    with patch('adapters.QUIET_REQUEST', root / '.cache/kontra-quiet-request'), patch('adapters.subprocess.run') as run:
        records, witness, raw = adapters.capture(['unreachable'], root / 'quiet')
        assert records == [] and raw == '' and witness['returncode'] == 75
        assert witness['reason'] == 'quiet-request-active'
        run.assert_not_called()
print('quiet request admission check passed')

# A repeat load without a production restore witness must never become restored-state evidence.
unknown=scanner.extra_columns({'programs':[], 'gate_condition':'restored-state', 'loads':'yes', 'load_ms':10, 'first_audio_ms':11, 'peak_rss_mb':12})
assert unknown['restored_state_status']=='unknown' and unknown['load_ms']=='unknown'
restored=scanner.extra_columns({'gate_condition':'restored-state', 'loads':'yes', 'load_ms':10, 'restored_state':{'status':'single-init','timer_excludes_seed':True}, 'programs':[{'loaded':True,'source':'kontakt','restored_state':{'status':'single-init','scalar_overrides':3,'init_runs':1,'expected_init_runs':1,'timer_excludes_seed':True}}]})
assert restored['restored_state_status']=='single-init' and restored['script_init_runs']==1 and restored['load_ms']==10
print('restored-state witness checks passed')
with tempfile.TemporaryDirectory() as tmp:
    out=Path(tmp); capture=Capture(out,{})
    capture.stderr.write(b'AUDIT {"stage":"translate_ksp_init","ms":12.5,"hwm_kb":20}\nAUDIT {"stage":"authored secret","ms":1}\n')
    capture.finish()
    stage=json.loads(out.joinpath('load-stages.json').read_text())
    assert stage['stages']==[{'stage':'translate_ksp_init','ms':12.5,'hwm_kb':20}]
    assert 'secret' not in out.joinpath('plugin-diagnostics.json').read_text()
print('numeric audit stage retention checks passed')

with tempfile.TemporaryDirectory() as tmp:
    out=Path(tmp); env={}; capture=Capture(out,env)
    report=capture.root/'reports'; nested=report/'1'; nested.mkdir()
    trace={'schema':1,'graph':{'nodes':[{'id':7}], 'edges':[]},'records':[], 'complete':True,'dropped':0}
    report.joinpath('signal-trace.json').write_text(json.dumps(trace)); nested.joinpath('signal-trace.json').write_text(json.dumps(dict(trace,dropped=1)))
    capture.finish()
    kept=json.loads(out.joinpath('signal-traces.json').read_text())['traces']
    assert {v['path']:v['status'] for v in kept}=={'signal-trace.json':'VALID','1/signal-trace.json':'UNKNOWN'}
    assert out.joinpath('1/signal-trace.json').exists()
print('schema1 runtime retention and complete/drop admission checks passed')

with tempfile.TemporaryDirectory() as tmp:
    root=Path(tmp); engine=root/'engine';engine.write_text('fixture'); items=root/'items.tsv';items.write_text('fixture.nki\n')
    import sys
    saved_probe=scanner.probe; saved_argv=sys.argv; saved_notes=scanner.NOTE_ROOT
    scanner.NOTE_ROOT=root/'notes';scanner.NOTE_ROOT.mkdir()
    def probe(engine,item,work,*args):
        work.mkdir(parents=True);work.joinpath('signal-trace.json').write_text('fixture')
        scanner.note_path(item).write_text('{}')
        return {'loads':'yes','ui':'unknown','programs':[]}
    scanner.probe=probe
    try:
        sys.argv=['scanner','--engine',str(engine),'--list',str(items),'--out',str(root/'results')]
        assert scanner.main()==0
        key=next((root/'results/cache').glob('*.json')).stem
        assert (root/'results/items'/key/'signal-trace.json').exists()
    finally: scanner.probe=saved_probe;sys.argv=saved_argv;scanner.NOTE_ROOT=saved_notes
print('first-load note-plan signature retains matching diagnostics checks passed')
