#!/usr/bin/env python3
"""One shared, resumable CLI for the optimized v1 and v2 scan adapters. Stdlib only."""
import argparse
import csv
import fcntl
import glob
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

COLUMNS = ['path', 'library', 'loads', 'ui', 'controls_bound', 'plays_note', 'load_ms', 'peak_rss_mb', 'reason']


def library(item):
    path = item.split('::', 1)[0]
    for marker in ['/Libraries/Kontakt/', '/Libraries/UVI/']:
        if marker in path:
            return path.split(marker, 1)[1].split('/', 1)[0]
    return Path(path).parent.name


def items(spec):
    p = Path(spec)
    if p.is_file() and p.suffix.lower() in {'.tsv', '.txt'}:
        rows = [r.split('\t', 1)[-1] for r in p.read_text().splitlines() if r and not r.startswith('#')]
    else:
        rows = sorted(glob.glob(spec, recursive=True))
    return list(dict.fromkeys(rows))


def signature(item, revision):
    path = item.split('::', 1)[0]
    try:
        s = Path(path).stat()
        identity = [item, s.st_size, s.st_mtime_ns, revision]
    except OSError:
        identity = [item, 'absent', revision]
    return hashlib.sha256(json.dumps(identity).encode()).hexdigest()


def atomic(path, value):
    tmp = path.with_suffix('.tmp')
    tmp.write_text(json.dumps(value, separators=(',', ':')) + '\n')
    tmp.replace(path)


def export(out, revision):
    records = []
    for p in (out / 'cache').glob('*.json'):
        record = json.loads(p.read_text())
        if record.get('revision') == revision:
            records.append(record)
    tmp = out / 'results.tmp'
    with tmp.open('w') as f:
        writer = csv.writer(f, delimiter='\t', lineterminator='\n')
        writer.writerow(COLUMNS)
        for r in sorted(records, key=lambda r: r['path']):
            writer.writerow([str(r.get(k, '')).replace('\t', ' ').replace('\n', ' ') for k in COLUMNS])
    tmp.replace(out / 'results.tsv')
    return len(records)


def probe(engine, item, work, timeout, shots):
    work.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env['KONTRA_SCAN_ACTIVE'] = '1'
    reader = Path('/home/derpcat/.codex/cache/kontakto-uvi-official-reader/app/UVIWorkstationx64.exe')
    if reader.is_file():
        env.setdefault('KONTRA_UVI_READER', str(reader))
    if shots:
        env['KONTRA_SCAN_SHOTS'] = '1'
    with (work / 'stdout.json').open('w') as output:
        child = subprocess.Popen([str(engine), '--worker', item, str(work)], stdout=output,
                                 stderr=subprocess.DEVNULL, start_new_session=True, env=env)
        started = time.monotonic()
        rss = 0
        timed_out = False
        while child.poll() is None:
            try:
                for line in Path(f'/proc/{child.pid}/status').read_text().splitlines():
                    if line.startswith(('VmHWM:', 'VmRSS:')):
                        rss = max(rss, int(line.split()[1]) / 1024)
            except (OSError, ValueError):
                pass
            if time.monotonic() - started >= timeout:
                timed_out = True
                os.killpg(child.pid, signal.SIGKILL)
                break
            time.sleep(0.1)
        child.wait()
    try:
        r = json.loads((work / 'stdout.json').read_text())
    except (ValueError, OSError):
        try:
            r = json.loads((work / 'progress.json').read_text())
        except (ValueError, OSError):
            r = {}
        r.update(loads='no', plays_note='no')
        r.setdefault('ui', 'error')
        r['reason'] = ('timeout' if timed_out else f'worker exit {child.returncode}') + ' at ' + r.get('stage', 'start')
    r['peak_rss_mb'] = round(max(rss, r.get('peak_rss_mb', 0)), 2)
    r['process_ms'] = round((time.monotonic() - started) * 1000, 2)
    r.setdefault('load_ms', r['process_ms'])
    r.setdefault('controls_bound', '0/0')
    r.setdefault('reason', '')
    r['timed_out'] = timed_out
    # stdout is metrics only; keep one canonical cached record, not a second copy.
    (work / 'stdout.json').unlink(missing_ok=True)
    (work / 'progress.json').unlink(missing_ok=True)
    return r


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', required=True)
    parser.add_argument('--list', required=True, help='items TSV or quoted glob (zero-based indexing)')
    parser.add_argument('--start', type=int, default=0)
    parser.add_argument('--count', type=int, default=25)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--budget-seconds', type=float, default=235)
    parser.add_argument('--timeout-seconds', type=float, default=90)
    parser.add_argument('--shots', action='store_true', help='retain small screenshots of OUR Original renderer')
    args = parser.parse_args()
    if args.start < 0 or args.count < 0 or not 0 < args.budget_seconds <= 240:
        parser.error('start/count must be nonnegative; shard budget must be in (0,240]')
    engine = Path(args.engine).resolve()
    revision = hashlib.sha256(engine.read_bytes()).hexdigest()
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'cache').mkdir(exist_ok=True)
    with (args.out / '.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        selected = items(args.list)[args.start:args.start + args.count]
        started = time.monotonic()
        done = reused = 0
        for item in selected:
            key = signature(item, revision)
            cached = args.out / 'cache' / (key + '.json')
            if cached.exists():
                reused += 1
                continue
            remaining = args.budget_seconds - (time.monotonic() - started)
            if remaining < 1:
                break
            r = probe(engine, item, args.out / 'items' / key, min(args.timeout_seconds, remaining), args.shots)
            r.update(path=item, library=library(item), revision=revision)
            atomic(cached, r)
            done += 1
            print(f'{done}: {r["loads"]} {r["ui"]} {item}', flush=True)
        total = export(args.out, revision)
        print(f'shard: new={done} reused={reused} selected={len(selected)} total_cached={total} wall={time.monotonic()-started:.1f}s', flush=True)
        return 0 if done + reused == len(selected) else 75


if __name__ == '__main__':
    raise SystemExit(main())
