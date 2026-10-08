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
    engine.write_text('#!/usr/bin/env python3\nimport sys,json,time,os\n'
                      'if "hung" in sys.argv[2]: time.sleep(60)\n'
                      'assert os.environ.get("KONTRA_UVI_STATIC_PCM_CACHE")=="0"\n'
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
assert scanner.extra_columns({'loads':'yes','plays_note':'silent','ui':'no-ui','programs':[{'pick':None}]})['plays_note']=='no'
# Phase failures, MUI budgets, slot partitions and fixed dictionaries share one exporter.
r=scanner.extra_columns({'ui':'error','programs':[{'source':'kontakt','program':0,'pick':[62,64],
 'ksp':{'compile_ok':'yes','init_ok':'yes','slots':[{'compile_ok':True,'compile_clean':False,'disabled_block_errors':2,'init':{'completion':'completed'},'persistence_changed':{'completion':'failed','fault':{'category':'fuel-budget'}}}]},
 'views':[{'renders':[{'ok':False,'budget_hit':True,'reason':'MUI tree budget'}],'kinds':{'ui_knob':2}}]}],
 'metadata':{'slots':[{'raw_category':'bypassed','compile_disposition':'bypassed','saved_sigils':{'!':1},'saved_histogram_complete':True}]}})
assert r['ui']=='budget-hit' and r['paint_ok']=='no'
assert r['clean_compiled_slots']==0 and r['init_callbacks_completed']==1 and r['persistence_changed_completed']==0
assert r['slots_bypassed']==1 and r['slots_seen']==1
assert json.loads(r['note_picked'])['0']==[62,64]
assert json.loads(r['saved_entry_sigils'])=={'!':1}
assert scanner.extra_columns({'metadata':{'slots':[{'raw_category':'inline_nonempty','saved_sigils':{},'saved_histogram_complete':False}]}})['saved_entry_sigils']=='unknown'
fallback=scanner.extra_columns({'loads':'yes','ui':'original-ok','audition_status':'matched-note-plan','programs':[{'source':'kontakt','pick':[60,64],'pick_source':'fallback'}]})
assert fallback['fallback_note']==1 and fallback['audition_status']=='fallback-note'
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
timing=scanner.extra_columns({'loads':'yes','ui':'original-ok','first_audio_ms':12.5,'cache_state':'cold','programs':[{'source':'kontakt','views':[{'renders':[{'ok':True,'ui_first_frame_ms':20.},{'ok':True,'ui_first_frame_ms':18.}]}]}]})
assert timing['first_audio_ms']==12.5 and timing['ui_first_frame_ms']==18. and timing['cache_state']=='cold'
assert fallback['first_audio_ms']=='unknown' and fallback['ui_first_frame_ms']=='unknown'
missing=scanner.extra_columns({'loads':'yes','ui':'missing-images','reason':'loaded','programs':[{'views':[{'missing_images':2,'asset_failure_reasons':{'lookup-not-found':2},'renders':[{'ok':True}]}]}]})
assert 'lookup-not-found=2' in missing['reason']
assert scanner.extra_columns(missing)['reason']==missing['reason']
pending=scanner.extra_columns({'loads':'yes','ui':'original-ok','ui_first_frame_ms':None,'programs':[{'views':[{'source_presentation':'native-package','font_declared':None,'renders':[{'ok':True,'ui_first_frame_ms':3.0}]}]}]})
assert pending['ui']=='error' and pending['paint_ok']=='no' and pending['ui_first_frame_ms']=='unknown'
native=scanner.extra_columns({'ui':'original-ok','programs':[{'views':[{'source_presentation':'native-package','font_declared':0,'native_diagnostic':'Native runtime; time-budget#safe','renders':[{'ok':True}]}]}]})
assert native['ui']=='budget-hit' and 'time-budget#safe' not in native['reason']
partial=scanner.extra_columns({'loads':'no','ui':'missing-images','reason':'audio audible','programs':[{'stage':'import','error_hash':'opaque'},{'loaded':True}]})
assert 'admission: 1/2 embedded programs failed import/plan construction' in partial['reason']
before=partial['reason'];scanner.extra_columns(partial);assert partial['reason']==before
assert 'opaque' not in partial['reason']
print('shared scanner checks passed')
