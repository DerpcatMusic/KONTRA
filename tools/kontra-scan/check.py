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
    subprocess.run(args,check=True,stdout=subprocess.DEVNULL)
    assert len((root/'out/results.tsv').read_text().splitlines())==3 # stale signature never duplicates a row
    assert scanner.items(str(manifest)) == [str(good), str(hung)]
# Phase failures, MUI budgets, slot partitions and fixed dictionaries share one exporter.
r=scanner.extra_columns({'ui':'error','programs':[{'source':'kontakt','program':0,'pick':[62,64],
 'ksp':{'compile_ok':'yes','init_ok':'yes','slots':[{'compile_ok':True,'compile_clean':False,'disabled_block_errors':2,'init':{'completion':'completed'},'persistence_changed':{'completion':'failed','fault':{'category':'fuel-budget'}}}]},
 'views':[{'renders':[{'ok':False,'budget_hit':True,'reason':'MUI tree budget'}],'kinds':{'ui_knob':2}}]}],
 'metadata':{'slots':[{'raw_category':'bypassed','compile_disposition':'bypassed','saved_sigils':{'!':1}}]}})
assert r['ui']=='budget-hit' and r['paint_ok']=='no'
assert r['clean_compiled_slots']==0 and r['init_callbacks_completed']==1 and r['persistence_changed_completed']==0
assert r['slots_bypassed']==1 and r['slots_seen']==1
assert json.loads(r['note_picked'])['0']==[62,64]
assert json.loads(r['saved_entry_sigils'])=={'!':1}
with tempfile.TemporaryDirectory() as tmp:
    scanner.NOTE_ROOT=Path(tmp)
    item='same item'
    before=scanner.signature(item,'r')
    scanner.atomic(scanner.note_path(item),{'programs':{'0':{'key':62,'velocity':64}},'policy':'declared-keys-then-zone-v1'})
    assert before!=scanner.signature(item,'r')
    scanner.NOTE_ROOT=Path(tmp)/'notes'; scanner.V2_SHA='v2'
    cache=Path(tmp)/'results/v2/cache'; cache.mkdir(parents=True)
    scanner.atomic(cache/(scanner.signature(item,'v2')+'.json'),{'programs':[{'program':0,'pick':[62,64],'pick_source':'native_declared'}]})
    row=scanner.extra_columns({'path':item,'ui':'no-ui','programs':[{'source':'uvi','program':0,'pick':[62,64],'pick_source':'shared-note-plan'}]})
    assert json.loads(row['pick_source'])=={'0':'native_declared'}
    assert row['programs'][0]['adapter_pick_source']=='shared-note-plan'
    row=scanner.extra_columns({'path':item,'ui':'no-ui','programs':[{'source':'uvi','program':0,'pick':[40,64],'pick_source':'shared-note-plan'}]})
    assert json.loads(row['pick_source'])=={'0':'unknown'} # unequal notes never inherit native-valid provenance
print('shared scanner checks passed')
