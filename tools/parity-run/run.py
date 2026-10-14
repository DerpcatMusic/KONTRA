#!/usr/bin/env python3
"""Post-merge "does it actually work" run over tools/parity-run/instruments.tsv.

Builds the shots test binary once (heavy slot, -j 4), runs the ignored
`parity_run` test per instrument (one process each, so peak RSS is per load),
then writes run.json, summary.md and a diff against the previous run under
/mnt/Windows11/DEV_WORKSPACE/parity-runs/<timestamp>/.

    tools/parity-run/run.py [--only NAME] [--presets N] [--controls N] [--binary PATH]
"""
import argparse, json, os, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RUNS = Path('/mnt/Windows11/DEV_WORKSPACE/parity-runs')
HEAVY = os.path.expanduser('~/.cache/kontakto-heavy')
TEST = 'ui::widget_gate::parity_run::parity_run'


def build():
    cmd = [HEAVY, 'cargo', 'test', '-j', '4', '--profile', 'ci', '--lib', '--features', 'shots',
           '--no-run', '--message-format=json']
    p = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, text=True)
    exe = None
    for line in p.stdout.splitlines():
        try:
            r = json.loads(line)
        except ValueError:
            continue
        if r.get('reason') == 'compiler-artifact' and r.get('profile', {}).get('test') and r.get('executable'):
            exe = r['executable']
    if p.returncode or not exe:
        sys.exit(f'build failed ({p.returncode})')
    return exe


def instruments():
    rows = []
    for line in (Path(__file__).parent / 'instruments.tsv').read_text().splitlines():
        if line.strip() and not line.startswith('#'):
            name, path = line.split('\t')[:2]
            rows.append((name, path))
    return rows


def run_one(exe, name, path, out, args):
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, KONTRA_PARITY_PATH=path, KONTRA_PARITY_OUT=str(out),
               KONTRA_PARITY_PRESETS=str(args.presets), KONTRA_PARITY_CONTROLS=str(args.controls))
    t = time.time()
    try:
        p = subprocess.run([HEAVY, exe, '--ignored', '--exact', TEST, '--nocapture', '--test-threads=1'],
                           cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                           timeout=args.timeout)
        code, log = p.returncode, p.stdout
    except subprocess.TimeoutExpired as e:
        code, log = 'timeout', (e.stdout or b'').decode(errors='replace') if isinstance(e.stdout, bytes) else (e.stdout or '')
    # Keep only the tail; panics name harness lines, not library content.
    (out / 'test.log').write_text('\n'.join(log.splitlines()[-60:]))
    result = out / 'result.json'
    if result.exists():
        r = json.loads(result.read_text())
    else:
        r = {'path': path, 'checks': [{'check': 'harness', 'pass': False, 'detail': {'exit': code}}]}
    r['name'], r['wall_s'], r['exit'] = name, round(time.time() - t, 1), code
    # WAVs are never written; clear anything a crashed run might leave.
    for wav in out.glob('*.wav'):
        wav.unlink()
    return r


def checks(run):
    return {(i['name'], c['check']): c['pass'] for i in run['instruments'] for c in i['checks']}


def summary(run, previous):
    lines = [f"# Parity run {run['stamp']}", '', f"commit `{run['commit']}`", '']
    for i in run['instruments']:
        fails = [c for c in i['checks'] if not c['pass']]
        load = i.get('load', {})
        lines += [f"## {i['name']}: {len(i['checks']) - len(fails)}/{len(i['checks'])} pass",
                  f"load {load.get('ms')} ms, peak RSS {load.get('peak_rss_mb')} MB, unsupported {load.get('unsupported')}, "
                  f"partial {load.get('partial')}, script faults {load.get('script_faults')}", '']
        for c in fails:
            lines.append(f"- FAIL `{c['check']}` {json.dumps(c['detail'])}")
        ctl = i.get('controls', {})
        bad = [(kind, c) for kind in ('ui', 'host', 'engine') for c in ctl.get(kind, [])
               if c.get('verdict') not in ('pass', 'masked-by-noise')]
        if bad:
            lines += ['', f"Controls that fail (noise threshold {i.get('baseline', {})}):"]
            for kind, c in bad[:30]:
                where = c.get('id') or c.get('route') or f"host {c.get('address')}"
                extra = ', '.join(c.get('routes', []))
                lines.append(f"- {kind} `{where}` {c.get('verdict')} at={c.get('at', '')} {extra} {json.dumps(c.get('audio', {}))}")
        bad = [p for p in i.get('presets', []) if not p.get('pass')]
        if bad:
            lines += ['', 'Presets that fail:']
            lines += [f"- {p['name']}: failure={p['failure']} rebuilt={p['ui_rebuilt']} silent={p.get('silent')} "
                      f"audio_changed={p['audio_changed']} faults={p['faults']}" for p in bad]
        lines.append('')
    if previous:
        old, new = checks(previous), checks(run)
        lines += [f"## Diff vs {previous['stamp']}", '']
        diff = [f"- {'REGRESSED' if not new[k] else 'fixed'} {k[0]} `{k[1]}`" for k in sorted(new) if k in old and old[k] != new[k]]
        diff += [f"- new {k[0]} `{k[1]}` {'pass' if new[k] else 'FAIL'}" for k in sorted(new) if k not in old]
        lines += diff or ['- no check changed']
    return '\n'.join(lines) + '\n'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--only')
    ap.add_argument('--presets', type=int, default=8)
    ap.add_argument('--controls', type=int, default=24)
    ap.add_argument('--timeout', type=int, default=1800)
    ap.add_argument('--binary')
    args = ap.parse_args()
    exe = args.binary or build()
    stamp = time.strftime('%Y%m%d-%H%M%S')
    previous = None
    if RUNS.exists():
        prior = sorted(p for p in RUNS.iterdir() if (p / 'run.json').exists())
        previous = json.loads((prior[-1] / 'run.json').read_text()) if prior else None
    out = RUNS / stamp
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], cwd=ROOT, stdout=subprocess.PIPE, text=True).stdout.strip()
    run = {'stamp': stamp, 'commit': commit, 'instruments': []}
    for name, path in instruments():
        if args.only and args.only != name:
            continue
        run['instruments'].append(run_one(exe, name, path, out / name, args))
        (out / 'run.json').write_text(json.dumps(run, indent=1))
    (out / 'summary.md').write_text(summary(run, previous))
    print(out / 'summary.md')
    print((out / 'summary.md').read_text())


if __name__ == '__main__':
    main()
