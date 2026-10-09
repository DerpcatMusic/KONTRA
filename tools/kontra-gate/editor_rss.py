"""Raw live-X11 CLAP RSS; run through kontakto-heavy, without timing claims.

Native states and plugin output remain in tmpfs. Close hides then destroys the
editor; the same plugin and paced 48 kHz/64-frame audio engine survive all steps.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import struct
import subprocess
import tempfile
import time

from evidence import Capture
from live_host import V1, sha, private_settings, v1_state, native_selection, log_rows, keyed, sized

PHASES = ('loaded', 'open', 'closed', 'reopened')


def summarize(rows):
    assert len(rows) == 40, 'four complete RSS phases required'
    result = {}
    for index, phase in enumerate(PHASES):
        samples = rows[index*10:(index+1)*10]
        assert [r['phase'] for r in samples] == [phase]*10
        assert [r['sample'] for r in samples] == list(range(10))
        shown = phase in ('open', 'reopened')
        for row in samples:
            assert row['editor_children'] == int(shown), 'editor lifecycle mismatch'
            assert (0 < row['width'] <= 4096 and 0 < row['height'] <= 2160) if shown else (row['width'], row['height']) == (0, 0)
            assert all(type(row[k]) is int and row[k] >= 0 for k in ('rss_kib', 'hwm_kib', 'swap_kib'))
            assert 0 < row['rss_kib'] <= row['hwm_kib']
        values = [r['rss_kib']/1024 for r in samples]
        result[phase] = {'rss_mib': statistics.median(values), 'min_mib': min(values), 'max_mib': max(values)}
    result['open_delta_mib'] = result['open']['rss_mib']-result['loaded']['rss_mib']
    result['close_delta_mib'] = result['closed']['rss_mib']-result['loaded']['rss_mib']
    result['reopen_delta_mib'] = result['reopened']['rss_mib']-result['loaded']['rss_mib']
    return result


def selected_loads(rows, item, programs):
    latest = {}
    for row in rows:
        if (row.get('event') == 'load_finished' and row.get('path') == str(item)
                and row.get('program') in programs):
            latest[row['program']] = row.get('data',{}).get('status')
    return latest

def author_state(template, item):
    assert item.suffix.lower() != '.nkm' or item.name == 'Big Screen.nkm', 'only the inventoried two-part Big Screen multi is supported'
    programs = [0,1] if item.suffix.lower() == '.nkm' else [0]
    first = v1_state(template, str(item), 0)
    if len(programs) == 1: return first, programs
    parts = [keyed({'path': sized(str(item).encode()), 'program': struct.pack('<I', program),
                    'port': b'\0', 'channel': struct.pack('<h', -1), 'output': b'\0',
                    'output_manual': b'\1', 'gain': struct.pack('<f', 0), 'aux': struct.pack('<h', -1)})
             for program in programs]
    selection = keyed({'parts': struct.pack('<I',len(parts))+b''.join(sized(p) for p in parts),
                       'order': struct.pack('<III',2,0,1)})
    persist = struct.pack('<I',1)+sized(b'selection')+sized(sized(selection))
    at = 20+12*struct.unpack_from('<I',first,16)[0]
    at += 8+struct.unpack_from('<Q',first,at)[0]
    return first[:at]+struct.pack('<Q',len(persist))+persist, programs

def observe(host, plugin, item, folder):
    folder.mkdir(parents=True, exist_ok=False)
    receipt = dict(plugin_path=str(plugin), plugin_sha256=sha(plugin), host_sha256=sha(host),
                   item_sha256=sha(item), display=os.environ.get('DISPLAY'), status='UNKNOWN',
                   ui_selection='Original (private settings)', ui_scale=1, window=[1180, 760], host_viewport='X11 override_redirect (fixed, mapped on real DISPLAY)', effective_device_scale='CLAP-requested 1; renderer internal scale not exported',
                   settle_seconds=4, sample_count_per_step=10, sample_interval_seconds=.1,
                   close='hide + destroy; plugin/audio engine retained', explicit_gc=False,
                   malloc_trim=False, pixel_readback=False, audio='48kHz/64 frames paced; no MIDI',
                   timing_claim=False, source_artifact_feature_matched_to_stage_probe=False)
    with tempfile.TemporaryDirectory(prefix='kontra-editor-rss-', dir='/dev/shm') as root:
        root = Path(root); private_settings(root/'config')
        settings = root/'config/kontra/settings.json'
        values = json.loads(settings.read_text())
        values.update(uvi_imported=True, view_mode='Original', ui_scale=1.0, window_size=[1180, 760])
        settings.write_text(json.dumps(values))
        env = dict(os.environ, XDG_CONFIG_HOME=str(root/'config'))
        for key in ('PROBE_ALLOCS','KONTRA_SIGNAL_TRACE','KONTRA_AUDIT_STACKS','PROBE_EDITOR_SETTLE', 'KONTRA_GATE_ACTIVITY'):
            env.pop(key, None)
        capture = Capture(folder, env); job = None
        try:
            template = root/'template'; prefix = root/'readback'; ready = root/'ready'
            bootstrap = subprocess.run([str(host), str(plugin), '--template', str(template)], env=env,
                                       stdout=subprocess.PIPE, stderr=capture.stderr, timeout=30)
            assert bootstrap.returncode == 0, 'native plugin template failed'
            receipt.update(json.loads(bootstrap.stdout))
            capture.finish()
            session = folder/'session'; session.mkdir()
            capture = Capture(session, env)
            native = root/'state'; state, programs = author_state(template.read_bytes(), item); native.write_bytes(state)
            receipt['selected_programs'] = programs
            with tempfile.TemporaryFile(dir='/dev/shm') as output:
                job = subprocess.Popen([str(host), str(plugin), str(native), '5', str(ready), str(prefix)],
                                       stdout=output, stderr=capture.stderr, env=env)
                deadline = time.monotonic()+150
                while job.poll() is None:
                    assert time.monotonic() < deadline, 'host lifecycle deadline'
                    if not ready.exists():
                        selected = selected_loads(log_rows(capture.root), item, programs)
                        if len(selected) == len(programs) and all(s in ('loaded','partial') for s in selected.values()): ready.touch()
                        assert 'failed' not in selected.values(), 'selected plugin load failed'
                    time.sleep(.05)
                output.seek(0); raw = output.read()
            receipt.update(returncode=job.returncode, stdout_sha256=hashlib.sha256(raw).hexdigest())
            records = [json.loads(line) for line in raw.splitlines()]
            receipt['geometry_checks'] = [r for r in records if r.get('kind') == 'geometry']
            rows = [r for r in records if 'sample' in r]
            receipt['samples'] = rows
            logs = log_rows(capture.root)
            selected = selected_loads(logs,item,programs)
            finished = list(selected.values())
            receipt['selected_loads'] = selected
            receipt['renderer_stages'] = [label for r in logs if r.get('event') == 'native_window'
                for text,label in [('GPU ready','gpu-ready'),('GPU unavailable','gpu-unavailable'),('waiting for first frame','window-ready')]
                if text in r.get('data',{}).get('reason','')]
            receipt['load_statuses'] = finished
            assert job.returncode == 0, 'presented host failed'
            receipt['rss'] = summarize(rows)
            authored = native_selection(native.read_bytes())
            identities = [native_selection(Path(str(prefix)+'.'+phase).read_bytes()) for phase in PHASES]
            assert all(identity == authored for identity in identities), 'loaded selection changed'
            receipt['native_selection_verified'] = True
            receipt['selected_parts'] = len(authored[0])
            receipt['native_state_hashes'] = {phase: sha(Path(str(prefix)+'.'+phase)) for phase in PHASES}
            assert sum(s in ('loaded','partial') for s in finished) >= len(programs), 'selected load receipts incomplete'
            receipt['all_loads_complete'] = all(s == 'loaded' for s in finished)
            receipt['device_windows'] = {phase: [rows[i*10]['width'],rows[i*10]['height']] for i,phase in enumerate(PHASES)}
            receipt['requested_device_size_matched'] = all(receipt['device_windows'][phase] == [1180,760] for phase in ('open','reopened'))
            receipt['status'] = 'RAW_MEASURED'
        except (AssertionError, ValueError, OSError, subprocess.TimeoutExpired) as error:
            # Only host-owned validation labels are retained, never plugin text.
            receipt['failure'] = type(error).__name__
            if isinstance(error, AssertionError): receipt['failure_stage'] = str(error)
        finally:
            if job and job.poll() is None: job.kill(); job.wait()
            capture.stderr.flush(); capture.stderr.seek(0); errors = capture.stderr.read()
            receipt['host_failures'] = [stage for stage in ('GUI create','GUI scale 1','GUI fixed size','matched GUI size','RSS editor child','RSS mapped bounded editor','GUI hide','load deadline','native state save','GUI attach/show','RSS viewport agreement') if ('presented-host failure: '+stage).encode() in errors]
            capture.finish()
        diagnostics = (folder/'session' if (folder/'session').exists() else folder)/'plugin-diagnostics.json'
        values = json.loads(diagnostics.read_text()); values['plugin_host_run'] = True
        diagnostics.write_text(json.dumps(values)+'\n')
    (folder/'metrics.json').write_text(json.dumps(receipt, indent=2)+'\n')
    print(json.dumps({k: receipt[k] for k in ('plugin_sha256','status','rss','failure_stage') if k in receipt}), flush=True)
    return receipt['status'] == 'RAW_MEASURED'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('host', type=Path); parser.add_argument('plugin', type=Path)
    parser.add_argument('item', type=Path); parser.add_argument('output', type=Path)
    args = parser.parse_args()
    if args.plugin.resolve().is_relative_to(V1.resolve()):
        subprocess.run(['sha256sum','-c','SHA256SUMS'], cwd=V1, check=True, stdout=subprocess.DEVNULL)
    raise SystemExit(0 if observe(args.host.resolve(), args.plugin.resolve(), args.item.resolve(), args.output.resolve()) else 1)


if __name__ == '__main__': main()
