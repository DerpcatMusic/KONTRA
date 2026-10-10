#!/usr/bin/env python3
"""Run owned native widget transport; raw state/output/pixels remain in RAM.

Run this whole driver through kontakto-heavy (it must not nest the wrapper).
This observes transport/fresh-instance recall, not the full widget axis.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile
import threading

from evidence import Capture
from live_host import private_settings, v1_state, native_selection, log_rows


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def observe(host, plugin, plugin_sha, plugin_source, item, program, out, plan=None):
    assert digest(plugin.read_bytes()) == plugin_sha, 'frozen plugin changed'
    assert len(plugin_source) == 40 and all(c in '0123456789abcdef' for c in plugin_source)
    out.mkdir(parents=True, exist_ok=False)
    receipt = {'schema': 1, 'status': 'UNKNOWN', 'scope': 'OS-input-transport-and-fresh-plugin-library-recall',
               'host_sha256': digest(host.read_bytes()), 'plugin_sha256': plugin_sha,
               'plugin_source_sha': plugin_source, 'item_sha256': digest(str(item).encode()),
               'program': program, 'timing_claim': False, 'plugin_host_run': True,
               'widget_owner_causality': 'UNKNOWN', 'sound_causality': 'UNKNOWN',
               'frozen_v1_pointer_parity': 'UNKNOWN', 'all_state_and_payloads': 'RAM'}
    with tempfile.TemporaryDirectory(prefix='kontra-widget-host-', dir='/dev/shm') as folder:
        root = Path(folder); private_settings(root / 'config')
        settings = root / 'config/kontra/settings.json'
        values = json.loads(settings.read_text())
        values.update(roots=['/mnt/MAIN_STORAGE/Libraries/Kontakt'], uvi_imported=True, view_mode='Original', ui_scale=1.0, window_size=[1180, 760])
        settings.write_text(json.dumps(values))
        env = dict(os.environ, XDG_CONFIG_HOME=str(root / 'config'), KONTRA_GATE_ITEM_CACHE=str(root / 'cache'))
        for name in ['PROBE_ALLOCS', 'KONTRA_SIGNAL_TRACE', 'KONTRA_AUDIT_STACKS', 'PROBE_EDITOR_SETTLE', 'KONTRA_GATE_ACTIVITY', 'KONTRA_UVI_READER', 'KONTRA_WIDGET_RGB_FD']:
            env.pop(name, None)
        capture = Capture(out, env); job = None
        try:
            template = root / 'template'
            result = subprocess.run([str(host), str(plugin), '--template', str(template)], input=b'', stdout=subprocess.PIPE, stderr=capture.stderr, env=env, timeout=30)
            receipt['bootstrap_returncode'] = result.returncode
            assert result.returncode == 0, 'native bootstrap failed'
            # This exact plugin supplied the envelope; the helper only authors its
            # native Selection, in RAM. It never adds an old file reader to product.
            state = v1_state(template.read_bytes(), str(item), program)
            assert native_selection(state)[0][0]['path'] == str(item)
            native = root / 'state'; native.write_bytes(state)
            if plan is None:
                # Default smoke exercises real OS transport without claiming an
                # unassociated library widget. Causal plans are explicitly supplied.
                commands = b'0 wait 500\n1 checkpoint\n2 move 10 10\n3 key 65507 1\n4 wait 30\n5 key 65507 0\n6 wait 100\n7 save\n8 reload\n9 wait 500\n10 checkpoint\n11 quit\n'
            else:
                assert plan.resolve().is_relative_to(Path('/dev/shm')), 'plans must remain tmpfs'
                commands = plan.read_bytes()
            receipt['plan_sha256'] = digest(commands)
            receipt['plan_commands'] = len(commands.splitlines())
            assert 0 < receipt['plan_commands'] <= 4096 and len(commands) <= 4096 * 257
            # stdout is drained into tmpfs; no raw parameter values are persisted.
            with tempfile.TemporaryFile(dir='/dev/shm') as stdout:
                job = subprocess.Popen([str(host), str(plugin), str(native), '--gestures'], stdin=subprocess.PIPE, stdout=stdout, stderr=capture.stderr, env=env)
                try:
                    job.communicate(commands, timeout=330)
                except subprocess.TimeoutExpired:
                    job.terminate()
                    try: job.wait(timeout=10)
                    except subprocess.TimeoutExpired: job.kill(); job.wait()
                    receipt['timed_out'] = True
                receipt['returncode'] = job.returncode
                stdout.seek(0); raw = stdout.read()
                receipt['stdout_sha256'] = digest(raw)
                rows = []
                for line in raw.splitlines():
                    try: row = json.loads(line)
                    except (UnicodeError, ValueError): continue
                    if isinstance(row, dict) and row.get('schema') == 1: rows.append(row)
                points = [row for row in rows if row.get('kind') == 'checkpoint']
                snapshots = []
                for row in points:
                    params = dict(row['parameters'])
                    assert len(params) == len(row['parameters'])
                    assert all(type(k) is int and type(v) in (float, int) and math.isfinite(v) for k, v in params.items())
                    snapshots.append(params)
                observations = []
                previous = None
                for row, params in zip(points, snapshots):
                    changed = sorted(k for k, v in params.items() if previous is not None and previous.get(k) != v)
                    observations.append({k: row[k] for k in ['sequence', 'generation', 'width', 'height', 'load_ready', 'blocks', 'samples', 'rms', 'peak', 'nonfinite', 'parameter_events']})
                    observations[-1].update(parameter_count=len(params), parameter_snapshot_sha256=digest(json.dumps(sorted(params.items()), separators=(',', ':')).encode()), changed_parameter_ids=changed)
                    previous = params
                receipt['checkpoints'] = observations
                receipt['fresh_instance_observed'] = any(row.get('kind') == 'reloaded' and row.get('generation', 0) > 1 for row in rows)
                receipt['parameter_recall_equal'] = len(snapshots) >= 2 and snapshots[-1] == snapshots[-2]
                receipt['readiness_confirmed'] = bool(points) and all(row.get('load_ready') is True for row in points)
                receipt['nonfinite_samples'] = sum(row.get('nonfinite', 0) for row in points)
                # Raw logs stay in Capture.root until sanitized metadata is written.
                loaded = [row for row in log_rows(capture.root) if row.get('event') == 'load_finished' and row.get('path') == str(item) and row.get('program') == program]
                receipt['selected_library_load_events'] = len(loaded)
                receipt['selected_library_ready_events'] = sum(row.get('data', {}).get('status') in ['loaded', 'partial'] for row in loaded)
                if job.returncode != 0:
                    receipt['status'] = 'FAIL'; receipt['reason'] = 'host-process-or-plan-failed'
                elif not receipt['fresh_instance_observed'] or not receipt['parameter_recall_equal'] or receipt['nonfinite_samples']:
                    receipt['status'] = 'FAIL'; receipt['reason'] = 'fresh-plugin-parameter-recall-or-audio-failed'
                elif not receipt['readiness_confirmed'] or receipt['selected_library_ready_events'] < 2:
                    receipt['reason'] = 'library-source-or-readiness-not-certified'
                else:
                    receipt['status'] = 'PASS'; receipt['reason'] = 'transport-fresh-library-recall-only'
        finally:
            if job is not None and job.poll() is None:
                job.terminate(); job.wait(timeout=10)
            capture.stderr.flush(); capture.stderr.seek(0)
            receipt['stderr_sha256'] = digest(capture.stderr.read())
            capture.finish()
    (out / 'metrics.json').write_text(json.dumps(receipt, indent=2) + '\n')
    return receipt


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--plugin', type=Path, required=True)
    parser.add_argument('--plugin-sha', required=True); parser.add_argument('--plugin-source', required=True)
    parser.add_argument('--item', type=Path, required=True); parser.add_argument('--program', type=int, default=0)
    parser.add_argument('--out', type=Path, required=True); parser.add_argument('--plan', type=Path)
    args = parser.parse_args()
    result = observe(args.host, args.plugin, args.plugin_sha, args.plugin_source, args.item, args.program, args.out, args.plan)
    print(json.dumps({k: result.get(k) for k in ['status', 'reason', 'returncode', 'fresh_instance_observed', 'parameter_recall_equal', 'selected_library_load_events', 'selected_library_ready_events']}))
