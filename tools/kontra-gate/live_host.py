#!/usr/bin/env python3
"""Loaded real-time CLAP evidence; frozen v1 is run, never rebuilt.

Run through kontakto-heavy after requesting a quiet window. Plugin logs and
native states stay in tmpfs; only numeric receipts and hashes are retained.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import time

from contention import Activity
from evidence import Capture

V1 = Path.home() / '.cache/kontra-v1'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def artifact_receipt(plugin, cli, host):
    receipt = json.loads(plugin.with_name('BUILD.json').read_text())
    revision = receipt['source_sha']
    assert len(revision) == 40 and all(c in '0123456789abcdef' for c in revision), 'full source revision required'
    assert receipt.get('profile') == 'ci', 'release/install forbidden'
    assert Path(receipt['path']).resolve() == plugin.resolve()
    assert receipt['sha256'] == sha(plugin) and receipt['cli_sha256'] == sha(cli)
    assert receipt['host_sha256'] == sha(host), 'artifact changed after build receipt'
    return receipt


def events(program, seconds):
    """Gate CC/key/velocity/keyswitch choices, repeated at exact sample positions."""
    key, velocity = program['key'], program['velocity']
    switch = program.get('keyswitch')
    assert isinstance(key, int) and 0 <= key < 128
    assert isinstance(velocity, int) and 1 <= velocity < 128
    assert switch is None or isinstance(switch, int) and 0 <= switch < 128
    result = [(0, 0xb0, 1, 100), (0, 0xb0, 11, 127)]
    at = 0
    if switch is not None:
        result += [(0, 0x90, switch, 64), (128, 0x80, switch, 0)]
        at = 128
    for start in range(at, int((seconds - 1) * 48000), 48000):
        result += [(start, 0x90, key, velocity), (start + 24000, 0x80, key, 0)]
    return sorted(result, key=lambda e: e[0])


def keyed(fields):
    """Probe-only native moose State encoder, pinned to v1's State derive.

    This never adds a v1 file reader to v2. Unknown/missing keyed fields follow
    the frozen plugin's defaults, as its native session restore specifies.
    """
    result = struct.pack('<II', 0xffffff01, len(fields))
    for name, payload in fields.items():
        value = 0x811c9dc5
        for byte in name.encode():
            value = ((value ^ byte) * 0x01000193) & 0xffffffff
        result += struct.pack('<II', value, len(payload)) + payload
    return result


def sized(data):
    return struct.pack('<I', len(data)) + data


def v1_state(template, path, program):
    """Keep the frozen CLI's plugin identity/parameter envelope; author v1 state only."""
    assert template[:8] == b'OAST\x01\0\0\0'
    count = struct.unpack_from('<I', template, 16)[0]
    at = 20 + 12 * count
    extra = struct.unpack_from('<Q', template, at)[0]
    at += 8 + extra
    length = struct.unpack_from('<Q', template, at)[0]
    assert at + 8 + length == len(template)
    # Native keyed frames exist in frozen 0.3.152 (d097c363) and 0cb7a8a0.
    part = keyed({'path': sized(path.encode()), 'program': struct.pack('<I', program),
                  'port': b'\0', 'channel': struct.pack('<h', -1), 'output': b'\0',
                  'output_manual': b'\1', 'gain': struct.pack('<f', 0), 'aux': struct.pack('<h', -1)})
    selection = keyed({'parts': struct.pack('<I', 1) + sized(part), 'order': struct.pack('<II', 1, 0)})
    persist = struct.pack('<I', 1) + sized(b'selection') + sized(sized(selection))
    return template[:at] + struct.pack('<Q', len(persist)) + persist


def private_settings(config):
    # Prevent first-run Kontakt/Wine auto-import in both native host versions.
    settings = config / 'kontra/settings.json'
    settings.parent.mkdir(parents=True, exist_ok=True)
    settings.write_text(json.dumps({'imported': True, 'roots': []}))


def log_rows(root):
    rows = []
    for file in root.rglob('*.jsonl'):
        for line in file.read_text(errors='replace').splitlines():
            try:
                row = json.loads(line)
                if isinstance(row, dict): rows.append(row)
            except ValueError:
                pass  # A writer may still be appending the final line.
    return rows


def load_status(rows):
    finished = [r.get('data', {}).get('status') for r in rows if r.get('event') == 'load_finished']
    if any(state in ['loaded', 'partial'] for state in finished): return 'READY'
    return 'FAILED' if 'failed' in finished else 'WAITING'


