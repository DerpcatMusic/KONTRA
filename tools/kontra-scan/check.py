#!/usr/bin/env python3
"""Runnable check for shard timeout, per-item reuse, TSV escaping and changed-input invalidation."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

driver = Path(__file__).with_name('kontra_scan.py')
spec = importlib.util.spec_from_file_location('scanner', driver)
scanner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scanner)
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    good, hung = root / 'good.nki', root / 'hung.nki'
    good.touch(); hung.touch()
    manifest = root / 'items.tsv'
    manifest.write_text(f'kontakt\t{good}\nkontakt\t{hung}\n')
    engine = root / 'engine'
    engine.write_text('#!/usr/bin/env python3\nimport sys,json,time\n'
                      'if "hung" in sys.argv[2]: time.sleep(60)\n'
                      'print(json.dumps({"loads":"yes","ui":"original-ok","plays_note":"silent","reason":"x\\ty\\nz"}))\n')
    engine.chmod(0o755)
    args = [sys.executable, str(driver), '--engine', str(engine), '--list', str(manifest),
            '--start', '0', '--count', '2', '--out', str(root / 'out'), '--timeout-seconds', '0.3']
    subprocess.run(args, check=True, stdout=subprocess.DEVNULL)
    records = [json.loads(p.read_text()) for p in (root / 'out/cache').glob('*.json')]
    assert len(records) == 2
    assert any(r['loads'] == 'yes' for r in records)
    assert any(r['timed_out'] and r['loads'] == 'no' for r in records)
    assert len((root / 'out/results.tsv').read_text().splitlines()) == 3
    result = subprocess.run(args, check=True, capture_output=True, text=True)
    assert 'new=0 reused=2' in result.stdout
    old = scanner.signature(str(good), 'revision')
    good.write_text('changed')
    assert scanner.signature(str(good), 'revision') != old
    assert scanner.items(str(manifest)) == [str(good), str(hung)]
print('shared scanner checks passed')
