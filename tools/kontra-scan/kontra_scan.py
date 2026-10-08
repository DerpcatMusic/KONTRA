#!/usr/bin/env python3
"""One shared, resumable CLI for the optimized v1 and v2 scan adapters. Stdlib only."""
import argparse
import csv
import fcntl
import glob
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

NOTE_ROOT = Path(os.environ.get('KONTRA_SCAN_NOTE_ROOT', Path.home()/'.cache/kontra-scan/notes'))
SIDECAR = Path(os.environ.get('KONTRA_SCAN_SIDECAR', NOTE_ROOT.parent/'bin/kontra-scan-v1-uvi'))
SIDECAR_SHA = hashlib.sha256(SIDECAR.read_bytes()).hexdigest() if SIDECAR.is_file() else None
V2_ENGINE = Path(os.environ.get('KONTRA_SCAN_V2_ENGINE', NOTE_ROOT.parent/'bin/kontra-scan-v2'))
V2_SHA = hashlib.sha256(V2_ENGINE.read_bytes()).hexdigest() if V2_ENGINE.is_file() else None

def note_path(item): return NOTE_ROOT/(hashlib.sha256(item.encode()).hexdigest()+'.json')

COLUMNS = ['path', 'library', 'loads', 'ui', 'controls_bound', 'plays_note', 'load_ms', 'peak_rss_mb', 'reason', 'lua_init_faults', 'lua_init_first', 'lua_runtime_faults', 'lua_runtime_first', 'lua_budget_hits', 'controls_declared', 'controls_bound_declared', 'asset_lookup_requested', 'asset_lookup_ok', 'asset_decode_requested', 'asset_decode_ok', 'font_declared', 'font_success', 'paint_ok', 'paint_error', 'load_path', 'sample_resident_bytes', 'underruns', 'ksp_compile_ok', 'ksp_init_ok', 'first_script_error', 'active_script_slots', 'compiled_script_slots', 'clean_compiled_slots', 'init_callbacks_completed', 'persistence_changed_completed', 'load_fault_records', 'disabled_block_errors', 'widget_kind_counts', 'ui_api_refs', 'bypassed_ui_api_refs', 'saved_entry_sigils', 'custom_font_uses', 'picture_strips', 'picture_frames', 'picture_margins', 'resource_failure_reasons', 'page_background_rgba', 'plain_background_fraction', 'note_picked', 'note_policy', 'audition_status', 'pick_source', 'native_valid_keys', 'native_key_conflicts', 'native_preferred_note', 'slots_seen', 'slots_decode_failed', 'slots_bypassed', 'slots_inline_nonempty', 'slots_linked_only', 'slots_empty', 'admitted_saved_entry_sigils', 'ksp_runtime_fault_records', 'sample_zone_count', 'zero_zone_reason', 'fallback_note', 'keyswitch_picked', 'bound_typed', 'phantom_free_controls', 'first_audio_ms', 'ui_first_frame_ms', 'cache_state']


def library(item):
    path = item.split('::', 1)[0]
    for marker in ['/Libraries/Kontakt/', '/Libraries/UVI/']:
        if marker in path:
            return path.split(marker, 1)[1].split('/', 1)[0]
    return Path(path).parent.name


def items(spec):
    p = Path(spec)
    if p.is_file() and p.suffix.lower() in {'.tsv', '.txt'}:
        rows = [r.split('\t', 1)[-1] for r in p.read_text().splitlines() if r and not r.startswith('#')]
    else:
        rows = sorted(glob.glob(spec, recursive=True))
    return list(dict.fromkeys(rows))


def signature(item, revision):
    path = item.split('::', 1)[0]
    try:
        s = Path(path).stat()
        identity = [item, s.st_size, s.st_mtime_ns, revision]
    except OSError:
        identity = [item, 'absent', revision]
    if '::' in item and SIDECAR_SHA: identity += [SIDECAR_SHA]
    plan=note_path(item)
    identity += ['declared-keys-then-zone-v1', hashlib.sha256(plan.read_bytes()).hexdigest() if plan.exists() else 'unplanned']
    return hashlib.sha256(json.dumps(identity).encode()).hexdigest()


def atomic(path, value):
    tmp = path.with_suffix('.tmp')
    tmp.write_text(json.dumps(value, separators=(',', ':')) + '\n')
    tmp.replace(path)