def frozen_underruns(rows):
    audio = any(r.get('event') == 'host_audio_config' or r.get('code') == 'host_audio_config' for r in rows)
    total = 0
    for row in rows:
        if row.get('event', row.get('code')) != 'playback_drops': continue
        data = row.get('data', row.get('details', {}))
        total = max(total, data.get('state', {}).get('underruns', 0))
    return total if audio else None


def measured_status(live):
    complete = (live.get('returncode') == 0 and live.get('events_dispatched', 0) > 0
                and live.get('events_dispatched') == live.get('events_planned')
                and live.get('peak', 0) > 0 and live.get('nonfinite') == 0)
    return 'MEASURED' if complete and live.get('contention') == 'QUIET' else 'UNKNOWN'


def observe(host, plugin, state, plan, block, seconds, folder, version):
    folder.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='kontra-live-', dir='/dev/shm') as temp:
        temp = Path(temp)
        ready = temp / 'ready'
        schedule = temp / 'events.tsv'
        schedule.write_text(''.join('\t'.join(map(str, e)) + '\n' for e in events(plan, seconds)))
        native = temp / 'session.state'; native.write_bytes(state)
        private_settings(temp / 'config')
        env = dict(os.environ, XDG_CONFIG_HOME=str(temp / 'config'))
        capture = Capture(folder, env)
        activity = Activity(folder)
        job = None
        live = {}
        activity.start()
        try:
            with tempfile.TemporaryFile(dir='/dev/shm') as output:
                job = subprocess.Popen([str(host), str(plugin), str(native), str(block), str(seconds), str(ready), str(schedule), '1'],
                                       env=env, stdout=output, stderr=capture.stderr)
                started = time.monotonic()
                while job.poll() is None:
                    if time.monotonic() - started > 145:
                        job.kill(); job.wait(); break
                    if not ready.exists():
                        loaded = load_status(log_rows(capture.root))
                        if loaded == 'READY': ready.touch()
                        elif loaded == 'FAILED':
                            live['host_failure'] = 'plugin load reported failed'
                            job.kill(); job.wait(); break
                    time.sleep(.05)
                output.seek(0)
                raw = output.read()
                records = []
                for line in raw.splitlines():
                    try: records.append(json.loads(line))
                    except ValueError: pass
                live.update(next((r for r in records if r.get('kind') == 'live_host'), {}))
                views = [r for r in records if r.get('kind') == 'perf_view']
                io = next((r for r in records if r.get('kind') == 'stream_io'), {})
                rows = log_rows(capture.root)
                capture.stderr.flush(); capture.stderr.seek(0)
                errors = capture.stderr.read().decode(errors='replace')
                for stage in ['native CLAP state load', 'matched zero-dB master', 'bounded load/readiness wait', 'dlopen plugin']:
                    if 'FAIL: ' + stage in errors: live['host_failure'] = stage
                live.update(version=version, returncode=job.returncode, plugin_sha256=sha(plugin),
                            host_sha256=sha(host), state_sha256=hashlib.sha256(state).hexdigest(),
                            audition_sha256=sha(schedule), stdout_sha256=hashlib.sha256(raw).hexdigest(),
                            perf_view=views, streaming_io=io, underruns=views[-1]['underruns'] if views else frozen_underruns(rows))
        finally:
            if job and job.poll() is None: job.kill(); job.wait()
            activity.finish()
            capture.finish()
        diagnostics = folder / 'plugin-diagnostics.json'
        captured = json.loads(diagnostics.read_text()); captured['plugin_host_run'] = True
        diagnostics.write_text(json.dumps(captured) + '\n')
        live['contention'] = json.loads((folder / 'activity.json').read_text())['status']
        live['status'] = measured_status(live)
        (folder / 'metrics.json').write_text(json.dumps(live, indent=2) + '\n')
        return live


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('gate', type=Path)
    parser.add_argument('v2_plugin', type=Path)
    parser.add_argument('v2_cli', type=Path)
    parser.add_argument('host', type=Path)
    parser.add_argument('out', type=Path)
    parser.add_argument('--block', type=int, choices=[32, 64, 256], default=64)
    parser.add_argument('--seconds', type=float, default=6)
    parser.add_argument('--version', choices=['v1', 'v2', 'both'], default='both')
    parser.add_argument('--start', type=int, default=0)
    parser.add_argument('--count', type=int, default=2)
    args = parser.parse_args()
    driver_sha256 = sha(__file__)
    assert 2 <= args.seconds <= 30 and args.start >= 0 and 1 <= args.count <= 128
    assert os.environ.get('KONTRA_QUIET_OWNER') == '1', 'request a quiet window first'
    assert (Path.home() / '.cache/kontra-quiet-request').exists(), 'quiet request absent'
    assert (Path.home() / '.cache/kontra-quiet-granted').exists(), 'quiet grant absent'
    os.environ['KONTRA_GATE_REQUIRE_QUIET'] = '1'
    subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], cwd=V1, check=True, stdout=subprocess.DEVNULL)
    build = artifact_receipt(args.v2_plugin, args.v2_cli, args.host)
    args.out.mkdir(parents=True, exist_ok=True)
    items = [line.split('\t', 1)[1] for line in (args.gate / 'items.tsv').read_text().splitlines()]
    cells = []
    with tempfile.TemporaryDirectory(prefix='kontra-live-state-', dir='/dev/shm') as tmp:
        tmp = Path(tmp)
        # Bootstrap only the native envelope with the existing frozen export command.
        private_settings(tmp / 'config')
        first = next(path for path in items if path.lower().endswith('.nki'))
        state_env = dict(os.environ, XDG_CONFIG_HOME=str(tmp / 'config'), XDG_DATA_HOME=str(tmp / 'data'),
                         XDG_CACHE_HOME='/proc/self/kontra-gate-no-cache', KONTRA_DISABLE_NETWORK='1', KONTRA_LOG_DIR=str(tmp / 'logs'))
        common = {'port': 0, 'channel': -1, 'output': 0, 'aux': -1, 'aux_gain': -60.,
                  'output_manual': True, 'mic_buses': [], 'mic_names': []}
        multi = tmp / 'bootstrap.kontra-multi'
        multi.write_text(json.dumps({'format': 'kontra-multi', 'version': 1, 'name': 'Live host probe', 'parts': [dict(common, path=first)]}))
        template = tmp / 'bootstrap.state'
        subprocess.run([str(V1 / 'bin/kontakto-v1'), 'export-multi-state', str(multi), str(template)],
                       env=state_env,
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for path in items[args.start:args.start + args.count]:
            identity = hashlib.sha256(path.encode()).hexdigest()
            notes = args.gate / 'notes' / (identity + '.json')
            if not notes.exists():
                cells.append({'item_sha256': identity, 'status': 'UNKNOWN', 'reason': 'gate-audition-plan-absent'}); continue
            for program, plan in json.loads(notes.read_text())['programs'].items():
                if not isinstance(plan.get('key'), int):
                    cells.append({'item_sha256': identity, 'program': program, 'status': 'UNKNOWN', 'reason': 'gate-audition-key-absent'}); continue
                native = tmp / (identity + '-' + program + '.state')
                mapping = tmp / (identity + '-' + program + '.kontra-multi')
                mapping.write_text(json.dumps({'format': 'kontra-multi', 'version': 2, 'name': 'Live host probe',
                                              'parts': [dict(common, path=path, program=int(program))]}))
                subprocess.run([str(args.v2_cli), 'export-multi-state', str(mapping), str(native)],
                               env=state_env, check=True)
                for version, plugin, blob in [('v1', V1 / 'plugin/KONTRA.clap', v1_state(template.read_bytes(), path, int(program))),
                                              ('v2', args.v2_plugin, native.read_bytes())]:
                    if args.version != 'both' and version != args.version: continue
                    cell = observe(args.host, plugin, blob, plan, args.block, args.seconds,
                                   args.out / f'{identity}-{program}-{args.block}-{version}', version)
                    cell.update(item_sha256=identity, program=int(program))
                    (args.out / f'{identity}-{program}-{args.block}-{version}' / 'metrics.json').write_text(json.dumps(cell, indent=2) + '\n')
                    cells.append(cell)
                    print(json.dumps({k: v for k, v in cell.items() if k != 'perf_view'}), flush=True)
    receipt = {'scope': 'loaded-exported-CLAP-realtime-editor-closed', 'cells': cells,
               'v2_source_sha': build['source_sha'], 'v2_artifact': build,
               'gate_sha': json.loads((args.gate / 'manifest.json').read_text())['sha'],
               'host_source_sha256': sha(Path(__file__).parents[2] / 'vendor/moose-clap/tests/live_performance.cpp'),
               'driver_sha256': driver_sha256, 'frozen_v1_perf_view': 'UNKNOWN: frozen binary has no numeric readback export',
               'streaming_scope': 'whole plugin process /proc/self/io delta during audition, logical rchar minus first probe read and physical read_bytes; load wait excluded; OS page cache uncontrolled; sampler stream-underruns and host process/wake deadlines reported separately',
               'ui_cpu_policy': 'same cumulative busy/span counters and 100ms half smoothing as Watch; headless numeric model, not a rendered DAW frame'}
    (args.out / f'host-{args.start}-{args.count}-{args.block}-{args.version}.json').write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__': main()
