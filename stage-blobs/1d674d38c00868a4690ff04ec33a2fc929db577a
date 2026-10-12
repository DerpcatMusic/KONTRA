#!/usr/bin/env python3
"""Six editor-closed load cells on frozen 0.3.326 and frozen v1; quiet owner only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import subprocess
import tempfile

from live_host import observe, private_settings, v1_state, sha

SOURCE = '608a20a1687a501e161d80af72280639c122fbc3'
V1 = Path.home() / '.cache/kontra-v1'
PRESETS = [('conflux', '/Conflux.nki'), ('pacific', '10 Cellos - Legato Sustains.nki'), ('analog', '/ANALOG STRINGS.nki')]


def receipts(artifacts, cli):
    rows = json.loads(artifacts.read_text())
    native = json.loads(cli.read_text())
    assert {r['format'] for r in rows} == {'clap', 'vst3'}
    for row in rows + [native]:
        build = row['build']
        assert build['version'] == '0.3.326' and build['source_revision'] == SOURCE
        assert build['profile'] == 'release' and build['dirty'] is False
        assert len(row['sha256']) == 64 and all(c in '0123456789abcdef' for c in row['sha256'])
        assert Path(row['artifact']).is_file()
    return rows, native


def selected_paths():
    rows = (Path.home() / '.cache/kontakto-fix-load/scan-items.tsv').read_text().splitlines()
    selected = []
    for name, needle in PRESETS:
        paths = [r.split('\t')[1] for r in rows if needle in r]
        assert len(paths) == 1
        path = paths[0]
        assert path.startswith('/mnt/MAIN_STORAGE/Libraries/Kontakt/') and Path(path).suffix.lower() == '.nki'
        assert Path(path).is_file()
        selected.append((name, path))
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifacts', type=Path)
    parser.add_argument('cli', type=Path)
    parser.add_argument('host', type=Path)
    parser.add_argument('out', type=Path)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--resume', action='store_true')
    args = parser.parse_args()
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    rows, native = receipts(args.artifacts, args.cli)
    selected = selected_paths()
    assert args.host.is_file()
    if not args.run:
        print('PREPARED: receipt schema, native Kontakt-only paths and host exist; hashes/timing deferred to quiet owner')
        return
    request = Path.home() / '.cache/kontra-quiet-request'
    grant = Path.home() / '.cache/kontra-quiet-granted'
    assert os.environ.get('KONTRA_QUIET_OWNER') == '1' and os.environ.get('KONTRA_GATE_REQUIRE_QUIET') == '1'
    assert request.is_file() and grant.is_file()
    assert json.loads(request.read_text())['owner'] == 'W8 release 0.3.326 load cells'
    subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], cwd=V1, stdout=subprocess.DEVNULL, check=True)
    for row in rows + [native]:
        assert sha(row['artifact']) == row['sha256'], 'artifact changed after frozen receipt'
    plugin = Path(next(r for r in rows if r['format'] == 'clap')['artifact'])
    cli = Path(native['artifact'])
    args.out.mkdir(parents=True, exist_ok=args.resume)
    record = dict(source_sha=SOURCE, artifacts=rows, cli=native, host_sha256=sha(args.host),
                  driver_sha256=sha(__file__), scope='exported CLAP, editor closed',
                  product_cache='disabled', os_page_cache='uncontrolled', block=64, rate=48000, seconds=2,
                  readiness='same load_finished diagnostic flag, Python 5ms / host 1ms polling',
                  first_audio='host wall time from state-load start plus fixed 100ms post-ready warmup; sample frame from audition start, threshold1e-7',
                  memory='process /proc/self/status RSS/HWM/swap, sampled before state load, at readiness and after audition; state save excluded',
                  editor_open_rss='UNKNOWN, separate W13 evidence', cells=[])
    if args.resume:
        prior = json.loads((args.out / 'load-cells.json').read_text())
        for key in ('source_sha', 'artifacts', 'cli', 'host_sha256', 'scope', 'block', 'rate', 'seconds'):
            assert prior[key] == record[key], 'resume provenance changed'
        record['cells'] = prior['cells']
        record['prior_driver_sha256'] = prior['driver_sha256']
        assert len({(c['preset'], c['version']) for c in record['cells']}) == len(record['cells'])
    (args.out / 'provenance.json').write_text(json.dumps(record, indent=2) + '\n')
    with tempfile.TemporaryDirectory(prefix='kontra-load-state-', dir='/dev/shm') as tmp:
        tmp = Path(tmp)
        private_settings(tmp / 'config')
        env = dict(os.environ, XDG_CONFIG_HOME=str(tmp / 'config'), XDG_CACHE_HOME='/proc/self/kontra-load-no-cache',
                   XDG_DATA_HOME=str(tmp / 'data'), KONTRA_LOG_DIR=str(tmp / 'logs'), KONTRA_REPORT_DIR=str(tmp / 'reports'),
                   KONTRA_DISABLE_NETWORK='1')
        for key in ['KONTRA_UVI_READER', 'KONTRA_UVI_AUDIT_SEED', 'KONTRA_SIGNAL_TRACE', 'PROBE_ALLOCS']:
            env.pop(key, None)
        common = dict(port=0, channel=-1, output=0, aux=-1, aux_gain=-60., output_manual=True, mic_buses=[], mic_names=[])
        mapping = tmp / 'bootstrap.kontra-multi'
        mapping.write_text(json.dumps(dict(format='kontra-multi', version=1, name='Load probe', parts=[dict(common, path=selected[0][1])])) )
        template = tmp / 'bootstrap.state'
        subprocess.run([str(V1 / 'bin/kontakto-v1'), 'export-multi-state', str(mapping), str(template)],
                       env=env, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        template = template.read_bytes()
        for name, path in selected:
            mapping = tmp / f'{name}.kontra-multi'; state = tmp / f'{name}.state'
            mapping.write_text(json.dumps(dict(format='kontra-multi', version=2, name='Load probe', parts=[dict(common, path=path, program=0)])))
            subprocess.run([str(cli), 'export-multi-state', str(mapping), str(state)], env=env,
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            v2 = state.read_bytes(); v1 = v1_state(template, path, 0)
            versions = ['v2', 'v1'] if name == 'pacific' else ['v1', 'v2']
            for version in versions:
                if any(c['preset'] == name and c['version'] == version for c in record['cells']):
                    continue
                cell = observe(args.host, plugin if version == 'v2' else V1 / 'plugin/KONTRA.clap',
                               v2 if version == 'v2' else v1, dict(key=60, velocity=100, cc1=127),
                               64, 2, args.out / f'{name}-{version}', version, load_probe=True)
                cell.update(preset=name, item_id=hashlib.sha256(path.encode()).hexdigest(), program=0)
                record['cells'].append(cell)
                (args.out / 'load-cells.json').write_text(json.dumps(record, indent=2) + '\n')
                print(json.dumps({k: v for k, v in cell.items() if k not in ['perf_view', 'streaming_io']}), flush=True)
    print('DONE: six load cells; quiet ownership remains with the W8 launcher until cleanup/handoff')


if __name__ == '__main__':
    main()
