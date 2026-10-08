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
HEAVY = Path.home() / '.cache/kontakto-heavy'
V1 = Path.home() / '.cache/kontra-v1'
GESTURE = 'ui::v2_tests::widget_conflux_native_gestures_and_readback'


def capture(args, folder, env=None, cwd=None, timeout=235):
    folder.mkdir(parents=True, exist_ok=True)
    child_env = dict(os.environ, **(env or {}))
    evidence = Capture(folder, child_env)
    # Runtime output stays in RAM until reduced to numeric JSON and test witnesses.
    with tempfile.TemporaryFile(dir='/dev/shm') as output:
        try:
            job = subprocess.run([str(HEAVY), 'timeout', str(timeout), *map(str, args)], stdout=output, stderr=evidence.stderr, env=child_env, cwd=cwd)
            output.seek(0); raw = output.read().decode(errors='replace')
            records = []
            for line in raw.splitlines():
                try:
                    row = json.loads(line)
                    if isinstance(row, dict): records.append(row)
                except ValueError: pass
            witness = {'stdout_sha256': hashlib.sha256(raw.encode()).hexdigest(), 'returncode': job.returncode, 'lines': len(raw.splitlines())}
            return records, witness, raw
        finally:
            evidence.finish()


def cpu(run, source):
    frozen = V1 / 'bin/cpu-audit-v1'
    example = source / 'examples/cpu_audit.rs'
    result = {'status': 'UNKNOWN', 'cells': [], 'reason': 'frozen-v1-adapter-or-v2-example-absent'}
    if frozen.is_file() and example.is_file():
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
                            result['cells'].append(json.loads(metric.read_text())); continue
                        records, witness, _ = capture([exe, path, block, scenario], out)
                        row = next((r for r in records if r.get('block') == block and isinstance(r.get('all'), dict)), {})
                        cell = {'item_sha256': item, 'version': version, 'block': block, 'scenario': scenario, **witness,
                                'cpu_p50_us': row.get('all', {}).get('p50_us'), 'cpu_p99_us': row.get('all', {}).get('p99_us'),
                                'idle': row.get('idle'), 'steady': row.get('steady'), 'voices_mean': row.get('voices_mean'), 'voices_peak': row.get('voices_peak'),
                                'peak': row.get('peak'), 'deadline_misses': row.get('deadline_misses'), 'problems': redact(row.get('problems')),
                                'binary_sha256': hashlib.sha256(exe.read_bytes()).hexdigest()}
                        metric.write_text(json.dumps(cell) + '\n'); result['cells'].append(cell)
            result['reason'] = 'measured-sound-seam; UVI/multi/host coverage remains unknown'
    (run / 'cpu.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def gestures(run, source):
    result = {'status': 'UNKNOWN', 'scope': 'Conflux-native-input-readback', 'full_gate_set': 'UNKNOWN', 'reason': 'owner-test-absent'}
    test_source = source / 'src/ui/v2_tests.rs'
    if test_source.is_file() and 'fn widget_conflux_native_gestures_and_readback' in test_source.read_text():
        records, witness, _ = capture(['cargo', 'test', '--profile', 'ci', '--lib', '--features', 'shots', '--no-run', '--message-format=json'], run / 'gestures/build', cwd=source)
        executable = next((r.get('executable') for r in records if r.get('reason') == 'compiler-artifact' and r.get('profile', {}).get('test') and r.get('executable')), None)
        if executable and not witness['returncode']:
            path = next((r.split('\t', 1)[1] for r in (run / 'items.tsv').read_text().splitlines() if r.endswith('/Instruments/Conflux.nki')), None)
            if path and Path(path).exists():
                records, witness, raw = capture([executable, '--ignored', '--exact', GESTURE, '--nocapture'], run / 'gestures/sweep', {'KONTRA_AUDIT_WIDGET_PATCH': path}, source)
                passed = len(re.findall(r'^CONFLUX_GESTURE_PASS ', raw, re.M))
                tests = re.search(r'test result: ok\. (\d+) passed;', raw)
                result.update(**witness, gesture_witnesses=passed, test=GESTURE,
                              status='PASS' if tests and int(tests[1]) == 1 and passed > 0 and witness['returncode'] == 0 else 'FAIL', reason='observed-owner-test')
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
                    records, witness, _ = capture([probe, binary], run / 'host' / version)
                    for row in records:
                        if all(k in row for k in ['block', 'events', 'flush', 'p50_us', 'p99_us']):
                            result['cells'].append({'version': version, **{k: row[k] for k in ['block', 'events', 'flush', 'p50_us', 'p99_us', 'max_us']}})
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
