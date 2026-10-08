#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import tempfile

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
adapters.HEAVY = Path('/usr/bin/env')
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
print('owner adapter checks passed')
from evidence import Capture
with tempfile.TemporaryDirectory() as tmp:
    out = Path(tmp)
    capture = Capture(out, {})
    report = capture.root / 'reports'
    report.joinpath('signal-trace.json').write_text('{"nodes":[{"id":1,"gain":0.5}],"edges":[],"blocks":[]}')
    report.joinpath('signal-trace.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg"/>')
    capture.finish()
    assert json.loads(out.joinpath('signal-trace.json').read_text())['nodes'][0]['gain'] == 0.5
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
        item.joinpath('signal-trace.json').write_text(json.dumps({'nodes':[{'id':7,'peak':peak}]}))
    gate.diff(after, before)
    result=after.joinpath('diff.md').read_text()
    assert '/nodes/7/peak' in result and '0.2 | 0.4' in result
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
# A repeat load without a production restore witness must never become restored-state evidence.
unknown=scanner.extra_columns({'programs':[], 'gate_condition':'restored-state', 'loads':'yes', 'load_ms':10, 'first_audio_ms':11, 'peak_rss_mb':12})
assert unknown['restored_state_status']=='unknown' and unknown['load_ms']=='unknown'
restored=scanner.extra_columns({'gate_condition':'restored-state', 'loads':'yes', 'load_ms':10, 'restored_state':{'status':'single-init','timer_excludes_seed':True}, 'programs':[{'loaded':True,'source':'kontakt','restored_state':{'status':'single-init','scalar_overrides':3,'init_runs':1,'expected_init_runs':1,'timer_excludes_seed':True}}]})
assert restored['restored_state_status']=='single-init' and restored['script_init_runs']==1 and restored['load_ms']==10
print('restored-state witness checks passed')