def extra_columns(r):
    """Unknown is distinct from zero, including old cache records and failed admissions."""
    programs = r.get('programs', [])
    for p in programs:
        if p.get('loaded') is True and 'pick' in p and p['pick'] is None:p['plays_note']='no'
    if r.get('loads')=='yes' and programs and all('pick' in p and p['pick'] is None for p in programs):
        r['plays_note']='no'
        r['reason']=r.get('reason','').replace('audio not audible in 0.5s probe','audio not auditioned')
        if '; no safe audition key' not in r.get('reason',''):r['reason']=r.get('reason','')+'; no safe audition key'
    # A sidecar's override label is not the origin of the common audition pick.
    common = {}
    if r.get('path') and V2_SHA and r.get('revision')!=V2_SHA and bool(programs):
        witness=NOTE_ROOT.parent/'results/v2/cache'/(signature(r['path'],V2_SHA)+'.json')
        if witness.is_file():
            common={p.get('program',i):p for i,p in enumerate(json.loads(witness.read_text()).get('programs',[]))}
    for i,p in enumerate(programs):
        if p.get('pick_source')=='shared-note-plan':
            p['adapter_pick_source']='shared-note-plan'
            origin=common.get(p.get('program',i),{})
            p['pick_source']=origin.get('pick_source','unknown') if origin.get('pick')==p.get('pick') else 'unknown'
    actual=[p for p in programs if isinstance(p.get('pick'),list)]
    r['fallback_note']=int(any(p.get('fallback_note') or p.get('pick_source')=='fallback' or common.get(p.get('program',i),{}).get('pick_source')=='fallback' for i,p in enumerate(actual)))
    if r['fallback_note'] and r.get('audition_status')=='matched-note-plan':r['audition_status']='fallback-note'
    r['zero_zone_reason']=json.dumps({str(p.get('program',i)):p.get('zero_zone_reason') for i,p in enumerate(programs)},separators=(',',':'))
    r['keyswitch_picked']=json.dumps({str(p.get('program',i)):p.get('keyswitch','unknown') for i,p in enumerate(programs)},separators=(',',':'))
    r['phantom_free_controls']='unknown' # baseline compiler does not retain inferred-widget origin
    for p in programs:
        if p.get('source')=='uvi' and 'views' in r:p.setdefault('views',r['views'])
        if p.get('source')=='uvi' and 'sample_resident_bytes' in r:p.setdefault('sample_resident_bytes',r['sample_resident_bytes'])
    views = [v for p in programs for v in p.get('views', [])]
    renders = [x for v in views for x in v.get('renders', [])]
    renders += [v['render'] for v in views if isinstance(v.get('render'),dict)]
    r['first_audio_ms']=r.get('first_audio_ms') if isinstance(r.get('first_audio_ms'),(int,float)) else 'unknown'
    first_frames=[x['ui_first_frame_ms'] for x in renders if isinstance(x.get('ui_first_frame_ms'),(int,float))]
    r['ui_first_frame_ms']=min(first_frames) if first_frames else r.get('ui_first_frame_ms','unknown')
    r['cache_state']=r.get('cache_state','unknown')
    for render in renders:
        if 'error_hash' in render: render.update(ok=False,reason=render.get('stage','paint failure'))
        elif 'pixels' in render: render.setdefault('ok',True)
    lua = [p['lua'] for p in programs if p.get('lua') is not None]
    ksp = [p['ksp'] for p in programs if p.get('ksp') is not None]
    def count(records, key):
        vals=[v[key] for v in records if isinstance(v.get(key), (int,float))]
        return sum(vals) if vals and len(vals)==len(records) else 'unknown'
    for target,key in [('lua_init_faults','init_faults'),('lua_runtime_faults','runtime_faults'),('lua_budget_hits','budget_hits')]:
        r[target] = count(lua,key) if lua else 'n/a' if all(p.get('source')=='kontakt' for p in programs) and programs else 'unknown'
    for target,key in [('lua_init_first','init_first'),('lua_runtime_first','runtime_first')]:
        r[target]=next((x[key] for x in lua if x.get(key)), '')
    for key in ['controls_declared','controls_bound_declared','asset_lookup_requested','asset_lookup_ok','asset_decode_requested','asset_decode_ok','font_declared','font_success','bound_typed']:
        r[key]=count(views,key)
    for key in ['sample_resident_bytes','underruns','sample_zone_count']:
        r[key]=count(programs,key)
    def status(key):
        vals=[x[key] for x in ksp if key in x]
        if not vals: return 'unknown'
        if 'no' in vals: return 'no'
        if 'unknown' in vals: return 'unknown'
        return 'yes' if 'yes' in vals else 'no-scripts'
    r['ksp_runtime_fault_records']=sum(len(p.get('ksp_runtime_faults',[])) for p in programs) if programs else 'unknown'
    r['ksp_compile_ok']=status('compile_ok')
    r['ksp_init_ok']=status('init_ok')
    r['first_script_error']=next((x['first_error'] for x in ksp if x.get('first_error')), next((f.get('core_error') or f.get('category','runtime-fault') for p in programs for f in p.get('ksp_runtime_faults',[])),r.get('first_script_error','')))
    paths=sorted({p['load_path'] for p in programs if p.get('load_path')})
    r['load_path']=' + '.join(paths) or 'unknown'
    errors=[x for x in renders if x.get('ok') is False]
    # v1's retained full-editor render is itself the render record.
    errors += [v for v in views if v.get('ok') is False]
    if any(x.get('budget_hit') for x in errors): r['ui']='budget-hit'
    r['paint_ok']='no' if errors else 'yes' if renders or any(v.get('ok') is True for v in views) else 'no-ui' if r.get('ui')=='no-ui' else 'unknown'
    r['paint_error']=next((x.get('reason','paint error') for x in errors),'')
    slots=[x for p in ksp for x in p.get('slots',[])]
    inventory=r.get('metadata',{}).get('slots',[])
    raw_known=isinstance(r.get('metadata',{}).get('slots'),list)
    for program in programs:
        for slot in program.get('ksp',{}).get('slots',[]):
            slot['program_index']=program.get('program')
            raw=next((x for x in inventory if x.get('program_index')==program.get('program') and x.get('wire_slot')==slot.get('wire_slot') and x.get('owner') in ['standalone-program','embedded-program']),None)
            if raw:
                slot['owner']=raw['owner'];raw['runtime_slot']=slot.get('runtime_slot')
                raw['effective_source_kind']=slot.get('effective_source_kind','inline') if slot.get('compile_disposition','embedded')=='embedded' else 'none'
    for category in ['decode_failed','bypassed','inline_nonempty','linked_only','empty']: r['slots_'+category]=sum(x.get('raw_category')==category for x in inventory) if raw_known else 'unknown'
    r['slots_seen']=len(inventory) if raw_known else 'unknown'
    r['active_script_slots']=sum(x.get('compile_disposition')=='embedded' for x in inventory) if raw_known else 'unknown'
    r['compiled_script_slots']=sum(x.get('compile_ok') is True for x in slots) if ksp else 'unknown'
    r['clean_compiled_slots']=sum(x.get('compile_clean') is True for x in slots) if ksp else 'unknown'
    r['disabled_block_errors']=sum(x.get('disabled_block_errors',0) for x in slots) if ksp else 'unknown'
    for target,phase in [('init_callbacks_completed','init'),('persistence_changed_completed','persistence_changed')]:
        r[target]=sum(x.get(phase,{}).get('completion')=='completed' for x in slots) if ksp else 'unknown'
    r['load_fault_records']=sum(len(p['load_fault_records']) if 'load_fault_records' in p else sum(bool(x.get('init',{}).get('fault'))+bool(x.get('persistence_changed',{}).get('fault')) for x in p.get('slots',[])) for p in ksp) if ksp else 'unknown'
    def combine(records,field):
        result={}
        for x in records:
            for k,v in x.get(field,{}).items(): result[k]=result.get(k,0)+v
        return result
    r['admitted_saved_entry_sigils']=json.dumps(combine(programs,'admitted_saved_entries_by_sigil'),sort_keys=True,separators=(',',':')) if programs else 'unknown'
    for field in ['pick_source','native_valid_keys','native_key_conflicts','native_preferred_note']: r[field]=json.dumps({str(p.get('program',i)):p.get(field) for i,p in enumerate(programs)},separators=(',',':'))
    r['widget_kind_counts']=json.dumps(combine(views,'kinds'),sort_keys=True,separators=(',',':'))
    r['ui_api_refs']=json.dumps(combine([x for x in inventory if x.get('compile_disposition')=='embedded'],'symbols') if inventory else combine(programs,'symbols'),sort_keys=True,separators=(',',':'))
    r['bypassed_ui_api_refs']=json.dumps(combine([x for x in inventory if x.get('bypassed')],'symbols'),sort_keys=True,separators=(',',':')) if raw_known else 'unknown'
    r['saved_entry_sigils']=json.dumps(combine(inventory,'saved_sigils'),sort_keys=True,separators=(',',':')) if raw_known else 'unknown'
    for target,key in [('custom_font_uses','custom_font_uses'),('picture_strips','image_strips'),('picture_frames','image_frames'),('picture_margins','image_margins')]: r[target]=count(views,key)
    r['resource_failure_reasons']=json.dumps(combine(views,'asset_failure_reasons'),sort_keys=True,separators=(',',':'))
    backgrounds=[x['background'] for x in renders if isinstance(x.get('background'),dict)]
    r['page_background_rgba']=json.dumps([x.get('background_rgba') for x in backgrounds],separators=(',',':')) if backgrounds else 'unknown'
    r['plain_background_fraction']=json.dumps([x.get('plain_background_fraction') for x in backgrounds],separators=(',',':')) if backgrounds else 'unknown'
    # The supplied v1 UVI sidecar publishes view/memory metrics at instrument level.
    if any(p.get('source')=='uvi' for p in programs) and 'views' in r:
        for p in programs: p.setdefault('views',r['views'])
        r['sample_resident_bytes']=r.get('sample_resident_bytes','unknown')
    r['note_picked']=json.dumps({str(p.get('program',i)):p.get('pick') for i,p in enumerate(programs)},sort_keys=True,separators=(',',':'))
    r['note_policy']=r.get('note_policy','declared-keys-then-zone-v1')
    r.setdefault('audition_status','unmatched')
    return r


