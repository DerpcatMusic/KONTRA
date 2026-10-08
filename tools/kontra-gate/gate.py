#!/usr/bin/env python3
"""Release evidence ledger. The shared scanner remains the only corpus collector."""
import argparse
import csv
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from evidence import redact

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
ROOT = Path.home() / '.cache/kontra-runs'
V1 = Path.home() / '.cache/kontra-v1'
HEAVY = Path.home() / '.cache/kontakto-heavy'
DRIVER = HERE.parent / 'kontra-scan/kontra_scan.py'
METRICS = ['load_ms', 'first_audio_ms', 'peak_rss_mb', 'underruns', 'nonfinite', 'family_match', 'widget_gesture_pass', 'fx_slots_dropped', 'filter_slots_dropped', 'mod_slots_dropped', 'signal_graph_trace', 'script_init_runs', 'expected_init_runs', 'restored_scalar_overrides']


def utc():
    return datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')


def sha256(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def write_json(path, value):
    tmp = path.with_suffix('.tmp')
    tmp.write_text(json.dumps(value, indent=2) + '\n'); tmp.replace(path)


def verdict(states):
    return 'FAIL' if 'FAIL' in states else 'UNKNOWN' if not states or 'UNKNOWN' in states else 'PASS'


def number(value):
    try:
        n = float(value)
        return n if math.isfinite(n) and n >= 0 else None
    except (TypeError, ValueError):
        return None


def signed_number(value):
    try:
        n = float(value)
        return n if math.isfinite(n) else None
    except (TypeError, ValueError):
        return None


def compare(old, new, optimum=None):
    old, new = number(old), number(new)
    return 'UNKNOWN' if old is None or new is None else 'PASS' if new < old or old == new == optimum else 'FAIL'


def rows(run, version, condition):
    path = run / version / condition / 'results.tsv'
    if not path.exists():
        return {}
    with path.open() as f:
        return {row['path']: row for row in csv.DictReader(f, delimiter='\t')}


def trace_statuses(run, version, condition):
    result = {}
    folder = run / version / condition
    for cached in folder.glob('cache/*.json'):
        evidence = folder / 'items' / cached.stem / 'signal-traces.json'
        if not evidence.exists(): continue
        traces = json.loads(evidence.read_text()).get('traces', [])
        result[json.loads(cached.read_text())['path']] = 'complete' if traces and all(t.get('status') == 'VALID' for t in traces) else 'unknown'
    return result


def append_ledger(root, run, result):
    with (root / 'ledger.tsv').open('a+') as f:
        fcntl.flock(f, fcntl.LOCK_EX); f.seek(0)
        entries = f.read()
        if str(run) in [line.split('\t')[-1] for line in entries.splitlines()[1:]]:
            return
        if not entries:
            f.write('completed_utc\tsha\tverdict\trun_dir\n')
        sha = json.loads((run / 'manifest.json').read_text())['sha']
        f.seek(0, 2); f.write(f'{utc()}\t{sha}\t{result}\t{run}\n'); f.flush(); os.fsync(f.fileno())


def summarize(run, complete=False):
    manifest = json.loads((run / 'manifest.json').read_text())
    paths = [line.split('\t', 1)[1] for line in (run / 'items.tsv').read_text().splitlines()]
    observed = []
    cpu = json.loads((run / 'cpu.json').read_text()) if (run / 'cpu.json').exists() else {}
    gestures = json.loads((run / 'gestures.json').read_text()) if (run / 'gestures.json').exists() else {}
    host = json.loads((run / 'host.json').read_text()) if (run / 'host.json').exists() else {}
    axes = {key: [] for key in ['UI', 'DSP', 'UVI', 'scripting', 'beats-v1-every-metric']}
    totals = {version: {'rows': 0, 'original_ok': 0, 'audible': 0} for version in ['v1', 'v2']}
    for condition in manifest.get('conditions', ['cold', 'os-warm']):
        pair = {v: rows(run, v, condition) for v in ['v1', 'v2']}
        traces = {v: trace_statuses(run, v, condition) for v in ['v1', 'v2']}
        for path in paths:
            old, new = pair['v1'].get(path, {}), pair['v2'].get(path, {})
            item = hashlib.sha256(path.encode()).hexdigest()
            for version, row in [('v1', old), ('v2', new)]:
                totals[version]['rows'] += bool(row)
                totals[version]['original_ok'] += row.get('ui') == 'original-ok'
                totals[version]['audible'] += row.get('plays_note') == 'yes'
            ui = 'UNKNOWN' if not new or new.get('ui') in [None,'unknown',''] else 'PASS' if new.get('ui') == 'original-ok' else 'FAIL'
            audible = 'UNKNOWN' if not new or new.get('audition_status') in ['audition-mismatch', 'fallback-note', 'not-auditioned'] else 'PASS' if new.get('plays_note') == 'yes' else 'FAIL'
            axes['UI'] += [ui, gestures.get('status', 'UNKNOWN') if path.endswith('/Instruments/Conflux.nki') else 'UNKNOWN']  # gesture/persistence coverage is not scalar readback
            axes['DSP'] += [audible, 'UNKNOWN']  # native family and complete slot disposition not observed
            if '::' in path:
                axes['UVI'] += [ui, audible, 'UNKNOWN']  # gate subset never certifies the entire corpus
            faults = [new.get(k) for k in ['load_fault_records', 'ksp_runtime_fault_records', 'lua_init_faults', 'lua_runtime_faults', 'lua_budget_hits']]
            known = [number(v) for v in faults if v != 'n/a']
            if condition == 'restored-state':
                axes['scripting'].append('PASS' if new.get('restored_state_status') == 'single-init' else 'FAIL' if new.get('restored_state_status') == 'duplicate-init' else 'UNKNOWN')
            axes['scripting'].append('FAIL' if any(v and v > 0 for v in known) else 'UNKNOWN' if not known or None in known else 'PASS')
            for metric in METRICS:
                a, b = old.get(metric), new.get(metric)
                if metric in ['load_ms', 'first_audio_ms']:
                    a = a if old.get('loads') == 'yes' else None
                    b = b if new.get('loads') == 'yes' else None
                state = compare(a, b, 0 if metric in ['underruns', 'nonfinite'] else None)
                if metric == 'signal_graph_trace':
                    a, b = (traces[v].get(path, 'unknown') for v in ['v1', 'v2'])
                    state = 'PASS' if b == 'complete' else 'UNKNOWN'
                if manifest.get('signal_trace') and metric in ['load_ms','first_audio_ms','peak_rss_mb']:
                    state='UNKNOWN'  # trace specialization changes performance; diagnostic only
                if metric in ['load_ms', 'first_audio_ms', 'peak_rss_mb', 'cpu_p50_us', 'cpu_p99_us']:
                    axes['beats-v1-every-metric'].append(state)
                elif metric in ['underruns', 'nonfinite']:
                    # Zero underruns is an absolute ceiling; equality at zero is valid.
                    axes['beats-v1-every-metric'].append('PASS' if number(a) == number(b) == 0 else state)
                delta = number(b) - number(a) if number(a) is not None and number(b) is not None else None
                observed.append({'item_sha256': item, 'condition': condition, 'metric': metric, 'v1': a, 'v2': b, 'delta': delta, 'verdict': state})
    cpu_cells = {(cell['item_sha256'], cell['block'], cell['version']): cell for cell in cpu.get('cells', [])}
    for path in paths:
        item = hashlib.sha256(path.encode()).hexdigest()
        for block in [32, 64, 256]:
            for metric in ['cpu_p50_us', 'cpu_p99_us']:
                values = []
                for version in ['v1', 'v2']:
                    cell = cpu_cells.get((item, block, version), {})
                    usable = cell.get('returncode') == 0 and number(cell.get('peak')) is not None and number(cell['peak']) > 1e-5
                    values.append(cell.get(metric) if usable else None)
                a, b = values; state = compare(a, b)
                axes['beats-v1-every-metric'].append(state)
                observed.append({'item_sha256': item, 'condition': f'cpu-runtime-block-{block}', 'metric': metric, 'v1': a, 'v2': b, 'delta': number(b)-number(a) if number(a) is not None and number(b) is not None else None, 'verdict': state})
    host_cells = {(cell['version'], cell['block'], cell['events'], cell['flush']): cell for cell in host.get('cells', [])}
    for block in [32, 64, 256]:
        for events in [0, 1, 64]:
            for flush in [False, True]:
                for metric in ['p50_us', 'p99_us']:
                    a = host_cells.get(('v1', block, events, flush), {}).get(metric)
                    b = host_cells.get(('v2', block, events, flush), {}).get(metric)
                    state = compare(a, b); axes['beats-v1-every-metric'].append(state)
                    observed.append({'item_sha256': 'empty-CLAP', 'condition': f'block-{block}-events-{events}-' + ('flush' if flush else 'process'), 'metric': metric, 'v1': a, 'v2': b, 'delta': number(b)-number(a) if number(a) is not None and number(b) is not None else None, 'verdict': state})
    # Warm product cache, feature parity, full corpus, live plugin/host and native validation remain required.
    axes['beats-v1-every-metric'].append('UNKNOWN')
    axes['scripting'].append('UNKNOWN')
    results = {axis: verdict(states) for axis, states in axes.items()}
    full = verdict(list(results.values()))
    data = {'adapters': {'cpu': cpu.get('reason', 'unwired'), 'gestures': gestures.get('status', 'UNKNOWN'), 'host': host.get('reason', 'unwired'), 'family': 'UNKNOWN until shared scanner exposes selected-family evidence'}, 'axes': results, 'verdict': full, 'complete': complete, 'totals': totals, 'metrics': observed}
    write_json(run / 'metrics.json', data)
    text = ['# KONTRA gate', '', f"Source: `{manifest['sha']}`. Run state: {'complete' if complete else 'running'}. Release verdict: **{full}** (UNKNOWN blocks release).", '',
            'This is scanner/probe evidence, not a live CLAP/VST3 host run. No release or installation occurs. Scope: ' + manifest.get('scope','fixed-acceptance-set') + '. The input set has ' + str(len(paths)) + ' IDs; all programs in each multi are scanned.', '',
            '| Axis | v1 | v2 | Delta | Verdict |', '| --- | --- | --- | --- | --- |']
    for axis in results:
        a, b = totals['v1'], totals['v2']
        detail = f"Original {a['original_ok']}/{len(paths)*len(manifest.get('conditions', ['cold', 'os-warm']))}; audible {a['audible']}/{len(paths)*len(manifest.get('conditions', ['cold', 'os-warm']))}" if axis in ['UI', 'DSP', 'UVI'] else 'see per-item metrics'
        newdetail = f"Original {b['original_ok']}/{len(paths)*len(manifest.get('conditions', ['cold', 'os-warm']))}; audible {b['audible']}/{len(paths)*len(manifest.get('conditions', ['cold', 'os-warm']))}" if axis in ['UI', 'DSP', 'UVI'] else 'see per-item metrics'
        text.append(f'| {axis} | {detail} | {newdetail} | see metrics.json | {results[axis]} |')
    text += ['', 'Cache protocol: ' + manifest.get('cache_protocol', 'legacy-disabled-product-cache (baseline; not product-warm acceptance)'), '', 'Missing evidence: live plugin/DAW perf view; every widget gesture and persistence; native articulation/dynamic-family match (RR as a distribution); CPU p50/p99; complete FX/filter/modulator slot disposition; full Kontakt/UVI and scripting coverage; settings/features parity; product warm-cache acceptance.', '',
             'New protocol: cold has a fresh empty writable private RAM product cache per engine/item; product-warm is the next load with that cache retained. os-warm starts another empty product cache after earlier passes. OS page cache is uncontrolled. Cache file/byte counts before and after live in worker JSON; no cache content is retained. V1 cache-enabled scanner phase hooks are unavailable and marked unknown. Legacy runs retain their original method. Frozen v1 stage/onset probes use the same fresh/retained RAM-cache pairing.', '',
             'Restored-state: Kontakt scalar values are captured from a seed load in memory, then passed into a timed production reload. Load/onset timers exclude the seed; whole-process peak RSS includes both lifecycles. Actual initializer counts must match script counts. Frozen v1 collectors and UVI have no restored-state adapter and stay UNKNOWN. A worker without this witness cannot report restored timings.', '',
             'Plugin logs, session/crash diagnostics and stderr are captured in RAM and retained as numeric fields and hashed text under each item/plugin-diagnostics.json. ui-audit.json contains shared scanner metadata only. Host-only perf/session capture may be absent; absence stays unknown.', '',
             '## Exact frozen references', '', '```json', json.dumps(manifest['binaries'], indent=2), '```', '',
             '## Per-item measurements', '', '| Item hash (12 chars) | Condition | Metric | v1 | v2 | Delta | Verdict |', '| --- | --- | --- | ---: | ---: | ---: | --- |']
    for entry in observed:
        text.append('| ' + ' | '.join(str(entry[k]) if entry[k] is not None else 'unknown' for k in ['item_sha256', 'condition', 'metric', 'v1', 'v2', 'delta', 'verdict']).replace(entry['item_sha256'], entry['item_sha256'][:12]) + ' |')
    (run / 'summary.md').write_text('\n'.join(text) + '\n')
    return data


def diff(run, previous):
    current = json.loads((run / 'metrics.json').read_text())
    text = ['# Diff from previous run', '', f'Previous: {previous or "none (baseline)"}', '']
    if previous:
        old = json.loads((previous / 'metrics.json').read_text())
        indexed = {(x['item_sha256'], x['condition'], x['metric']): x for x in old['metrics']}
        text += ['| Item | Condition | Metric | Previous v2 | Current v2 | Change |', '| --- | --- | --- | ---: | ---: | --- |']
        for x in current['metrics']:
            before = indexed.get((x['item_sha256'], x['condition'], x['metric']), {}).get('v2')
            a, b = number(before), number(x['v2'])
            if a is not None and b is not None and a != b:
                text.append(f"| {x['item_sha256'][:12]} | {x['condition']} | {x['metric']} | {a} | {b} | {'improved' if b < a else 'regressed'} |")
        for axis, state in current['axes'].items():
            text.append(f"\n{axis}: {old['axes'].get(axis, 'UNKNOWN')} → {state}")
    def fingerprints(folder):
        result = set()
        if not folder: return result
        for path in folder.glob('v*/**/plugin-diagnostics.json'):
            for record in json.loads(path.read_text()).get('files', []):
                for line in record.get('lines', []):
                    if isinstance(line, dict):
                        line = {k: v for k, v in line.items() if k not in ['timestamp', 'sequence', 'instance_id']}
                    result.add(hashlib.sha256(json.dumps(line, sort_keys=True).encode()).hexdigest())
        return result
    def stages(folder):
        result = {}
        if not folder: return result
        for path in folder.glob('v2/**/plugin-diagnostics.json'):
            cache=path.parents[2]/'cache'/(path.parent.name+'.json')
            if not cache.exists(): continue
            item=json.loads(cache.read_text())['path']
            target=result.setdefault((path.parents[2].name,hashlib.sha256(item.encode()).hexdigest()),{})
            for trace in path.parent.rglob('signal-trace.json'):
                value=json.loads(trace.read_text())
                if value.get('schema')!=1 or value.get('complete') is not True or value.get('dropped')!=0: continue
                nodes={n['id']:n for n in value.get('graph',{}).get('nodes',[])}
                accumulated={}
                for record in value.get('records',[]):
                    node=record.get('node'); frames=record.get('frames',0)
                    if node not in nodes or not isinstance(frames,int) or frames<=0: continue
                    # Voice contributions and coherent sums are separate observations.
                    key=(node,record.get('contribution') is True)
                    stats=accumulated.setdefault(key,{'frames':0,'peak':[0.,0.],'energy':[0.,0.],'dc':[0.,0.],'enabled_frames':0,'latency_samples':0,'gain_frames':0,'gain':[0.,0.]})
                    metrics=record.get('output',{})
                    if any(not isinstance(metrics.get(k),list) or len(metrics[k])!=2 or any((signed_number(v) if k=='dc' else number(v)) is None for v in metrics[k]) for k in ['peak','rms','dc']): continue
                    gain=record.get('gain',[])
                    if isinstance(gain,list) and len(gain)==2 and all(signed_number(v) is not None for v in gain):
                        stats['gain_frames']+=frames
                        for ch in range(2): stats['gain'][ch]+=gain[ch]*frames
                    stats['frames']+=frames
                    for ch in range(2):
                        stats['peak'][ch]=max(stats['peak'][ch],metrics['peak'][ch])
                        stats['energy'][ch]+=metrics['rms'][ch]**2*frames
                        stats['dc'][ch]+=metrics['dc'][ch]*frames
                    stats['enabled_frames']+=frames*(record.get('enabled') is True)
                    stats['latency_samples']=max(stats['latency_samples'],record.get('latency_samples',0))
                runtime=str(trace.parent.relative_to(path.parent))
                for (node,contribution), stats in accumulated.items():
                    frames=stats['frames']
                    if not frames: continue
                    public=nodes[node]
                    address=f"runtime/{runtime}/node/{node}/{public.get('kind','unknown')}/{public.get('processor','unknown')}/"+('contributions' if contribution else 'coherent')
                    for ch in range(2):
                        target[f'{address}/peak/{ch}']=stats['peak'][ch]
                        target[f'{address}/rms/{ch}']=math.sqrt(stats['energy'][ch]/frames)
                        target[f'{address}/dc/{ch}']=stats['dc'][ch]/frames
                    target[address+'/enabled_fraction']=stats['enabled_frames']/frames
                    target[address+'/latency_samples']=stats['latency_samples']
                    if stats['gain_frames']:
                        measurement=public.get('gain_measurement','unknown')
                        for ch in range(2): target[f'{address}/gain/{measurement}/{ch}']=stats['gain'][ch]/stats['gain_frames']
        return result
    before_stages, after_stages = stages(previous), stages(run)
    text += ['', '## Per-stage signal graph changes', '', '| Item | Condition | Stage/metric | Previous | Current | Delta |', '| --- | --- | --- | ---: | ---: | ---: |']
    text.append('Only schema1, complete, zero-drop traces are compared. RMS/DC are frame-weighted block observations; peaks are maxima. Gain averages are qualified by gain_measurement (unknown on older graphs). Voice contributions remain separate from coherent sums; RR identities are not matched.')
    changes = 0
    for (condition, item), values in after_stages.items():
        for address, value in values.items():
            old = before_stages.get((condition, item), {}).get(address)
            if old is not None and old != value:
                text.append(f'| {item[:12]} | {condition} | {address} | {old} | {value} | {float(value)-float(old)} |'); changes += 1
    if not changes: text.append('Signal graph trace: unknown until W6 exporter is present, or no comparable per-stage changes observed.')
    new = fingerprints(run) - fingerprints(previous)
    text += ['', f'New redacted diagnostic-line fingerprints: {len(new)}.', '', *sorted(new)]
    (run / 'diff.md').write_text('\n'.join(text) + '\n')


def command(args, log, cwd=REPO, env=None, heavy=True):
    with log.open('a') as f:
        return subprocess.run(([str(HEAVY)] if heavy else []) + list(map(str, args)), cwd=cwd, env=env, stdout=f, stderr=subprocess.STDOUT).returncode


def prepare(sha, run, source=None):
    source = Path(source) if source else Path.home() / '.t3/worktrees/KONTAKTO' / ('gate-' + sha[:12])
    if not source.exists():
        subprocess.run(['git', '-C', str(REPO), 'worktree', 'add', '--detach', str(source), sha], check=True, stdout=subprocess.DEVNULL)
    actual = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    if actual != sha or subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise RuntimeError('gate checkout must be clean and exactly the requested SHA')
    artifact = Path('/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target') / ('kontakto-' + source.name) / 'ci/examples/kontra_scan'
    if command(['cargo', 'build', '--profile', 'ci', '--example', 'kontra_scan', '--features', 'shots'], run / 'build.log', source):
        raise RuntimeError('v2 scanner build failed; see build.log')
    if command(['cargo', 'test', '--profile', 'ci', '--example', 'kontra_scan', '--features', 'shots', '--no-run'], run / 'build.log', source):
        raise RuntimeError('v2 scanner compile check failed; see build.log')
    binary = run / 'bin/kontra-scan-v2'; binary.parent.mkdir(exist_ok=True)
    shutil.copy2(artifact, binary)
    return binary


def scans(run, engine, version, env):
    for condition in json.loads((run / 'manifest.json').read_text()).get('conditions', ['cold', 'product-warm', 'os-warm']):
        if condition == 'restored-state' and version == 'v1':
            continue  # Frozen collectors have no host-restore path; missing rows remain UNKNOWN.
        env = dict(env, KONTRA_GATE_CACHE_CONDITION=condition, KONTRA_GATE_VERSION=version)
        out = run / version / condition
        args = [sys.executable, run / 'harness/kontra-scan/kontra_scan.py', '--engine', engine, '--list', run / 'items.tsv', '--count', '100', '--out', out, '--budget-seconds', '235', '--timeout-seconds', '90']
        # Each invocation acquires and releases its own FIFO heavy slot.
        for attempt in range(100):
            rc = command(args, run / f'{version}-{condition}-shards.log', env=env)
            summarize(run)
            if rc == 0: break
            if rc != 75: raise RuntimeError(f'{version} {condition} scanner exited {rc}')
        else: raise RuntimeError('scanner made no bounded completion')
        for cache in (out / 'cache').glob('*.json'):
            record = json.loads(cache.read_text())
            item = out / 'items' / cache.stem; item.mkdir(exist_ok=True)
            write_json(item / 'ui-audit.json', {'programs': [{'program': p.get('program'), 'views': p.get('views', [])} for p in record.get('programs', [])], 'host_gestures': 'unknown', 'perf_view': 'unknown'})


def probes(run, env):
    # Reuse W8's numeric-only audit runner and both immutable v1 probes.
    fromlist = [line.split('\t', 1)[1] for line in (run / 'items.tsv').read_text().splitlines()]
    for uvi in [False, True]:
        selected = [p for p in fromlist if ('::' in p) == uvi]
        manifest = run / ('uvi-probes.tsv' if uvi else 'kontakt-probes.tsv')
        manifest.write_text(''.join(hashlib.sha256(p.encode()).hexdigest() + '\t' + p.replace('::', '/') + '\t0\n' for p in selected))
        binary = V1 / 'bin' / ('v1-uvi-onset-probe' if uvi else 'v1-stage-probe')
        probe_env = env.copy(); probe_env.pop('KONTRA_GATE_CAPTURE', None)
        probe_env['PROBE_PRODUCT_CACHE_ROOT'] = env['KONTRA_GATE_PRODUCT_CACHE_ROOT']
        probe_env.pop('KONTRA_SCAN_ACTIVE', None)
        probe_env.update(PROBE_TEST='uvi::scan::tests::probe_load_onset' if uvi else 'plugin::tests::probe_load', KONTRA_AUDIT_ONSET_ONLY='1')
        for mode in ['v1fresh', 'v1user']:
            args = [sys.executable, run / 'harness/audit-load.py', manifest, run / 'probes', binary, mode, '0', str(len(selected))]
            for attempt in range(100):
                rc = command(args, run / f'{mode}-{uvi}-probes.log', env=probe_env)
                if rc == 0: break
                if rc != 75: raise RuntimeError('v1 onset probe runner failed')
            else: raise RuntimeError('onset probe did not complete')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('sha')
    parser.add_argument('--source', type=Path, help='existing owned clean checkout at exact SHA')
    parser.add_argument('--resume', type=Path)
    parser.add_argument('--items', type=Path, help='targeted diagnostic manifest; default is the fixed acceptance set')
    parser.add_argument('--conditions', nargs='+', choices=['cold','product-warm','os-warm','restored-state'], help='targeted diagnostic conditions; default runs all')
    parser.add_argument('--signal-trace', action='store_true', help='diagnostic trace run; do not use trace-enabled timings for performance acceptance')
    parser.add_argument('--adapter', choices=['cpu', 'gestures', 'host', 'all'], help='run owner adapter(s) on an existing run without repeating scanner cells')
    args = parser.parse_args()
    sha = subprocess.check_output(['git', '-C', str(REPO), 'rev-parse', args.sha + '^{commit}'], text=True).strip()
    ROOT.mkdir(parents=True, exist_ok=True)
    previous = next((p for p in sorted(ROOT.iterdir(), reverse=True) if p.is_dir() and (p / 'metrics.json').exists() and p != args.resume), None)
    run = args.resume or ROOT / (utc() + '-' + sha[:12])
    run.mkdir(exist_ok=bool(args.resume))
    with (run / '.run.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if args.resume and json.loads((run / 'manifest.json').read_text())['sha'] != sha:
            parser.error('resume SHA differs')
        if args.adapter and not args.resume:
            parser.error('--adapter requires --resume')
        if not args.resume:
            shutil.copyfile(args.items or HERE / 'items.tsv', run / 'items.tsv')
        rc = command(['sha256sum', '-c', 'SHA256SUMS'], run / 'v1-integrity.log', V1, heavy=False)
        if rc: raise RuntimeError('frozen v1 integrity check failed')
        if args.adapter:
            if not args.source: parser.error('--adapter requires --source at the exact recorded SHA')
            for leaf in ['manifest.json', 'metrics.json', 'summary.md', 'diff.md']:
                initial = run / ('initial-' + leaf)
                if (run / leaf).is_file() and not initial.exists(): shutil.copyfile(run / leaf, initial)
            import adapters
            manifest = json.loads((run / 'manifest.json').read_text())
            if subprocess.check_output(['git', '-C', str(args.source), 'rev-parse', 'HEAD'], text=True).strip() != sha:
                parser.error('adapter checkout SHA differs')
            for name in (['cpu', 'gestures', 'host'] if args.adapter == 'all' else [args.adapter]):
                getattr(adapters, name)(run, args.source)
            manifest['adapter_completed_utc'] = utc()
            manifest['adapter_source_sha256'] = sha256(HERE / 'adapters.py')
            manifest['summary_generator_sha256'] = sha256(__file__)
            write_json(run / 'manifest.json', manifest)
            result = summarize(run, manifest['state'] == 'complete'); diff(run, previous)
            print(run / 'summary.md', flush=True)
            return 0 if result['verdict'] == 'PASS' else 2
        binaries = {str(p.relative_to(V1)): sha256(p) for p in V1.rglob('*') if p.is_file() and p.name not in ['README.md', 'SHA256SUMS']}
        manifest = {'signal_trace': args.signal_trace, 'sha': sha, 'created_utc': utc(), 'state': 'running', 'binaries': binaries, 'driver_sha256': sha256(DRIVER), 'gate_sha256': sha256(__file__), 'items_sha256': sha256(run / 'items.tsv'), 'source_checkout': str(args.source) if args.source else None, 'evidence_sha256': sha256(HERE / 'evidence.py'), 'conditions': args.conditions or ['cold', 'product-warm', 'os-warm', 'restored-state'], 'scope': 'targeted-diagnostic' if args.items or args.conditions else 'fixed-acceptance-set', 'restored_state_scope': 'Kontakt scalar controls; in-memory seed then timed production reload; v1 and UVI unsupported', 'cache_protocol': 'empty-writable-tmpfs-then-enabled-reload', 'profile': 'ci (release optimization, no cross-crate LTO)', 'previous_run': str(previous) if previous else None, 'plugin_host_run': False, 'os_page_cache': 'uncontrolled', 'no_release_or_install': True}
        if args.resume:
            manifest = json.loads((run / 'manifest.json').read_text())
        write_json(run / 'manifest.json', manifest); summarize(run)
        print(run / 'summary.md', flush=True)
        product_cache = tempfile.TemporaryDirectory(prefix='kontra-gate-cache-', dir='/dev/shm')
        try:
            harness = run / 'harness'
            if not harness.exists():
                (harness / 'kontra-gate').mkdir(parents=True)
                (harness / 'kontra-scan').mkdir()
                for name in ['evidence.py', 'adapters.py', 'gate.py']:
                    shutil.copyfile(HERE / name, harness / 'kontra-gate' / name)
                shutil.copyfile(DRIVER, harness / 'kontra-scan/kontra_scan.py')
                shutil.copyfile(HERE.parent / 'audit-load.py', harness / 'audit-load.py')
            engine = run / 'bin/kontra-scan-v2'
            if not engine.exists(): engine = prepare(sha, run, args.source)
            manifest['binaries']['v2-scanner'] = sha256(engine); write_json(run / 'manifest.json', manifest)
            env = os.environ.copy()
            env.update(KONTRA_GATE_PRODUCT_CACHE_ROOT=product_cache.name, KONTRA_GATE_CAPTURE='1', KONTRA_SCAN_NOTE_ROOT=str(run / 'notes'), KONTRA_SCAN_SIDECAR=str(V1 / 'scan/kontra-scan-v1-uvi'), KONTRA_SCAN_V2_ENGINE=str(engine), KONTRA_REPORT_DIR=str(run / 'reports'), KONTRA_DISABLE_NETWORK='1')
            scans(run, engine, 'v2', env)
            scans(run, V1 / 'scan/kontra-scan-v1', 'v1', env)
            if manifest.get('signal_trace'): env['KONTRA_SIGNAL_TRACE']='1'
            if any(c != 'restored-state' for c in manifest['conditions']): probes(run, env)
            import adapters
            source = Path(manifest['source_checkout']) if manifest.get('source_checkout') else Path.home() / '.t3/worktrees/KONTAKTO' / ('gate-' + sha[:12])
            for name in ['cpu', 'gestures', 'host']:
                getattr(adapters, name)(run, source)
            manifest.update(state='complete', completed_utc=utc())
        except Exception as error:
            manifest.update(state='failed', failure_category=type(error).__name__, completed_utc=utc())
            raise
        finally:
            write_json(run / 'manifest.json', manifest)
            result = summarize(run, manifest['state'] == 'complete')
            diff(run, previous)
            append_ledger(ROOT, run, result['verdict'])
            product_cache.cleanup()
        print(run / 'summary.md', flush=True)
        return 0 if result['verdict'] == 'PASS' else 2

if __name__ == '__main__':
    sys.exit(main())
