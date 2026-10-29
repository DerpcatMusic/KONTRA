#!/usr/bin/env python3
"""Run and tabulate the seven registered Kontakt user programs."""
import argparse
import concurrent.futures
import importlib.util
import json
import math
import os
from pathlib import Path
import subprocess
import sys
from datetime import datetime, timezone

import numpy as np

REPO = Path(__file__).resolve().parents[2]
ORACLE = REPO / 'tools/oracle'
sys.path.insert(0, str(ORACLE))
LIBRARIES = {
    'afflatus': 'Afflatus Chapter II Brass/Instruments/2. Solos/Single Instruments/Solo French Horn/Solo French Horn Legato.nki',
    'pacific': 'Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki',
    'chorus': 'Audio Imperia CHORUS/Instruments/02 Single Patches/03 Ensemble - Traditional Articulations/Chorus - 02 Ensemble - Legato Ooh.nki',
    'areia': 'Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki',
    'dolce': 'Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 01 7 1st Violins - Legato.nki',
    'una': 'Una Corda Library/Instruments/Una Corda Cotton.nki',
    'analog': 'ANALOG STRINGS/Instruments/ANALOG STRINGS.nki',
}
SEGMENTS = ('legato slow', 'legato fast', 'chord slow', 'chord fast', 'repeat slow', 'repeat fast')
CENTERS = (31.25, 62.5, 125, 250, 500, 1000, 2000, 4000, 8000, 16000)


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    obj = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(obj)
    return obj


compare = module('kontra_compare', ORACLE / 'compare.py')
helper = module('inspect_compare', ORACLE / 'inspect_compare.py')


def execute(binary, library_root, run_dir, item):
    name, relative = item
    env = dict(os.environ, KONTRA_ORACLE_DIR=str(ORACLE),
               KONTRA_INSPECT_ORACLE_DIR=str(run_dir / 'captures'))
    for mode in ('continuous', 'fresh'):
        dest = run_dir / mode
        dest.mkdir(parents=True, exist_ok=True)
        args = ['nice', '-n', '19', str(binary), 'play', str(library_root / relative),
                '--scenario', 'user', '--oracle', '--json', str(dest / (name + '.json'))]
        if mode == 'fresh':
            args.append('--fresh-state')
        with (dest / (name + '.log')).open('w') as log:
            result = subprocess.run(args, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT, check=False)
        print(f'{name} {mode}: exit {result.returncode}', flush=True)


def delta(a, b):
    return None if a is None or b is None else a - b


def db(x):
    return None if x <= 1e-12 else 20 * math.log10(x)


def formatted(x):
    return '—' if x is None else f'{x:+.1f}'


def flag(values):
    known = [x for x in values if x is not None]
    if any(abs(x) > 1 for x in known):
        return '>1 dB'
    return '—' if len(known) == len(values) else 'unknown'


def read_report(path):
    try:
        with path.open() as f:
            return json.load(f)['result'], None
    except (OSError, ValueError, KeyError) as e:
        return None, str(e)


def continuous_rows(result):
    oracle = result.get('oracle') or {}
    if not result.get('render_complete') or oracle.get('status') != 'ok':
        raise ValueError(oracle.get('reason', 'incomplete or unavailable capture'))
    artifact = Path(oracle['artifacts'])
    native = compare.load(artifact / 'native.wav')[:, int(compare.PRE * compare.RATE):]
    ours = compare.load(artifact / 'v3.wav')
    lag = round(oracle['comparison']['env_lag_ms'] * compare.RATE / 1000)
    for index, name in enumerate(SEGMENTS):
        lo = index * 2 * compare.RATE
        hi = lo + 2 * compare.RATE
        # Match the corresponding v3 interval when lag is negative too.
        begin = max(lo, -lag)
        end = min(hi, native.shape[1] - lag, ours.shape[1])
        if end <= begin:
            yield name, None, None, [None] * len(CENTERS), 'insufficient audio'
            continue
        a, b = native[:, begin + lag:end + lag], ours[:, begin:end]
        rms = delta(db(float(np.sqrt(np.mean(a*a)))), db(float(np.sqrt(np.mean(b*b)))))
        peak = delta(db(float(np.max(np.abs(a)))), db(float(np.max(np.abs(b)))))
        bands = [delta(x, y) for x, y in zip(helper.bands(a, compare.RATE), helper.bands(b, compare.RATE))]
        yield name, rms, peak, bands, None


