#!/usr/bin/env python3
"""Targeted contention checks; no libraries, cargo or heavy slots."""
import json
import csv
import hashlib
import os
import sys
from pathlib import Path
import tempfile
from unittest.mock import patch
from types import SimpleNamespace

import contention
import gate

quiet = {'units': [], 'processes': [], 'errors': [], 'loadavg': [0, 0, 0], 'disk_io': {}}
busy = dict(quiet, units=[{'name': 'kontakto-census.service', 'owned': False}])
assert contention.status(busy) == 'CONTENDED'
assert contention.status(dict(quiet, units=[{'name': 'kontakto-gate.service', 'owned': True}])) == 'QUIET'
assert contention.status(dict(quiet, errors=['systemctl'])) == 'UNKNOWN'
assert contention.status(dict(quiet, units=[{'name': 'kontakto-wait.service', 'owned': False,
                                           'waiting': True, 'cpu_io_active': True}])) == 'CONTENDED'
assert gate.verdict(['PASS', 'CONTENDED']) == 'UNKNOWN'
assert gate.verdict(['CONTENDED']) == 'UNKNOWN'
assert gate.timed_compare(1, 2, ['QUIET', 'CONTENDED']) == 'CONTENDED'
assert gate.timed_compare(1, 2, ['QUIET', 'QUIET']) == 'FAIL'
assert gate.timed_compare(0, 0, ['QUIET', 'CONTENDED'], 0) == 'CONTENDED'
assert gate.timed_compare(0, 0, ['QUIET', 'QUIET'], 0) == 'PASS'
assert gate.timed_compare(2, 1, ['UNKNOWN', 'QUIET']) == 'UNKNOWN'

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    for pid, parent, args, unit in [(1, 0, ['systemd'], 'user@1000.service'),
                                   (20, 1, ['python3', 'gate.py'], 'kontakto-gate.service'),
                                   (21, 20, ['kontra-scan-v2'], 'kontakto-gate.service'),
                                   (30, 1, ['cargo'], 'kontakto-build.service')]:
        proc = root / str(pid); proc.mkdir()
        proc.joinpath('stat').write_text(f'{pid} (fixture) S {parent}')
        proc.joinpath('cmdline').write_bytes(b'\0'.join(arg.encode() for arg in args))
        proc.joinpath('cgroup').write_text('0::/' + unit + '\n')
    root.joinpath('loadavg').write_text('1 2 3 1/4 5')
    root.joinpath('diskstats').write_text('8 0 sda 0 0 512 0 0 0 1024 0 1 4 5')
    def fake_path(value):
        text = str(value)
        if text == '/sys/fs/cgroup': return root / 'cgroups'
        return root if text == '/proc' else root / text[6:] if text.startswith('/proc/') else Path(value)
    with patch.object(contention, 'Path', side_effect=fake_path), patch.object(contention.os, 'getpid', return_value=20), patch.object(contention.subprocess, 'run', return_value=SimpleNamespace(stdout='kontakto-gate.service loaded active running\nkontakto-build.service loaded active running\n')):
        observed = contention.snapshot()
    assert observed['errors'] == [] and observed['loadavg'] == [1, 2, 3]
    assert observed['processes'] == [{'pid': 21, 'kind': 'kontra-scan-v2', 'owned': True}, {'pid': 30, 'kind': 'cargo', 'owned': False}]
    assert observed['units'][0]['owned'] and not observed['units'][1]['owned']
    assert observed['disk_io']['sda']['read_sectors'] == 512 and contention.status(observed) == 'CONTENDED'

    # Both queued and ticketless wrappers are idle until a workload starts.
    proc = root / '30'
    proc.joinpath('cmdline').write_bytes(b'bash\0/home/user/.cache/kontakto-heavy\0cargo')
    queue = root / 'queue'; queue.mkdir(); queue.joinpath('123.30').touch()
    with patch.object(contention, 'Path', side_effect=fake_path), patch.object(contention.os, 'getpid', return_value=20), patch.object(contention.subprocess, 'run', return_value=SimpleNamespace(stdout='kontakto-build.service loaded active running\n')):
        observed = contention.snapshot()
        assert contention.status(observed) == 'QUIET'
        assert observed['processes'][-1]['waiting'] and observed['units'][0]['waiting']
        # A normal systemd shell/Python runner waits above its heavy wrapper.
        parent = root / '29'; parent.mkdir()
        parent.joinpath('stat').write_text('29 (fixture) S 1')
        parent.joinpath('cmdline').write_bytes(b'python3\0runner.py')
        parent.joinpath('cgroup').write_text('0::/kontakto-build.service\n')
        proc.joinpath('stat').write_text('30 (fixture) S 29')
        assert contention.status(contention.snapshot()) == 'QUIET'
        child = root / '31'; child.mkdir()
        child.joinpath('stat').write_text('31 (fixture) S 30')
        child.joinpath('cmdline').write_bytes(b'sleep\0' + b'5')
        child.joinpath('cgroup').write_text('0::/kontakto-build.service\n')
        assert contention.status(contention.snapshot()) == 'QUIET'
        child.joinpath('cmdline').write_bytes(b'cargo\0test')
        assert contention.status(contention.snapshot()) == 'CONTENDED'
        child.joinpath('cmdline').write_bytes(b'sleep\0' + b'5')
        queue.joinpath('123.30').unlink()
        assert contention.status(contention.snapshot()) == 'QUIET'
        cgroup = root / 'cgroups/kontakto-build.service'; cgroup.mkdir(parents=True)
        cgroup.joinpath('cpu.stat').write_text('usage_usec 100\n')
        cgroup.joinpath('io.stat').write_text('8:0 rbytes=0 wbytes=0 rios=0 wios=0\n')
        contention.UNIT_COUNTERS.clear()
        assert contention.status(contention.snapshot()) == 'QUIET'
        cgroup.joinpath('cpu.stat').write_text('usage_usec 101\n')
        assert contention.status(contention.snapshot()) == 'CONTENDED'
        assert contention.status(contention.snapshot()) == 'QUIET'
        cgroup.joinpath('io.stat').write_text('8:0 rbytes=512 wbytes=0 rios=1 wios=0\n')
        assert contention.status(contention.snapshot()) == 'CONTENDED'
        child.joinpath('cmdline').write_bytes(b'fixture-renderer\0')
        assert contention.status(contention.snapshot()) == 'CONTENDED'
        child.joinpath('cmdline').write_bytes(b'sleep\0' + b'5')
        unrelated = root / '32'; unrelated.mkdir()
        unrelated.joinpath('stat').write_text('32 (fixture) S 1')
        unrelated.joinpath('cmdline').write_bytes(b'fixture-io\0')
        unrelated.joinpath('cgroup').write_text('0::/kontakto-build.service\n')
        assert contention.status(contention.snapshot()) == 'CONTENDED'

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    with patch.dict(os.environ, {'KONTRA_GATE_REQUIRE_QUIET': '1'}), patch.object(contention, 'snapshot', side_effect=[busy, quiet]), patch.object(contention.time, 'sleep') as sleep:
        contention.wait_for_quiet(root)
        sleep.assert_called_once()
    with patch.dict(os.environ, {'KONTRA_GATE_REQUIRE_QUIET': '0'}), patch.object(contention, 'snapshot', side_effect=[quiet, busy]):
        activity = contention.Activity(root); activity.start(); activity.finish()
    assert json.loads((root / 'activity.json').read_text())['status'] == 'CONTENDED'
    timeline = [json.loads(line) for line in (root / 'activity.jsonl').read_text().splitlines()]
    assert [row['phase'] for row in timeline] == ['waiting', 'waiting', 'timed', 'timed']

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    with patch.dict(os.environ, {'KONTRA_GATE_REQUIRE_QUIET': '1'}), patch.object(contention, 'snapshot', return_value=busy):
        try:
            contention.Activity(root).start()
        except contention.QuietBusy:
            pass
        else:
            raise AssertionError('a contended worker must not start in quiet mode')
    assert json.loads((root / 'activity.json').read_text())['status'] == 'WAITING'

