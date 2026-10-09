#!/usr/bin/env python3
"""Unscored frozen-CLAP smoke test using an original generated tone, never library PCM.

python3 tools/check-cpu-audit-native.py /absolute/clap-cpu-host OUT
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from test_native_midi import create_fixture

spec = importlib.util.spec_from_file_location('native_cpu', Path(__file__).with_name('cpu-audit-native.py'))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
from live_host import V1, native_selection, private_settings, v1_state

host, out = (Path(arg).resolve() for arg in sys.argv[1:])
assert not (Path.home() / '.cache/kontra-quiet-request').exists(), 'do not intrude on a scored machine window'
subprocess.run(['sha256sum', '-c', 'SHA256SUMS'], cwd=V1, check=True, stdout=subprocess.DEVNULL)
with tempfile.TemporaryDirectory(prefix='kontra-cpu-smoke-', dir='/dev/shm') as temp:
    temp = Path(temp)
    private_settings(temp / 'config')
    os.environ.update(XDG_CONFIG_HOME=str(temp / 'config'), XDG_DATA_HOME=str(temp / 'data'),
                      XDG_CACHE_HOME='/dev/null', KONTRA_DISABLE_NETWORK='1',
                      KONTRA_LOG_DIR=str(temp / 'logs'), KONTRA_REPORT_DIR=str(temp / 'reports'))
    nki = create_fixture(V1 / 'bin/kontakto-v1', temp)
    multi = temp / 'input.kontra-multi'
    multi.write_text(json.dumps(audit.audit_multi(nki, 'v1')))
    native = temp / 'input.state'
    subprocess.run([str(V1 / 'bin/kontakto-v1'), 'export-multi-state', str(multi), str(native)],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    state = v1_state(native.read_bytes(), str(nki), 0)
    assert native_selection(state)[0][0][0][4:] == str(nki).encode()
    for block in (32, 64, 256):
        assert not (Path.home() / '.cache/kontra-quiet-request').exists(), 'quiet window started; stop smoke runs'
        cell = audit.observe(host, V1 / 'plugin/KONTRA.clap', state, audit.audit_events('piano'),
                             block, 4, out / str(block), 'v1', cpu_audit=True, profile=block == 64)
        # This test exercises correctness, never certifies CPU parity.
        cell['status'] = 'UNSCORED-SMOKE'
        (out / str(block) / 'metrics.json').write_text(json.dumps(cell, indent=2) + '\n')
        assert cell['returncode'] == 0 and cell['native_state_verified']
        assert cell['events_dispatched'] == cell['events_planned'] == 20
        assert cell['nonfinite'] == 0 and cell['cpu_audit']['steady_peak'] > 0
        assert cell['cpu_audit']['all']['blocks'] == 192000 // block
        assert cell['cpu_audit']['steady']['blocks'] == sum(12000 <= at < 48000 for at in range(0, 192000, block))
        if block == 64:
            assert cell['profiler_exit_code'] in (0, -2) and cell['profile_samples_steady'] > 0
            assert (out / str(block) / 'audio-steady.perf.txt').stat().st_size > 0
        print(f'PASS: frozen CLAP, native state readback, exact audit events/window, block={block}', flush=True)