def fresh_rows(result):
    segments = result.get('segments', [])
    by_name = {s.get('result', {}).get('scenario'): s.get('result', {}) for s in segments}
    for name in SEGMENTS:
        r = by_name.get(name, {})
        o = r.get('oracle') or {}
        if not r.get('render_complete') or o.get('status') != 'ok':
            yield name, None, None, [None]*len(CENTERS), o.get('reason', 'incomplete or unavailable capture')
            continue
        cmp = o.get('comparison', {})
        bands = o.get('native_minus_v3', {}).get('octave_band_rms_db', [])
        if len(bands) != len(CENTERS):
            yield name, None, None, [None]*len(CENTERS), 'invalid band data'
            continue
        yield name, cmp.get('level_diff_db'), cmp.get('peak_diff_db'), bands, None


def report(run_dir, output, revision):
    lines = ['# Seven-library oracle differences', '',
             f'Run revision: `{revision}`. Native minus v3 in dB; >1 means absolute difference exceeds 1 dB. '
             'Continuous segments use one performance and global envelope alignment; fresh segments use separate '
             'engine/oracle sessions and first-onset alignment (10 ms bins), comparing two seconds after each onset. '
             'Quiet-band differences may be large even when inaudible. Native internal stages are unavailable.', '']
    failures = []
    for mode, method in (('continuous', continuous_rows), ('fresh', fresh_rows)):
        lines += [f'## {mode.capitalize()}', '',
                  '| Library | Segment | RMS Δ | RMS flag | Peak Δ | Band Δ (31–16000 Hz) | Band flag | JSON |',
                  '|---|---|---:|---|---:|---|---|---|']
        for name in LIBRARIES:
            path = run_dir / mode / (name + '.json')
            result, error = read_report(path)
            if error:
                failures.append(f'{name} {mode}: {error}')
                lines.append(f'| {name} | unavailable | — | unknown | — | — | unknown | {error.replace("|", "/")} |')
                continue
            try:
                rows = list(method(result))
            except (KeyError, OSError, ValueError, AssertionError) as e:
                failures.append(f'{name} {mode}: {e}')
                lines.append(f'| {name} | unavailable | — | unknown | — | — | unknown | [{name}.json]({path}) |')
                continue
            for segment, rms, peak, bands, err in rows:
                if err:
                    failures.append(f'{name} {mode} {segment}: {err}')
                notes = f' ({err.replace("|", "/")})' if err else ''
                lines.append(f'| {name} | {segment}{notes} | {formatted(rms)} | {flag([rms])} | {formatted(peak)} | {", ".join(map(formatted,bands))} | {flag(bands)} | [{name}.json]({path}) |')
        lines.append('')
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text('\n'.join(lines) + '\n')
    print(f'wrote {output}; {len(failures)} unavailable segment(s)', flush=True)
    for error in failures:
        print(error, file=sys.stderr)
    return bool(failures)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reuse', type=Path, help='tabulate an existing run directory without rendering')
    parser.add_argument('--output', type=Path, default=Path('/data/dev/workspace/scratch/laneG/ORACLE-DIFF.md'))
    parser.add_argument('--library-root', type=Path, default=Path('/data/main/Libraries/Kontakt'))
    parser.add_argument('--jobs', type=int, default=3)
    args = parser.parse_args()
    if args.jobs < 1 or args.jobs > 3:
        parser.error('--jobs must be between 1 and 3 (oracle slots)')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip()
    dirty = subprocess.check_output(['git', 'status', '--porcelain'], cwd=REPO, text=True).strip()
    if dirty:
        revision += ' (worktree has uncommitted changes)'
    if args.reuse:
        run_dir = args.reuse.resolve()
        revision_file = run_dir / 'revision.txt'
        revision = revision_file.read_text().strip() if revision_file.exists() else 'existing reports: revision not inferred'
    else:
        env = dict(os.environ, BUILD_SLOTS=os.environ.get('BUILD_SLOTS', '6'))
        subprocess.run(['build-slot', 'cargo', 'build', '--release', '-p', 'kontra-inspect'], cwd=REPO, env=env, check=True)
        target = subprocess.check_output(['build-slot', 'sh', '-c', 'printf %s "$CARGO_TARGET_DIR"'], cwd=REPO, env=env, text=True)
        binary = Path(target) / 'release/kontra-inspect'
        stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
        run_dir = args.output.parent / ('oracle-diff-' + stamp)
        run_dir.mkdir(parents=True, exist_ok=False)
        (run_dir / 'revision.txt').write_text(revision + '\n')
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            list(pool.map(lambda item: execute(binary, args.library_root, run_dir, item), LIBRARIES.items()))
    return int(report(run_dir, args.output, revision))


if __name__ == '__main__':
    raise SystemExit(main())