with tempfile.TemporaryDirectory() as tmp:
    run = Path(tmp); path = 'fixture.nki'; item = hashlib.sha256(path.encode()).hexdigest()
    gate.write_json(run / 'manifest.json', {'sha': 'fixture', 'binaries': {}, 'conditions': ['cold']})
    run.joinpath('items.tsv').write_text(item + '\t' + path + '\n')
    for version, load, state in [('v1', 1, 'QUIET'), ('v2', 10, 'CONTENDED')]:
        folder = run / version / 'cold'; folder.mkdir(parents=True)
        row = {'path': path, 'loads': 'yes', 'ui': 'original-ok', 'plays_note': 'yes', 'audition_status': 'matched-note-plan',
               'load_ms': load, 'first_audio_ms': load, 'peak_rss_mb': load, 'underruns': 0, 'contention': state}
        with folder.joinpath('results.tsv').open('w') as output:
            writer = csv.DictWriter(output, row.keys(), delimiter='\t'); writer.writeheader(); writer.writerow(row)
    gate.write_json(run / 'cpu.json', {'cells': [{'item_sha256': item, 'block': 64, 'version': version, 'returncode': 0,
                      'peak': 0.5, 'cpu_p50_us': load, 'cpu_p99_us': load, 'underruns': 0, 'deadline_misses': 0,
                      'contention': state} for version, load, state in [('v1', 1, 'CONTENDED'), ('v2', 10, 'QUIET')]]})
    result = gate.summarize(run, True)
    affected = [row for row in result['metrics'] if row['condition'] == 'cold' and row['metric'] in ['load_ms', 'first_audio_ms', 'peak_rss_mb', 'underruns'] or row['condition'] == 'cpu-runtime-block-64']
    assert affected and all(row['verdict'] == 'CONTENDED' for row in affected)
    assert result['axes']['beats-v1-every-metric'] == 'UNKNOWN'
    assert 'CONTENDED' in run.joinpath('summary.md').read_text()

    # A rejected held-note plan is no valid silence observation.
    for status, expected in [('invalid-note-plan', 'UNKNOWN'), ('audition-mismatch', 'UNKNOWN'), ('matched-note-plan', 'FAIL')]:
        row.update(plays_note='silent', audition_status=status)
        with folder.joinpath('results.tsv').open('w') as output:
            writer = csv.DictWriter(output, row.keys(), delimiter='\t'); writer.writeheader(); writer.writerow(row)
        assert gate.summarize(run)['axes']['DSP'] == expected, status

