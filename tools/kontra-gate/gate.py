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

from evidence import redact

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
ROOT = Path.home() / '.cache/kontra-runs'
V1 = Path.home() / '.cache/kontra-v1'
HEAVY = Path.home() / '.cache/kontakto-heavy'
DRIVER = HERE.parent / 'kontra-scan/kontra_scan.py'
METRICS = ['load_ms', 'first_audio_ms', 'peak_rss_mb', 'underruns', 'cpu_p50_us', 'cpu_p99_us', 'family_match', 'widget_gesture_pass', 'fx_slots_dropped', 'filter_slots_dropped', 'mod_slots_dropped', 'signal_graph_trace']


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


def compare(old, new):
    old, new = number(old), number(new)
    return 'UNKNOWN' if old is None or new is None else 'PASS' if new < old else 'FAIL'


def rows(run, version, condition):
    path = run / version / condition / 'results.tsv'
    if not path.exists():
        return {}
    with path.open() as f:
        return {row['path']: row for row in csv.DictReader(f, delimiter='\t')}


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
    axes = {key: [] for key in ['UI', 'DSP', 'UVI', 'scripting', 'beats-v1-every-metric']}
    totals = {version: {'rows': 0, 'original_ok': 0, 'audible': 0} for version in ['v1', 'v2']}
    for condition in ['cold', 'os-warm']:
        pair = {v: rows(run, v, condition) for v in ['v1', 'v2']}
        for path in paths:
            old, new = pair['v1'].get(path, {}), pair['v2'].get(path, {})
            item = hashlib.sha256(path.encode()).hexdigest()
            for version, row in [('v1', old), ('v2', new)]:
                totals[version]['rows'] += bool(row)
                totals[version]['original_ok'] += row.get('ui') == 'original-ok'
                totals[version]['audible'] += row.get('plays_note') == 'yes'
            ui = 'UNKNOWN' if not new else 'PASS' if new.get('ui') == 'original-ok' else 'FAIL'
            audible = 'UNKNOWN' if not new or new.get('audition_status') in ['audition-mismatch', 'fallback-note', 'not-auditioned'] else 'PASS' if new.get('plays_note') == 'yes' else 'FAIL'
            axes['UI'] += [ui, 'UNKNOWN']  # gesture/persistence coverage is not scalar readback
            axes['DSP'] += [audible, 'UNKNOWN']  # native family and complete slot disposition not observed
            if '::' in path:
                axes['UVI'] += [ui, audible, 'UNKNOWN']  # gate subset never certifies the entire corpus
            faults = [new.get(k) for k in ['load_fault_records', 'ksp_runtime_fault_records', 'lua_init_faults', 'lua_runtime_faults', 'lua_budget_hits']]
            known = [number(v) for v in faults if v != 'n/a']
            axes['scripting'].append('FAIL' if any(v and v > 0 for v in known) else 'UNKNOWN' if not known or None in known else 'PASS')
            for metric in METRICS:
                a, b = old.get(metric), new.get(metric)
                if metric in ['load_ms', 'first_audio_ms']:
                    a = a if old.get('loads') == 'yes' else None
                    b = b if new.get('loads') == 'yes' else None
                state = compare(a, b)
                if metric in ['load_ms', 'first_audio_ms', 'peak_rss_mb', 'cpu_p50_us', 'cpu_p99_us']:
                    axes['beats-v1-every-metric'].append(state)
                elif metric == 'underruns':
                    # Zero underruns is an absolute ceiling; equality at zero is valid.
                    axes['beats-v1-every-metric'].append('PASS' if number(a) == number(b) == 0 else state)
                delta = number(b) - number(a) if number(a) is not None and number(b) is not None else None
                observed.append({'item_sha256': item, 'condition': condition, 'metric': metric, 'v1': a, 'v2': b, 'delta': delta, 'verdict': state})
    # Warm product cache, feature parity, full corpus, live plugin/host and native validation remain required.
    axes['beats-v1-every-metric'].append('UNKNOWN')
    axes['scripting'].append('UNKNOWN')
    results = {axis: verdict(states) for axis, states in axes.items()}
    full = verdict(list(results.values()))
    data = {'axes': results, 'verdict': full, 'complete': complete, 'totals': totals, 'metrics': observed}
    write_json(run / 'metrics.json', data)
    text = ['# KONTRA gate', '', f"Source: `{manifest['sha']}`. Run state: {'complete' if complete else 'running'}. Release verdict: **{full}** (UNKNOWN blocks release).", '',
            'This is scanner/probe evidence, not a live CLAP/VST3 host run. No release or installation occurs. The fixed set has ' + str(len(paths)) + ' IDs; all programs in each multi are scanned.', '',
            '| Axis | v1 | v2 | Delta | Verdict |', '| --- | --- | --- | --- | --- |']
    for axis in results:
        a, b = totals['v1'], totals['v2']
        detail = f"Original {a['original_ok']}/{len(paths)*2}; audible {a['audible']}/{len(paths)*2}" if axis in ['UI', 'DSP', 'UVI'] else 'see per-item metrics'
        newdetail = f"Original {b['original_ok']}/{len(paths)*2}; audible {b['audible']}/{len(paths)*2}" if axis in ['UI', 'DSP', 'UVI'] else 'see per-item metrics'
        text.append(f'| {axis} | {detail} | {newdetail} | see metrics.json | {results[axis]} |')
    text += ['', 'Missing evidence: live plugin/DAW perf view; every widget gesture and persistence; native articulation/dynamic-family match (RR as a distribution); CPU p50/p99; complete FX/filter/modulator slot disposition; full Kontakt/UVI and scripting coverage; settings/features parity; product warm-cache acceptance.', '',
             'Cold means product caches disabled/read-only; OS page cache is uncontrolled. os-warm is a second process after the cold pass, still with product caches disabled. It is NOT a product warm-cache measurement. V1 frozen stage/onset probes additionally measure fresh and existing-user-cache paths in probes/.', '',
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
        def visit(value, address, target):
            if isinstance(value, dict):
                for key, val in value.items():
                    if key in ['level_db', 'rms_db', 'peak_db', 'gain_db', 'gain', 'latency', 'latency_frames', 'bypass', 'bypassed'] and isinstance(val, (int, float, bool)):
                        target[address + '/' + key] = val
                    else: visit(val, address + '/' + key, target)
            elif isinstance(value, list):
                for i, val in enumerate(value): visit(val, address + '/' + str(i), target)
        for path in folder.glob('v2/**/plugin-diagnostics.json'):
            # Use the stable item path from the cache rather than a signature containing note plans.
            cache = path.parents[2] / 'cache' / (path.parent.name + '.json')
            if not cache.exists(): continue
            item = json.loads(cache.read_text())['path']
            target = result.setdefault((path.parents[2].name, hashlib.sha256(item.encode()).hexdigest()), {})
            for record in json.loads(path.read_text()).get('files', []):
                if 'signal_graph_trace' in record: visit(record['signal_graph_trace'], '', target)
        return result
    before_stages, after_stages = stages(previous), stages(run)
    text += ['', '## Per-stage signal graph changes', '', '| Item | Condition | Stage/metric | Previous | Current | Delta |', '| --- | --- | --- | ---: | ---: | ---: |']
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
    for condition in ['cold', 'os-warm']:
        out = run / version / condition
        args = [sys.executable, DRIVER, '--engine', engine, '--list', run / 'items.tsv', '--count', '100', '--out', out, '--budget-seconds', '235', '--timeout-seconds', '90']
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
        probe_env.update(PROBE_TEST='uvi::scan::tests::probe_load_onset' if uvi else 'plugin::tests::probe_load', KONTRA_AUDIT_ONSET_ONLY='1')
        for mode in ['v1fresh', 'v1user']:
            args = [sys.executable, HERE.parent / 'audit-load.py', manifest, run / 'probes', binary, mode, '0', str(len(selected))]
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
    args = parser.parse_args()
    sha = subprocess.check_output(['git', '-C', str(REPO), 'rev-parse', args.sha + '^{commit}'], text=True).strip()
    ROOT.mkdir(parents=True, exist_ok=True)
    with (ROOT / '.gate.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        previous = next((p for p in sorted(ROOT.iterdir(), reverse=True) if p.is_dir() and (p / 'metrics.json').exists() and p != args.resume), None)
        run = args.resume or ROOT / (utc() + '-' + sha[:12])
        run.mkdir(exist_ok=bool(args.resume))
        if args.resume and json.loads((run / 'manifest.json').read_text())['sha'] != sha:
            parser.error('resume SHA differs')
        if not args.resume:
            shutil.copyfile(HERE / 'items.tsv', run / 'items.tsv')
        rc = command(['sha256sum', '-c', 'SHA256SUMS'], run / 'v1-integrity.log', V1, heavy=False)
        if rc: raise RuntimeError('frozen v1 integrity check failed')
        binaries = {str(p.relative_to(V1)): sha256(p) for p in V1.rglob('*') if p.is_file() and p.name not in ['README.md', 'SHA256SUMS']}
        manifest = {'sha': sha, 'created_utc': utc(), 'state': 'running', 'binaries': binaries, 'driver_sha256': sha256(DRIVER), 'gate_sha256': sha256(__file__), 'items_sha256': sha256(run / 'items.tsv'), 'profile': 'ci (release optimization, no cross-crate LTO)', 'previous_run': str(previous) if previous else None, 'plugin_host_run': False, 'os_page_cache': 'uncontrolled', 'no_release_or_install': True}
        write_json(run / 'manifest.json', manifest); summarize(run)
        print(run / 'summary.md', flush=True)
        try:
            engine = run / 'bin/kontra-scan-v2'
            if not engine.exists(): engine = prepare(sha, run, args.source)
            manifest['binaries']['v2-scanner'] = sha256(engine); write_json(run / 'manifest.json', manifest)
            env = os.environ.copy()
            env.update(KONTRA_GATE_CAPTURE='1', KONTRA_SCAN_NOTE_ROOT=str(run / 'notes'), KONTRA_SCAN_SIDECAR=str(V1 / 'scan/kontra-scan-v1-uvi'), KONTRA_SCAN_V2_ENGINE=str(engine), KONTRA_REPORT_DIR=str(run / 'reports'), KONTRA_DISABLE_NETWORK='1')
            scans(run, engine, 'v2', env)
            scans(run, V1 / 'scan/kontra-scan-v1', 'v1', env)
            probes(run, env)
            manifest.update(state='complete', completed_utc=utc())
        except Exception as error:
            manifest.update(state='failed', failure_category=type(error).__name__, completed_utc=utc())
            raise
        finally:
            write_json(run / 'manifest.json', manifest)
            result = summarize(run, manifest['state'] == 'complete')
            diff(run, previous)
            append_ledger(ROOT, run, result['verdict'])
        print(run / 'summary.md', flush=True)
        return 0 if result['verdict'] == 'PASS' else 2

if __name__ == '__main__':
    sys.exit(main())
