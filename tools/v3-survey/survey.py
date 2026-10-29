#!/usr/bin/env python3
"""v3 scoreboard: render every Kontakt and UVI instrument through the v3
engine and log what plays, what is silent, what fails and what is missing.

Each instrument plays key 60 (or, if silent there, 48/72/36/84) for 2 s at velocity 100, then at velocity 30
(the `render` example of kontra-host, one process per instrument). A run
writes to <out>/<UTC>-<sha>/:
  results.tsv  one row per instrument
  gaps.tsv     every diagnostic, ranked: Broken, Audible, Latent (closed at load), by instruments
  actions.tsv  script actions the engine does not answer, by instruments
  summary.md   per library, top gaps, top actions
  diff.md      against the previous run: fixed, regressed, gap counts
and appends one line to <out>/ledger.tsv.

Build first (once):
  build-slot cargo build --release -p kontra-host --example render --example uvi_programs
  tools/v3-survey/survey.py [--jobs 3] [--only TEXT] [--libraries DIR ...]
"""
import argparse, concurrent.futures as cf, math, os, re, statistics, subprocess, sys, tempfile, time
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = Path('/data/dev/workspace/PluginArtifacts/v3-survey')
LIBRARIES = [Path('/data/main/Libraries/Kontakt'), Path('/data/main/Libraries/UVI')]
SILENT = 1e-4
KEYS = (60, 48, 72, 36, 84)
COLUMNS = ['library', 'instrument', 'status', 'key', 'load_s', 'rss_mb', 'rtf', 'zones', 'key_zones',
           'peak', 'peak_v30', 'vel_db', 'rms_sustain', 'rms_release', 'broken', 'audible',
           'latent', 'cosmetic', 'unanswered', 'script_faults', 'stolen', 'cut', 'refused', 'missing_frames',
           'stream_errors', 'error']


def examples():
    target = subprocess.run(['build-slot', 'sh', '-c', 'echo $CARGO_TARGET_DIR'], cwd=ROOT,
                            capture_output=True, text=True).stdout.strip()
    return Path(target) / 'release' / 'examples'


def instruments(roots, bin_dir):
    found = []
    for root in roots:
        for nki in sorted(root.rglob('*.nki')):
            found.append((nki.relative_to(root).parts[0], str(nki)))
        banks = sorted(root.rglob('*.ufs'))
        if banks:
            listed = subprocess.run([str(bin_dir / 'uvi_programs'), *map(str, banks)],
                                    capture_output=True, text=True)
            for line in listed.stdout.splitlines():
                found.append((Path(line).relative_to(root).parts[0], line))
    return found


def number(pattern, text, cast=float, default=None):
    m = re.search(pattern, text, re.M)
    return cast(m.group(1)) if m else default