import adapters
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    with patch.object(adapters, 'QUIET_REQUEST', root / 'absent-request'), patch.object(adapters, 'HEAVY', Path('/usr/bin/env')), patch.dict(os.environ, {'KONTRA_GATE_REQUIRE_QUIET': '0'}):
        records, witness, _ = adapters.capture([sys.executable, '-c', 'print(\'{"block":64}\')'], root, timed=True)
    assert records == [{'block': 64}] and witness['returncode'] == 0
    assert witness['contention'] in ['QUIET', 'CONTENDED', 'UNKNOWN']
    assert len(root.joinpath('activity.jsonl').read_text().splitlines()) >= 2

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp); calls = []
    def admitted(args, **kwargs):
        calls.append(args)
        waiting = len(calls) == 1
        root.joinpath('activity.json').write_text(json.dumps({'status': 'WAITING' if waiting else 'QUIET'}))
        if not waiting: kwargs['stdout'].write(b'{"block":64}\n')
        return SimpleNamespace(returncode=75 if waiting else 0)
    with patch.object(adapters, 'QUIET_REQUEST', root / 'absent-request'), patch.dict(os.environ, {'KONTRA_GATE_REQUIRE_QUIET': '1'}), patch.object(adapters, 'wait_for_quiet') as wait, patch.object(adapters.subprocess, 'run', side_effect=admitted):
        records, witness, _ = adapters.capture(['fixture-worker'], root, timed=True)
    assert records == [{'block': 64}] and witness['contention'] == 'QUIET' and wait.call_count == 2
    assert len(calls) == 2 and Path(calls[0][4]).name == 'contention.py'

import importlib.util
spec = importlib.util.spec_from_file_location('scanner_contention_test', Path(__file__).parent.parent / 'kontra-scan/kontra_scan.py')
scanner = importlib.util.module_from_spec(spec); spec.loader.exec_module(scanner)
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp); scanner.NOTE_ROOT = root / 'notes'
    with patch.dict(os.environ, {'KONTRA_GATE_REQUIRE_QUIET': '1', 'KONTRA_GATE_CAPTURE': '1', 'KONTRA_GATE_ACTIVITY': '1'}), patch.object(contention, 'snapshot', return_value=busy), patch.object(scanner.subprocess, 'Popen') as child:
        result = scanner.probe(root / 'engine', 'fixture.nki', root / 'item', 1, False)
    assert result == {'gate_quiet_retry': True}
    child.assert_not_called()

print('contention timeline, process-family exclusion, quiet waiting, CPU wrapper and release UNKNOWN checks passed')
