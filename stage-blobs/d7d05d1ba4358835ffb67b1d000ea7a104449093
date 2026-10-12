#!/usr/bin/env python3
"""One matched CPU-audit cell through CLAP; never rebuild frozen v1.

Build vendor/moose-clap/tests/live_performance.cpp as clap-cpu-host first.
Run this through kontakto-heavy in your own granted quiet window.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent / 'kontra-gate'))
from live_host import V1, artifact_receipt, observe, private_settings, sha, v1_state

SCENARIOS = {
    'piano': ('Una Corda Library', 'Instruments/Una Corda Pure.nki'),
    'strings': ('Performance Samples Vista', 'Instruments/Vista - 5 Violins.nki'),
    'fx': ('ANALOG STRINGS', 'Instruments/ANALOG STRINGS.nki'),
}


def audit_multi(path, version):
    return {'format': 'kontra-multi', 'version': 1 if version == 'v1' else 2, 'name': 'CPU audit',
            'parts': [{'path': str(path), 'program': 0, 'port': 0, 'channel': -1, 'output': 0,
                       'output_manual': True, 'gain': 0., 'aux': -1, 'aux_gain': -60.,
                       'mic_buses': [], 'mic_names': []}]}


def audit_events(scenario):
    assert scenario in SCENARIOS
    keys = [48, 52, 55, 60, 64, 67, 72, 76] if scenario == 'piano' else list(range(48, 60))
    result = [(0, 0xb0, 1, 110), (0, 0xb0, 11, 127), (0, 0xb0, 64, 127)]
    for key in keys:
        result += [(0, 0x90, key, 100), (48000, 0x80, key, 0)]
    result.append((144000, 0xb0, 64, 0))
    return sorted(result, key=lambda event: event[0])


def cold_receipt(path):
    row = json.loads(path.read_text())
    assert isinstance(row, dict), 'cold-source receipt required'
    assert all(type(row.get(key)) is int and row[key] >= 0
               for key in ('files', 'pages_total', 'pages_before', 'pages_after')), 'invalid cold-source counters'
    assert row['files'] > 0 and row['pages_total'] > 0, 'empty cold-source observation'
    assert row['pages_before'] <= row['pages_total'] and row['pages_after'] == 0, 'source pages still resident; cold cell not admitted'
    return row


def check():
    for scenario in SCENARIOS:
        events = audit_events(scenario)
        keys = [e[2] for e in events if e[1] == 0x90]
        assert keys == ([48, 52, 55, 60, 64, 67, 72, 76] if scenario == 'piano' else list(range(48, 60)))
        assert len(events) == 2 * len(keys) + 4
        assert events[:3] == [(0, 0xb0, 1, 110), (0, 0xb0, 11, 127), (0, 0xb0, 64, 127)]
        assert events[-1] == (144000, 0xb0, 64, 0)
        for block in (32, 64, 256):
            dispatched = [e for begin in range(0, 192000, block) for e in events if begin <= e[0] < begin + block]
            assert dispatched == events
            assert 48000 // block * block == (47872 if block == 256 else 48000)
    print('PASS: original audit notes, CCs, pedals, order and block-start dispatch')


def main():
    if sys.argv[1:] == ['--check']:
        check(); return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('scenario', choices=SCENARIOS)
    parser.add_argument('block', type=int, choices=[32, 64, 256])
    parser.add_argument('out', type=Path)
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--version', choices=['v1', 'v2'], default='v1')
    parser.add_argument('--plugin', type=Path)
    parser.add_argument('--cli', type=Path)
    parser.add_argument('--source-sha', help='required v2 full selected candidate SHA; must match adjacent BUILD.json')
    parser.add_argument('--cold', action='store_true')
    parser.add_argument('--profile', action='store_true', help='audio-TID-only leaf-IP perf recording; score unprofiled repeats')
    parser.add_argument('--quiet-owner', required=True, help='exact owner in request and grant')
    args = parser.parse_args()
    assert os.environ.get('KONTRA_QUIET_OWNER') == '1', 'quiet-owner wrapper required'
    for name in ('request', 'granted'):
        flag = Path.home() / ('.cache/kontra-quiet-' + name)
        record = json.loads(flag.read_text())
        assert record['owner'] == args.quiet_owner, 'another writer owns the quiet window'
        assert 0 <= time.time() - flag.stat().st_mtime < 45 * 60, 'quiet window expired'
    os.environ['KONTRA_GATE_REQUIRE_QUIET'] = '1'
    subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], cwd=V1, check=True, stdout=subprocess.DEVNULL)
    plugin = V1 / 'plugin/KONTRA.clap' if args.version == 'v1' else args.plugin
    cli = V1 / 'bin/kontakto-v1' if args.version == 'v1' else args.cli
    assert plugin and cli, 'v2 requires its matching --plugin and --cli'
    assert args.version != 'v1' or not (args.plugin or args.cli or args.source_sha), 'v1 always uses the verified frozen artifacts'
    build = None
    if args.version == 'v2':
        assert args.source_sha, 'v2 requires --source-sha for the selected candidate'
        build = artifact_receipt(plugin, cli, args.host)
        assert build['profile'] == 'release', 'CPU comparison requires release; ci changes ThinLTO policy'
        assert build['source_sha'] == args.source_sha, 'artifact belongs to a different candidate'
        host_source = Path(__file__).resolve().parents[1] / 'vendor/moose-clap/tests/live_performance.cpp'
        assert build.get('host_source_sha256') == sha(host_source), 'built host source receipt missing or mismatched'
    library, instrument = SCENARIOS[args.scenario]
    library = Path('/mnt/MAIN_STORAGE/Libraries/Kontakt') / library
    path = library / instrument
    assert path.is_file(), 'original audit instrument absent'
    assert not (args.out / 'metrics.json').exists(), 'use a fresh output directory for each repeat'
    args.out.mkdir(parents=True, exist_ok=True)
    cache = None
    with tempfile.TemporaryDirectory(prefix='kontra-cpu-state-', dir='/dev/shm') as tmp:
        tmp = Path(tmp)
        private_settings(tmp / 'config')
        env = dict(os.environ, XDG_CONFIG_HOME=str(tmp / 'config'), XDG_DATA_HOME=str(tmp / 'data'),
                   XDG_CACHE_HOME='/dev/null', KONTRA_DISABLE_NETWORK='1', KONTRA_LOG_DIR=str(tmp / 'logs'),
                   KONTRA_REPORT_DIR=str(tmp / 'reports'))
        multi = tmp / 'input.kontra-multi'
        multi.write_text(json.dumps(audit_multi(path, args.version)))
        native = tmp / 'input.state'
        subprocess.run([str(cli), 'export-multi-state', str(multi), str(native)], env=env,
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        state = native.read_bytes()
        if args.version == 'v1': state = v1_state(state, str(path), 0)
        if args.cold:
            with (args.out / 'cache.json').open('w') as cache_file:
                subprocess.run([sys.executable, str(Path(__file__).with_name('cpu-audit-cold.py')), str(library)],
                               stdout=cache_file, check=True)
            cache = cold_receipt(args.out / 'cache.json')
        live = observe(args.host.resolve(), plugin.resolve(), state, audit_events(args.scenario),
                       args.block, 4, args.out, args.version, cpu_audit=True, profile=args.profile)
    live.update(scope='loaded-native-clap-callback-plus-main-output-peak-scan', scenario=args.scenario,
                cache_condition='cold-source-files' if args.cold else 'unforced-source-cache', profiled=args.profile,
                cli_sha256=sha(cli), host_source_sha256=build.get('host_source_sha256') if build else None,
                source_sha=build['source_sha'] if build else None, artifact_receipt=build, cache=cache,
                driver_sha256=sha(__file__), render_heap_calls=None, event_heap_calls=None,
                voices_scope='plugin diagnostics; no per-voice normalization')
    (args.out / 'metrics.json').write_text(json.dumps(live, indent=2) + '\n')
    print(json.dumps(live))
    return 0 if live['status'] == 'MEASURED' else 1


if __name__ == '__main__': sys.exit(main())