def export(out, revision):
    records = []
    for p in (out / 'cache').glob('*.json'):
        record = json.loads(p.read_text())
        if record.get('revision') == revision and p.stem == signature(record['path'],revision):
            records.append(extra_columns(record))
    tmp = out / 'results.tmp'
    with tmp.open('w') as f:
        writer = csv.writer(f, delimiter='\t', lineterminator='\n')
        writer.writerow(COLUMNS)
        for r in sorted(records, key=lambda r: r['path']):
            writer.writerow([str(r.get(k, '')).replace('\t', ' ').replace('\n', ' ') for k in COLUMNS])
    tmp.replace(out / 'results.tsv')
    return len(records)


def probe(engine, item, work, timeout, shots):
    work.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env['KONTRA_SCAN_ACTIVE'] = '1'
    plan_path=note_path(item)
    if plan_path.exists(): env['KONTRA_SCAN_NOTE_PLAN']=str(plan_path)
    reader = Path('/home/derpcat/.codex/cache/kontakto-uvi-official-reader/app/UVIWorkstationx64.exe')
    if reader.is_file():
        env.setdefault('KONTRA_UVI_READER', str(reader))
    if shots:
        env['KONTRA_SCAN_SHOTS'] = '1'
    worker=engine
    if '::' in item and engine.name=='kontra-scan-v1':
        sidecar=SIDECAR if os.environ.get('KONTRA_SCAN_SIDECAR') else engine.with_name('kontra-scan-v1-uvi')
        if sidecar.exists(): worker=sidecar
    capture = None
    if os.environ.get('KONTRA_GATE_CAPTURE'):
        import importlib.util
        spec = importlib.util.spec_from_file_location('gate_evidence', Path(__file__).resolve().parent.parent / 'kontra-gate/evidence.py')
        evidence = importlib.util.module_from_spec(spec); spec.loader.exec_module(evidence)
        capture = evidence.Capture(work, env)
    with (work / 'stdout.json').open('w') as output:
        child = subprocess.Popen([str(worker), '--worker', item, str(work)], stdout=output,
                                 stderr=capture.stderr if capture else subprocess.DEVNULL, start_new_session=True, env=env)
        started = time.monotonic()
        rss = 0
        timed_out = False
        while child.poll() is None:
            try:
                for line in Path(f'/proc/{child.pid}/status').read_text().splitlines():
                    if line.startswith(('VmHWM:', 'VmRSS:')):
                        rss = max(rss, int(line.split()[1]) / 1024)
            except (OSError, ValueError):
                pass
            if time.monotonic() - started >= timeout:
                timed_out = True
                os.killpg(child.pid, signal.SIGKILL)
                break
            time.sleep(0.1)
        child.wait()
    if capture:
        capture.finish()
    try:
        r = json.loads((work / 'stdout.json').read_text())
    except (ValueError, OSError):
        try:
            r = json.loads((work / 'progress.json').read_text())
        except (ValueError, OSError):
            r = {}
        r.update(loads='no', plays_note='no')
        r.setdefault('ui', 'error')
        r['reason'] = ('timeout' if timed_out else f'worker exit {child.returncode}') + ' at ' + r.get('stage', 'start')
    r['peak_rss_mb'] = round(max(rss, r.get('peak_rss_mb', 0)), 2)
    r['process_ms'] = round((time.monotonic() - started) * 1000, 2)
    r.setdefault('load_ms', r['process_ms'])
    r.setdefault('controls_bound', '0/0')
    r.setdefault('reason', '')
    r['timed_out'] = timed_out
    picks={str(p.get('program',i)):{'key':p['pick'][0],'velocity':p['pick'][1]} for i,p in enumerate(r.get('programs',[])) if isinstance(p.get('pick'),list) and len(p['pick'])==2}
    if picks:
        plan_path.parent.mkdir(parents=True,exist_ok=True)
        with plan_path.with_suffix('.lock').open('w') as note_lock:
            fcntl.flock(note_lock,fcntl.LOCK_EX)
            if not plan_path.exists(): atomic(plan_path,{'programs':picks,'policy':'declared-keys-then-zone-v1'})
            plan=json.loads(plan_path.read_text())
            # Upgrade numeric plan metadata without changing a previously frozen key/velocity.
            if engine.name=='kontra-scan-v2':
                for i,p in enumerate(r.get('programs',[])):
                    target=plan.get('programs',{}).get(str(p.get('program',i)))
                    if target is not None and 'keyswitch' in p:target['keyswitch']=p['keyswitch']
                plan['selection_source']={str(p.get('program',i)):p.get('pick_source','unknown') for i,p in enumerate(r.get('programs',[]))}
                atomic(plan_path,plan)
            wanted={k:{'key':v['key'],'velocity':v['velocity']} for k,v in plan.get('programs',{}).items()}
            r['audition_status']='matched-note-plan' if wanted==picks else 'audition-mismatch'
            r['note_policy']=plan.get('policy','unknown')
    else: r['audition_status']='not-auditioned'
    # stdout is metrics only; keep one canonical cached record, not a second copy.
    (work / 'stdout.json').unlink(missing_ok=True)
    (work / 'progress.json').unlink(missing_ok=True)
    return r


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', required=True)
    parser.add_argument('--list', required=True, help='items TSV or quoted glob (zero-based indexing)')
    parser.add_argument('--start', type=int, default=0)
    parser.add_argument('--count', type=int, default=25)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--budget-seconds', type=float, default=235)
    parser.add_argument('--timeout-seconds', type=float, default=90)
    parser.add_argument('--shots', action='store_true', help='retain small screenshots of OUR Original renderer')
    args = parser.parse_args()
    if args.start < 0 or args.count < 0 or not 0 < args.budget_seconds <= 240:
        parser.error('start/count must be nonnegative; shard budget must be in (0,240]')
    engine = Path(args.engine).resolve()
    revision = hashlib.sha256(engine.read_bytes()).hexdigest()
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'cache').mkdir(exist_ok=True)
    with (args.out / '.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        selected = items(args.list)[args.start:args.start + args.count]
        started = time.monotonic()
        done = reused = 0
        for item in selected:
            key = signature(item, revision)
            cached = args.out / 'cache' / (key + '.json')
            if cached.exists() and json.loads(cached.read_text()).get("audition_status")!="audition-mismatch":
                reused += 1
                continue
            remaining = args.budget_seconds - (time.monotonic() - started)
            if remaining < min(args.timeout_seconds,args.budget_seconds):
                break
            r = probe(engine, item, args.out / 'items' / key, min(args.timeout_seconds, args.budget_seconds), args.shots)
            r.update(path=item, library=library(item), revision=revision)
            extra_columns(r)
            cached=args.out/'cache'/(signature(item,revision)+'.json')
            atomic(cached, r)
            done += 1
            print(f'{done}: {r["loads"]} {r["ui"]} {item}', flush=True)
        total = export(args.out, revision)
        print(f'shard: new={done} reused={reused} selected={len(selected)} total_cached={total} wall={time.monotonic()-started:.1f}s', flush=True)
        return 0 if done + reused == len(selected) else 75


if __name__ == '__main__':
    raise SystemExit(main())
