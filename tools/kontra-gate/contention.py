"""Linux activity evidence around timed workers; no command lines or library data."""
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time

HEAVY_NAMES = {'cargo', 'rustc', 'kontakto-heavy', 'kontakto-v1', 'kontra_scan', 'kontra_scan.py',
               'cpu_audit', 'cpu_audit_v1', 'cpu-audit-v1', 'cpu-audit-v2',
               'kontra-scan-v1', 'kontra-scan-v2', 'kontra-scan-v1-uvi',
               'audit-load.py', 'run-shards.py', 'v1-stage-probe', 'v1-uvi-onset-probe', 'clap-cpu-host'}
QUEUE = Path.home() / '.cache/kontakto-heavy.d/queue'
WAIT_HELPERS = {'sleep', 'find', 'ls', 'sort', 'head', 'grep', 'awk', 'seq', 'flock', 'tr', 'tail', 'df', 'date', 'mkdir', 'rm'}


def status(sample):
    if any(not row['owned'] and not row.get('waiting', False) for row in sample['units'] + sample['processes']):
        return 'CONTENDED'
    return 'UNKNOWN' if sample['errors'] else 'QUIET'


def snapshot():
    result = {'wall_ns': time.time_ns(), 'monotonic_ns': time.monotonic_ns(),
              'units': [], 'processes': [], 'loadavg': [], 'disk_io': {}, 'errors': []}
    parents, kinds, groups, commands = {}, {}, {}, {}
    try:
        for folder in Path('/proc').iterdir():
            if not folder.name.isdigit(): continue
            try:
                fields = folder.joinpath('stat').read_text().rsplit(') ', 1)[1].split()
                pid = int(folder.name); parents[pid] = int(fields[1])
                if fields[0] == 'Z': continue
                names = [Path(arg.decode(errors='replace')).name for arg in folder.joinpath('cmdline').read_bytes().split(b'\0')[:3]]
                commands[pid] = names[0]
                kind = next((name for name in names if name in HEAVY_NAMES), None)
                if kind: kinds[pid] = kind
                groups[pid] = folder.joinpath('cgroup').read_text()
            except (FileNotFoundError, ProcessLookupError): pass
            except (OSError, ValueError, IndexError): result['errors'].append('proc-process')
    except OSError: result['errors'].append('proc')
    ancestors = set(); pid = os.getpid()
    while pid and pid not in ancestors:
        ancestors.add(pid); pid = parents.get(pid, 0)
    descendants = {os.getpid()}
    while True:
        grown = descendants | {pid for pid, parent in parents.items() if parent in descendants}
        if grown == descendants: break
        descendants = grown
    owned = ancestors | descendants
    waiting = set()
    try:
        queued = {int(ticket.name.rsplit('.', 1)[1]) for ticket in QUEUE.iterdir()}
        for pid in queued:
            if kinds.get(pid) != 'kontakto-heavy': continue
            children = {pid}
            while True:
                grown = children | {child for child, parent in parents.items() if parent in children and child in commands}
                if grown == children: break
                children = grown
            # A queue ticket is removed before a slot's workload starts. Check
            # children too, so a stale ticket cannot hide a running workload.
            if all(commands[child] in WAIT_HELPERS for child in children - {pid}):
                waiting |= children
    except FileNotFoundError: pass
    except (OSError, ValueError, IndexError): result['errors'].append('heavy-queue')
    own_units = {part for pid in ancestors for part in groups.get(pid, '').split('/') if part.strip().endswith('.service')}
    own_units = {name.strip() for name in own_units}
    result['processes'] = [{'pid': pid, 'kind': kind, 'owned': pid in owned, **({'waiting': True} if pid in waiting else {})}
                           for pid, kind in sorted(kinds.items())]
    try:
        units = subprocess.run(['systemctl', '--user', 'list-units', '--state=active,activating',
                                '--plain', '--no-legend', '--no-pager', 'kontakto-*', '*census*'],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, timeout=3, check=True)
        result['units'] = [{'name': line.split()[0], 'owned': line.split()[0] in own_units}
                           for line in units.stdout.splitlines() if line.strip()]
        for unit in result['units']:
            members = {pid for pid, group in groups.items() if unit['name'] in group.strip().split('/')}
            if members and members <= waiting: unit['waiting'] = True
    except (OSError, subprocess.SubprocessError): result['errors'].append('systemctl')
    try:
        result['loadavg'] = [float(n) for n in Path('/proc/loadavg').read_text().split()[:3]]
        for line in Path('/proc/diskstats').read_text().splitlines():
            fields = line.split(); values = list(map(int, fields[3:]))
            result['disk_io'][fields[2]] = {'read_sectors': values[2], 'write_sectors': values[6],
                                           'in_flight': values[8], 'io_ms': values[9], 'weighted_io_ms': values[10]}
    except (OSError, ValueError, IndexError): result['errors'].append('loadavg-diskstats')
    return result


def wait_for_quiet(folder):
    folder.mkdir(parents=True, exist_ok=True)
    with (folder / 'activity.jsonl').open('a') as output:
        while True:
            sample = snapshot(); state = status(sample)
            output.write(json.dumps(dict(sample, phase='waiting', status=state)) + '\n'); output.flush()
            if state == 'QUIET': return
            if sample['errors']: raise RuntimeError('quiet activity observation unavailable')
            time.sleep(2)


class QuietBusy(Exception):
    pass


class Activity:
    def __init__(self, folder):
        self.folder = folder
        folder.mkdir(parents=True, exist_ok=True)
        self.output = (folder / 'activity.jsonl').open('a')
        self.stop = threading.Event()
        self.states = []

    def record(self, sample, phase='timed'):
        state = status(sample)
        self.output.write(json.dumps(dict(sample, phase=phase, status=state)) + '\n'); self.output.flush()
        if phase == 'timed': self.states.append(state)

    def start(self):
        sample = snapshot()
        if os.environ.get('KONTRA_GATE_REQUIRE_QUIET') == '1' and status(sample) != 'QUIET':
            self.record(sample, 'waiting'); self.output.close()
            (self.folder / 'activity.json').write_text(json.dumps({'status': 'WAITING', 'protocol': 1}) + '\n')
            raise QuietBusy()
        self.record(sample)
        def watch():
            while not self.stop.wait(1): self.record(snapshot())
        self.thread = threading.Thread(target=watch, daemon=True); self.thread.start()

    def finish(self):
        self.stop.set(); self.thread.join()
        self.record(snapshot()); self.output.close()
        state = 'CONTENDED' if 'CONTENDED' in self.states else 'UNKNOWN' if 'UNKNOWN' in self.states else 'QUIET'
        self.result = {'status': state, 'protocol': 1, 'samples': len(self.states), 'interval_s': 1}
        (self.folder / 'activity.json').write_text(json.dumps(self.result) + '\n')


def main():
    folder = Path(sys.argv[1]); activity = Activity(folder)
    try: activity.start()
    except QuietBusy: return 75  # Release the heavy slot; the parent waits before retrying.
    try: return subprocess.run(sys.argv[2:]).returncode
    finally: activity.finish()


if __name__ == '__main__': sys.exit(main())
