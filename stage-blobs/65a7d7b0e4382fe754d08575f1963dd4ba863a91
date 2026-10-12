"""Adapters to the owners' existing CPU, gesture and CLAP probes; no corpus collector."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from evidence import Capture, redact
from contention import wait_for_quiet
HEAVY = Path.home() / '.cache/kontakto-heavy'
QUIET_REQUEST = Path.home() / '.cache/kontra-quiet-request'
V1 = Path.home() / '.cache/kontra-v1'
CPU_V1 = Path.home() / '.cache/kontra-scan/cpu-v1/bin/cpu-audit-v1'
CPU_V1_SHA256 = 'b9998ca2ce2f2ed4f9f88bbfb11c5e884fa162a87cdf89f26ece6f1248fdc6ab'
GESTURE = 'ui::widget_gate::original_widget_gestures'


def capture(args, folder, env=None, cwd=None, timeout=235, timed=False):
    folder.mkdir(parents=True, exist_ok=True)
    child_env = dict(os.environ, **(env or {}))
    evidence = Capture(folder, child_env)
    if env and 'XDG_CACHE_HOME' in env:
        child_env['XDG_CACHE_HOME'] = env['XDG_CACHE_HOME']
    # Runtime output stays in RAM until reduced to numeric JSON and test witnesses.
    with tempfile.TemporaryFile(dir='/dev/shm') as output:
        try:
            worker = [sys.executable, HERE / 'contention.py', folder, *args] if timed else args
            while True:
                if QUIET_REQUEST.exists():
                    return [], {'stdout_sha256': hashlib.sha256(b'').hexdigest(), 'returncode': 75,
                                'lines': 0, 'reason': 'quiet-request-active'}, ''
                if timed and child_env.get('KONTRA_GATE_REQUIRE_QUIET') == '1': wait_for_quiet(folder)
                job = subprocess.run([str(HEAVY), 'timeout', str(timeout), *map(str, worker)], stdout=output, stderr=evidence.stderr, env=child_env, cwd=cwd)
                activity = json.loads((folder / 'activity.json').read_text()) if timed and (folder / 'activity.json').exists() else {}
                if not (timed and job.returncode == 75 and activity.get('status') == 'WAITING'): break
            output.seek(0); raw = output.read().decode(errors='replace')
            records = []
            for line in raw.splitlines():
                try:
                    row = json.loads(line)
                    if isinstance(row, dict): records.append(row)
                except ValueError: pass
            witness = {'stdout_sha256': hashlib.sha256(raw.encode()).hexdigest(), 'returncode': job.returncode, 'lines': len(raw.splitlines())}
            if timed: witness['contention'] = activity.get('status', 'UNKNOWN')
            return records, witness, raw
        finally:
            evidence.finish()


def cpu(run, source):
    frozen = CPU_V1
    example = source / 'examples/cpu_audit.rs'
    result = {'status': 'UNKNOWN', 'cells': [], 'reason': 'frozen-v1-adapter-or-v2-example-absent'}
    if frozen.is_file() and example.is_file():
        try:
            receipt = json.loads(frozen.with_name('BUILD.json').read_text())
        except (OSError, ValueError):
            receipt = {}
        if (not isinstance(receipt, dict) or receipt.get('sha256') != CPU_V1_SHA256
                or hashlib.sha256(frozen.read_bytes()).hexdigest() != CPU_V1_SHA256
                or receipt.get('source_sha') != '0cb7a8a0b4d43086596a64c77320caa1b26d6d98'
                or receipt.get('adapter_commit') != '42a0ae9103b1a1cc31a93b3c08e5b86462ccfcad'
                or receipt.get('equivalence') != 'PASS'):
            result['reason'] = 'frozen-v1-cpu-adapter-integrity-failed'
            (run / 'cpu.json').write_text(json.dumps(result, indent=2) + '\n')
            return result
        result['v1_adapter'] = receipt
        records, witness, _ = capture(['cargo', 'build', '--profile', 'ci', '--example', 'cpu_audit'], run / 'cpu/build', cwd=source)
        if witness['returncode']:
            result['reason'] = 'v2-cpu-adapter-build-failed'
        else:
            target = Path('/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target') / ('kontakto-' + source.name) / 'ci/examples/cpu_audit'
            binary = run / 'bin/cpu-audit-v2'
            import shutil
            shutil.copy2(target, binary)
            paths = [line.split('\t', 1)[1] for line in (run / 'items.tsv').read_text().splitlines()]
            for path in paths:
                if '::' in path or Path(path).suffix.lower() == '.nkm': continue  # existing v1 adapter cannot address these
                item = hashlib.sha256(path.encode()).hexdigest()
                scenario = 'piano' if '/Una Corda ' in path else 'fx' if '/ANALOG STRINGS/' in path else 'strings'
                for block in [32, 64, 256]:
                    for version, exe in [('v2', binary), ('v1', frozen)]:
                        out = run / 'cpu' / f'{item}-{block}-{version}'
                        metric = out / 'metrics.json'
                        if metric.exists():
                            cached = json.loads(metric.read_text())
                            if os.environ.get('KONTRA_GATE_REQUIRE_QUIET') != '1' or cached.get('contention') == 'QUIET':
                                result['cells'].append(cached); continue
                        records, witness, _ = capture([exe, path, block, scenario], out, {'XDG_CACHE_HOME': '/dev/null'}, timed=True)
                        row = next((r for r in records if r.get('block') == block and isinstance(r.get('all'), dict)), {})
                        cell = {'item_sha256': item, 'version': version, 'block': block, 'scenario': scenario, **witness,
                                'cpu_p50_us': row.get('all', {}).get('p50_us'), 'cpu_p99_us': row.get('all', {}).get('p99_us'),
                                'idle': row.get('idle'), 'steady': row.get('steady'), 'voices_mean': row.get('voices_mean'), 'voices_peak': row.get('voices_peak'),
                                'peak': row.get('peak'), 'deadline_misses': row.get('deadline_misses'),
                                'underruns': row.get('problems', {}).get('underruns') if isinstance(row.get('problems'), dict) else None,
                                'problems': redact(row.get('problems')),
                                'binary_sha256': hashlib.sha256(exe.read_bytes()).hexdigest()}
                        metric.write_text(json.dumps(cell) + '\n'); result['cells'].append(cell)
            result['reason'] = 'measured-sound-seam; UVI/multi/host coverage remains unknown'
    (run / 'cpu.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def gesture_cell(cells, item, condition):
    """No receipt or incomplete enumeration can certify a gate cell."""
    row = next((c for c in cells if c.get('item_sha256') == item and c.get('condition') == condition), None)
    if row is None:
        return {'status': 'UNKNOWN', 'reason': 'gesture-receipt-absent'}
    if row.get('faults', 0) or row.get('status') == 'FAIL' or row.get('passed', 0) < row.get('total', 0):
        return dict(row, status='FAIL')
    if not row.get('coverage_complete') or not row.get('programs') or row.get('total', 0) == 0 or row.get('status') != 'PASS':
        return dict(row, status='UNKNOWN')
    return row


def presented(run, source):
    """Collect numeric live-GUI receipts; never build or install a plugin."""
    from presented import presented_cell
    manifest = json.loads((run / 'manifest.json').read_text())
    observed = []
    for path in (run / 'presented').glob('**/metrics.json'):
        try:
            row = json.loads(path.read_text())
            if row.get('schema') == 1 and row.get('source_sha') == manifest['sha']:
                observed.append(row)
        except (OSError, ValueError):
            pass
    cells = [dict(presented_cell(observed, hashlib.sha256(line.split('\t', 1)[1].encode()).hexdigest(), condition, manifest['sha']),
                  item_sha256=hashlib.sha256(line.split('\t', 1)[1].encode()).hexdigest(), condition=condition)
             for line in (run / 'items.tsv').read_text().splitlines()
             for condition in manifest.get('conditions', ['cold', 'product-warm', 'os-warm'])]
    result = {'schema': 1, 'status': 'FAIL' if any(c['status'] == 'FAIL' for c in cells) else 'UNKNOWN',
              'cells': cells, 'reason': 'presented-static-region-observations', 'plugin_host_run': bool(observed)}
    (run / 'presented.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def gestures(run, source):
    result = {'schema': 1, 'status': 'UNKNOWN', 'scope': 'Original-editor-input-engine-readback-host-save-reload',
              'cells': [], 'reason': 'owner-test-absent', 'plugin_host_run': False}
    test_source = source / 'src/ui/widget_gate.rs'
    if test_source.is_file():
        records, witness, _ = capture(['cargo', 'test', '--profile', 'ci', '--lib', '--features', 'shots', '--no-run', '--message-format=json'], run / 'gestures/build', cwd=source, timeout=1800)
        executable = next((r.get('executable') for r in records if r.get('reason') == 'compiler-artifact' and r.get('profile', {}).get('test') and r.get('executable')), None)
        if executable and not witness['returncode']:
            binary_hash = hashlib.sha256(Path(executable).read_bytes()).hexdigest()
            result['binary_sha256'] = binary_hash
            manifest = json.loads((run / 'manifest.json').read_text())
            for line in (run / 'items.tsv').read_text().splitlines():
                path = line.split('\t', 1)[1]
                item = hashlib.sha256(path.encode()).hexdigest()
                for condition in manifest.get('conditions', ['cold', 'product-warm', 'os-warm']):
                    folder = run / 'gestures' / condition / item
                    folder.mkdir(parents=True, exist_ok=True)
                    cached = folder / 'metrics.json'
                    if cached.exists():
                        cell = json.loads(cached.read_text())
                        if cell.get('binary_sha256') == binary_hash:
                            result['cells'].append(cell); continue
                    scan = run / 'v2' / condition / 'cache' / (item + '.json')
                    programs = [p.get('program', 0) for p in json.loads(scan.read_text()).get('programs', [])] if scan.exists() else []
                    if not programs and Path(path).suffix.lower() != '.nkm': programs = [0]
                    counts = {'passed': 0, 'total': 0, 'faults': 0}
                    failures, observations, complete = {}, [], bool(programs)
                    with tempfile.TemporaryDirectory(prefix='kontra-gestures-', dir='/dev/shm') as cache:
                        for program in programs:
                            env = {'KONTRA_WIDGET_GATE_PATH': path, 'KONTRA_WIDGET_GATE_PROGRAM': str(program),
                                   'KONTRA_WIDGET_GATE_CONDITION': condition, 'KONTRA_GATE_ITEM_CACHE': cache}
                            records, witness, _ = capture([executable, '--ignored', '--exact', GESTURE, '--nocapture', '--test-threads=1'], folder / str(program), env, source)
                            row = next((r for r in records if r.get('widget_gate_schema') == 1 and r.get('program') == program), None)
                            if row is None:
                                reason = 'probe-timeout' if witness['returncode'] == 124 else 'probe-crash-or-no-receipt'
                                failures[reason] = failures.get(reason, 0) + 1; complete = False
                                observations.append({'program': program, **witness, 'status': 'UNKNOWN', 'reason': reason}); continue
                            for key in counts: counts[key] += row.get(key, 0)
                            for phase in ['initial', 'reload']:
                                failure = row.get(phase + '_load_failure' if phase == 'initial' else 'reload_failure', 'none')
                                if failure != 'none':
                                    reason = phase + '-' + failure
                                    failures[reason] = failures.get(reason, 0) + 1
                            complete &= row.get('coverage_complete', False) and witness['returncode'] == 0
                            targets = row.get('targets', [])
                            if (len(targets) != row.get('total') or sum(t.get('reason') == 'passed' for t in targets) != row.get('passed')
                                    or any(t.get('reason') == 'passed' and not all(t.get(k) is True for k in ['value_changed', 'parameter_reached', 'persistence']) for t in targets)):
                                complete = False; failures['invalid-receipt'] = failures.get('invalid-receipt', 0) + 1
                            for target in targets:
                                reason = target.get('reason', 'invalid-receipt')
                                if reason != 'passed': failures[reason] = failures.get(reason, 0) + 1
                            observations.append({**row, **witness})
                    cell = {'item_sha256': item, 'condition': condition, 'binary_sha256': binary_hash,
                            'programs': programs, **counts, 'coverage_complete': complete, 'failures': failures, 'observations': observations,
                            'status': 'FAIL' if counts['passed'] < counts['total'] or counts['faults'] else 'PASS' if complete and counts['total'] else 'UNKNOWN'}
                    cached.write_text(json.dumps(cell, indent=2) + '\n'); result['cells'].append(cell)
                    print(f"GESTURES {item[:12]} {condition} {cell['passed']}/{cell['total']} {cell['status']}", flush=True)
            statuses = [gesture_cell(result['cells'], c['item_sha256'], c['condition'])['status'] for c in result['cells']]
            result.update(status='FAIL' if 'FAIL' in statuses else 'PASS' if statuses and all(s == 'PASS' for s in statuses) else 'UNKNOWN', reason='observed-per-item-Original-gestures')
        else: result['reason'] = 'owner-test-compile-failed'
    (run / 'gestures.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def host(run, source):
    result = {'status': 'UNKNOWN', 'scope': 'empty-exported-CLAP-process-and-parameter-flush', 'loaded_library_host': 'UNKNOWN', 'cells': [], 'reason': 'exact-source-v2-artifact-or-host-probe-absent'}
    # The release hold prohibits creating a new plugin release. Consume only a supplied existing artifact + provenance receipt.
    receipt_path = os.environ.get('KONTRA_GATE_V2_CLAP_RECEIPT')
    cpp = source / 'tools/cpu-audit-clap.cpp'
    sdk = os.environ.get('KONTRA_GATE_CLAP_INCLUDE')
    if receipt_path and cpp.is_file() and sdk:
        receipt = json.loads(Path(receipt_path).read_text())
        artifact = Path(receipt.get('path', ''))
        manifest = json.loads((run / 'manifest.json').read_text())
        if artifact.is_file() and receipt.get('source_sha') == manifest['sha'] and receipt.get('sha256') == hashlib.sha256(artifact.read_bytes()).hexdigest():
            probe = run / 'bin/clap-cpu-host'
            _, witness, _ = capture(['g++', '-O2', '-std=c++17', '-Wall', '-Wextra', '-Werror', '-I', sdk, cpp, '-ldl', '-o', probe], run / 'host/build')
            if witness['returncode'] == 0:
                for version, binary in [('v2', artifact), ('v1', V1 / 'plugin/KONTRA.clap')]:
                    records, witness, _ = capture([probe, binary], run / 'host' / version, timed=True)
                    for row in records:
                        if all(k in row for k in ['block', 'events', 'flush', 'p50_us', 'p99_us']):
                            result['cells'].append({'version': version, 'contention': witness['contention'], **{k: row[k] for k in ['block', 'events', 'flush', 'p50_us', 'p99_us', 'max_us']}})
                    result[version + '_run'] = witness
                result.update(reason='observed-empty-host-probe', status='PASS' if len(result['cells']) == 36 and all(result[v + '_run']['returncode'] == 0 for v in ['v1', 'v2']) else 'UNKNOWN')
    (run / 'host.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def main():
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('adapter', choices=['cpu', 'gestures', 'host'])
    parser.add_argument('run', type=Path)
    parser.add_argument('source', type=Path)
    args = parser.parse_args()
    source_sha = subprocess.check_output(['git', '-C', str(args.source), 'rev-parse', 'HEAD'], text=True).strip()
    if source_sha != json.loads((args.run / 'manifest.json').read_text())['sha']:
        parser.error('adapter source differs from gate source')
    print(json.dumps(globals()[args.adapter](args.run, args.source)))

if __name__ == '__main__': main()
