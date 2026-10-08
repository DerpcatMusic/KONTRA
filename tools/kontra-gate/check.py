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