def render(binary, path, velocity, timeout, key=60):
    with tempfile.TemporaryDirectory(dir='/data/dev/workspace/scratch') as tmp:
        env = dict(os.environ, KONTRA_VELOCITY=str(velocity))
        try:
            p = subprocess.run(['nice', '-n', '19', str(binary), path, str(key), f'{tmp}/out.wav'], env=env,
                               capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return None, 'timeout'
    if p.returncode:
        tail = (p.stderr.strip().splitlines() or ['exit %d' % p.returncode])[-1]
        return p.stdout, f'exit {p.returncode}: {tail[:200]}'
    return p.stdout, ''


def survey(binary, library, path, timeout):
    row = dict.fromkeys(COLUMNS, '')
    row.update(library=library, instrument=path)
    # Key 60 first; an instrument silent there (out of its playable range) is
    # retried lower and higher, and the key that plays is recorded.
    for key in KEYS:
        out, error = render(binary, path, 100, timeout, key)
        if error or number(r'^peak ([\d.]+)', out, default=0.0) >= SILENT:
            break
    row['key'] = key
    if error:
        row.update(status='timeout' if error == 'timeout' else 'failed', error=error)
        return row, [], []
    gaps = re.findall(r'^  (Broken|Audible|Latent|Cosmetic) ([^:\n]+):', out, re.M)
    actions = re.search(r'^all actions: \[(.*)\]$', out, re.M)
    actions = re.findall(r'\((\w+), (\d+)\)', actions.group(1)) if actions else []
    faults = sum(int(n) for n in re.findall(r'\w+: (\d+)', ' '.join(
        re.findall(r'Faults \{([^}]*)\}', out))))
    peak = number(r'^peak ([\d.]+)', out, default=0.0)
    impacts = Counter(i for i, _ in gaps)
    row.update(
        load_s=number(r'loaded in ([\d.]+)s', out) or round(number(r'loaded in ([\d.]+)ms', out, default=0) / 1000, 4),
        rss_mb=round(number(r'memory after render: VmHWM: (\d+) kB', out, int, 0) / 1024),
        # Engine::process time over audio time, one note at velocity 100 (renders run in parallel at nice 19).
        rtf=number(r'rtf ([\d.]+)', out, default=''),
        zones=number(r': (\d+) zones, loaded', out, int, ''),
        key_zones=number(r'key \d+: (\d+) zones', out, int, ''),
        peak=peak,
        rms_sustain=number(r'rms 0\.5-1 s ([\d.]+)', out, default=''),
        rms_release=number(r'after release [\d.]+-[\d.]+ s ([\d.]+)', out, default=''),
        broken=impacts['Broken'], audible=impacts['Audible'], latent=impacts['Latent'],
        cosmetic=impacts['Cosmetic'],
        unanswered=sum(int(n) for _, n in actions), script_faults=faults,
        stolen=number(r'voices_stolen: (\d+)', out, int, ''),
        cut=number(r'voices_cut: (\d+)', out, int, ''),
        refused=(number(r'notes_refused: (\d+)', out, int, 0) or 0) + (number(r'events_refused: (\d+)', out, int, 0) or 0),
        missing_frames=number(r'missing_frames: (\d+)', out, int, ''),
        stream_errors=number(r'read_errors: (\d+)', out, int, ''),
        status='silent' if peak < SILENT else 'ok')
    soft, error = render(binary, path, 30, timeout, key)
    if not error:
        row['peak_v30'] = number(r'^peak ([\d.]+)', soft, default=0.0)
        if peak >= SILENT and row['peak_v30'] >= SILENT:
            row['vel_db'] = round(20 * math.log10(peak / row['peak_v30']), 1)
    return row, sorted(set(gaps)), [a for a, _ in actions]


def write_tsv(path, header, rows):
    with open(path, 'w') as f:
        f.write('\t'.join(header) + '\n')
        f.writelines('\t'.join(str(r[h]) for h in header) + '\n' for r in rows)


def read_rows(path):
    lines = path.read_text().splitlines()
    head = lines[0].split('\t')
    return [dict(zip(head, l.split('\t'))) for l in lines[1:]]


def median(values):
    values = [float(v) for v in values if v not in ('', None)]
    return round(statistics.median(values), 3) if values else ''


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--jobs', type=int, default=3, help='parallel renders, each at nice 19 (keep the desktop and DAW responsive)')
    ap.add_argument('--only', help='only instruments whose path contains this')
    ap.add_argument('--libraries', nargs='*', type=Path, default=LIBRARIES)
    ap.add_argument('--out', type=Path, default=OUT)
    ap.add_argument('--timeout', type=int, default=180)
    args = ap.parse_args()

    bin_dir = examples()
    binary = bin_dir / 'render'
    if not binary.exists():
        sys.exit(f'{binary} missing: build it first (see --help)')
    todo = [(l, p) for l, p in instruments(args.libraries, bin_dir) if not args.only or args.only in p]
    sha = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    stamp = time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())
    run = args.out / f'{stamp}-{sha}'
    run.mkdir(parents=True)
    print(f'{len(todo)} instruments → {run}', flush=True)

    rows, gap_use, action_use = [], defaultdict(lambda: [set(), set()]), defaultdict(lambda: [set(), set()])
    with cf.ThreadPoolExecutor(args.jobs) as pool:
        jobs = {pool.submit(survey, binary, l, p, args.timeout): (l, p) for l, p in todo}
        for n, job in enumerate(cf.as_completed(jobs), 1):
            row, gaps, actions = job.result()
            rows.append(row)
            for gap in gaps:
                gap_use[gap][0].add(row['instrument']); gap_use[gap][1].add(row['library'])
            for a in actions:
                action_use[a][0].add(row['instrument']); action_use[a][1].add(row['library'])
            if n % 25 == 0 or n == len(todo):
                print(f'  {n}/{len(todo)}', flush=True)
    rows.sort(key=lambda r: (r['library'], r['instrument']))
    write_tsv(run / 'results.tsv', COLUMNS, rows)

    order = {'Broken': 0, 'Audible': 1, 'Latent': 2, 'Cosmetic': 3}
    gaps = sorted(gap_use.items(), key=lambda kv: (order[kv[0][0]], -len(kv[1][0]), kv[0][1]))
    with open(run / 'gaps.tsv', 'w') as f:
        f.write('impact\tfeature\tinstruments\tlibraries\tlibrary names\n')
        for (impact, feature), (ins, libs) in gaps:
            f.write(f'{impact}\t{feature}\t{len(ins)}\t{len(libs)}\t{", ".join(sorted(libs))}\n')
    actions = sorted(action_use.items(), key=lambda kv: -len(kv[1][0]))
    with open(run / 'actions.tsv', 'w') as f:
        f.write('action\tinstruments\tlibraries\n')
        for a, (ins, libs) in actions:
            f.write(f'{a}\t{len(ins)}\t{len(libs)}\n')

    by_lib = defaultdict(list)
    for r in rows:
        by_lib[r['library']].append(r)
    status = Counter(r['status'] for r in rows)
    md = [f'# v3 survey {stamp} ({sha})', '',
          f'{len(rows)} instruments: {status["ok"]} play, {status["silent"]} silent, '
          f'{status["failed"]} fail to load, {status["timeout"]} time out.', '',
          '| Library | Instruments | Play | Silent | Fail | Median load s | Max RSS MB | Broken | Audible gaps | Median vel 100/30 dB |',
          '|---|---|---|---|---|---|---|---|---|---|']
    for lib, rs in sorted(by_lib.items()):
        s = Counter(r['status'] for r in rs)
        md.append(f'| {lib} | {len(rs)} | {s["ok"]} | {s["silent"]} | {s["failed"] + s["timeout"]} | '
                  f'{median(r["load_s"] for r in rs)} | {max([int(r["rss_mb"] or 0) for r in rs] or [0])} | '
                  f'{sum(int(r["broken"] or 0) for r in rs)} | {sum(int(r["audible"] or 0) for r in rs)} | '
                  f'{median(r["vel_db"] for r in rs)} |')
    heavy = sorted((r for r in rows if r['rtf'] not in ('', None)), key=lambda r: -float(r['rtf']))[:10]
    md += ['', '## Heaviest renders (Engine::process time / audio time, one note)', '', '| Library | Instrument | RTF | Zones |', '|---|---|---|---|']
    md += [f'| {r["library"]} | {Path(r["instrument"]).name} | {r["rtf"]} | {r["zones"]} |' for r in heavy]
    md += ['', '## Top gaps (Broken and Audible, by instruments)', '', '| Impact | Feature | Instruments | Libraries |', '|---|---|---|---|']
    md += [f'| {i} | {f} | {len(ins)} | {len(libs)} |' for (i, f), (ins, libs) in gaps if i in ('Broken', 'Audible')][:30]
    # Closed at load (an effect on an aux no send reaches): audible only once a script opens it.
    latent = [(f, ins, libs) for (i, f), (ins, libs) in gaps if i == 'Latent']
    md += ['', f'## Latent gaps (closed at load; {sum(int(r["latent"] or 0) for r in rows)} in {sum(1 for r in rows if r["latent"])} instruments)',
           '', '| Feature | Instruments | Libraries |', '|---|---|---|']
    md += [f'| {f} | {len(ins)} | {len(libs)} |' for f, ins, libs in latent]
    md += ['', '## Top unanswered script actions', '', '| Action | Instruments | Libraries |', '|---|---|---|']
    md += [f'| {a} | {len(ins)} | {len(libs)} |' for a, (ins, libs) in actions[:20]]
    failed = [r for r in rows if r['status'] in ('failed', 'timeout', 'silent')]
    if failed:
        md += ['', '## Not playing', ''] + [f'- {r["status"]}: {r["instrument"]} {r["error"]}' for r in failed[:80]]
    (run / 'summary.md').write_text('\n'.join(md) + '\n')

    previous = sorted(p for p in args.out.iterdir() if p.is_dir() and p != run and (p / 'results.tsv').exists())
    if previous:
        old = {r['instrument']: r for r in read_rows(previous[-1] / 'results.tsv')}
        diff = [f'# Diff against {previous[-1].name}', '']
        fixed = [r for r in rows if r['instrument'] in old and old[r['instrument']]['status'] != 'ok' and r['status'] == 'ok']
        broke = [r for r in rows if r['instrument'] in old and old[r['instrument']]['status'] == 'ok' and r['status'] != 'ok']
        diff += [f'Now playing: {len(fixed)}. Regressed: {len(broke)}.', '']
        diff += [f'- REGRESSED {r["status"]}: {r["instrument"]} {r["error"]}' for r in broke]
        diff += [f'- fixed: {r["instrument"]}' for r in fixed]
        for lib, rs in sorted(by_lib.items()):
            before = [o for o in old.values() if o['library'] == lib]
            if not before:
                continue
            g0 = sum(int(o['audible'] or 0) + int(o['broken'] or 0) for o in before)
            g1 = sum(int(r['audible'] or 0) + int(r['broken'] or 0) for r in rs)
            l0, l1 = median(o['load_s'] for o in before), median(r['load_s'] for r in rs)
            diff.append(f'- {lib}: audible+broken gaps {g0} → {g1}, median load {l0} → {l1} s')
        (run / 'diff.md').write_text('\n'.join(diff) + '\n')

    ledger = args.out / 'ledger.tsv'
    new = not ledger.exists()
    with open(ledger, 'a') as f:
        if new:
            f.write('run\tsha\tinstruments\tplay\tsilent\tfail\tbroken\taudible\tmedian_load_s\tmedian_rss_mb\n')
        f.write(f'{run.name}\t{sha}\t{len(rows)}\t{status["ok"]}\t{status["silent"]}\t'
                f'{status["failed"] + status["timeout"]}\t{sum(int(r["broken"] or 0) for r in rows)}\t'
                f'{sum(int(r["audible"] or 0) for r in rows)}\t{median(r["load_s"] for r in rows)}\t'
                f'{median(r["rss_mb"] for r in rows)}\n')
    print((run / 'summary.md').read_text())


if __name__ == '__main__':
    main()
