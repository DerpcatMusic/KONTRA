#!/usr/bin/env python3
"""Serial, resumable cells. Each library run releases the shared heavy slot."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument('engine', choices=['v1', 'v2'])
p.add_argument('binary', type=Path)
p.add_argument('out', type=Path)
p.add_argument('--cold', action='store_true')
p.add_argument('--profile', action='store_true')
p.add_argument('--frequency', type=int, choices=[499, 1999, 9999], default=499)
p.add_argument('--block', type=int, choices=[32, 64, 256])
p.add_argument('--only', choices=['piano', 'strings', 'fx', 'afflatus', 'conflux', 'conflux-multi'])
p.add_argument('--sustain', action='store_true', help='48k sampler-perf idle/held/release audit')
p.add_argument('--notes', type=int, choices=[1, 8])
p.add_argument('--seconds', type=float, default=3.0)
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=True)
root = Path('/mnt/MAIN_STORAGE/Libraries/Kontakt')
scenarios = {
    'piano': ('Una Corda Library', 'Instruments/Una Corda Pure.nki'),
    'strings': ('Performance Samples Vista', 'Instruments/Vista - 5 Violins.nki'),
    'fx': ('ANALOG STRINGS', 'Instruments/ANALOG STRINGS.nki'),
}
heavy = '/home/derpcat/.cache/kontakto-heavy'
if a.sustain:
    if a.engine != 'v2' or a.cold or a.only in scenarios or a.block == 32:
        p.error('--sustain needs v2, no --cold, and afflatus|conflux|conflux-multi')
    failed_cells = 0
    for name in ([a.only] if a.only else ['afflatus', 'conflux', 'conflux-multi']):
        for block in ([a.block] if a.block else [64, 256]):
            for notes in ([a.notes] if a.notes else [1, 8]):
                tag = f'{name}-{block}-{notes}' + ('-perf' if a.profile else '')
                done = a.out / f'{tag}.done'
                if done.exists():
                    # Resuming a failed cell must still fail the overall audit.
                    failed_cells += int(json.loads(done.read_text()).get('exit_code', 1) != 0)
                    continue
                ready = a.out / f'{tag}.ready'
                finished = a.out / f'{tag}.finished'
                env = dict(os.environ, XDG_CACHE_HOME='/dev/null', KONTRA_SCAN_ACTIVE='1',
                           KONTRA_DISABLE_NETWORK='1')
                if a.profile:
                    ready.unlink(missing_ok=True)
                    finished.unlink(missing_ok=True)
                    env.update(CPU_AUDIT_READY=str(ready), CPU_AUDIT_FINISHED=str(finished))
                command = [heavy, 'timeout', '300', str(a.binary.resolve()), 'sustain',
                           '--only', name, '--block', str(block), '--notes', str(notes),
                           '--seconds', str(a.seconds), '--out', str((a.out/f'{tag}.json').resolve())]
                with (a.out/f'{tag}.err').open('w') as err:
                    job = subprocess.Popen(command, stderr=err, env=env)
                    prof = None
                    if a.profile:
                        while not ready.exists() and job.poll() is None:
                            time.sleep(.1)
                        if ready.exists():
                            # Frame-pointer callchains contain code addresses only, never copied sample/script stack bytes.
                            prof = subprocess.Popen(['perf', 'record', '-g', '--call-graph', 'fp', '--user-callchains',
                                '-F', str(a.frequency), '-e', 'cycles:u', '-p', ready.read_text(),
                                '-o', str(a.out/f'{tag}.perf.data')], stderr=err)
                        while not finished.exists() and job.poll() is None:
                            time.sleep(.01)
                        if prof and prof.poll() is None:
                            prof.send_signal(signal.SIGINT)
                    code = job.wait()
                    if a.profile and prof is None:
                        code = code or 1
                    if prof:
                        profile_code = prof.wait()
                        with (a.out/f'{tag}.perf.txt').open('w') as report:
                            report_job = subprocess.run(['perf', 'report', '--stdio', '--no-children',
                                '--percent-limit', '0.5', '-i', str(a.out/f'{tag}.perf.data')],
                                stdout=report, check=False)
                            code = code or report_job.returncode
                        with (a.out/f'{tag}.audio.perf.txt').open('w') as report:
                            report_job = subprocess.run(['perf', 'report', '--stdio', '--no-children',
                                '--percentage', 'relative', '--tid', ready.read_text(),
                                '--percent-limit', '0.5', '-i', str(a.out/f'{tag}.perf.data')],
                                stdout=report, check=False)
                            code = code or report_job.returncode
                        if profile_code not in (0, -signal.SIGINT, 128 + signal.SIGINT):
                            code = profile_code
                done.write_text(json.dumps({'exit_code': code})+'\n')
                print(tag, code, flush=True)
                failed_cells += int(code != 0)
    raise SystemExit(int(failed_cells > 0))
for name, (library, instrument) in scenarios.items():
    if a.only and a.only != name:
        continue
    for block in ([a.block] if a.block else [64] if a.profile else [32, 64, 256]):
        tag = f'{a.engine}-{name}-{block}' + ('-cold' if a.cold else '') + ('-perf' if a.profile else '')
        done = a.out / f'{tag}.done'
        if done.exists():
            continue
        # v1's parsed-instrument cache can contain decrypted scripts/impulses.
        # ENOTDIR disables writes without changing the read-only v1 source.
        env = dict(os.environ, XDG_CACHE_HOME='/dev/null', KONTRA_DISABLE_NETWORK='1', KONTRA_REPORT_DIR=str(a.out/'reports'))
        ready = a.out / f'{tag}.ready'
        finished = a.out / f'{tag}.finished'
        if a.profile:
            ready.unlink(missing_ok=True)
            finished.unlink(missing_ok=True)
            env['CPU_AUDIT_READY'] = str(ready)
            env['CPU_AUDIT_FINISHED'] = str(finished)
        command = [heavy, 'timeout', '240', str(a.binary), str(root/library/instrument), str(block), name]
        if a.cold:
            command = [heavy, 'bash', '-c',
                       'python3 "$1" "$2" > "$3" && exec timeout 240 "$4" "$5" "$6" "$7"',
                       'cpu-cold', str(Path(__file__).with_name('cpu-audit-cold.py')), str(root/library),
                       str(a.out/f'{tag}.cache.json'), str(a.binary), str(root/library/instrument), str(block), name]
        with (a.out/f'{tag}.json').open('w') as out, (a.out/f'{tag}.err').open('w') as err:
            job = subprocess.Popen(command, stdout=out, stderr=err, env=env)
            prof = None
            if a.profile:
                while not ready.exists() and job.poll() is None:
                    time.sleep(.1)
                if ready.exists():
                    # Leaf IPs only: no copied user stack or decoded sample bytes.
                    prof = subprocess.Popen(['perf', 'record', '-F', str(a.frequency), '-e', 'cycles:u', '-p', ready.read_text(), '-o', str(a.out/f'{tag}.perf.data')], stdout=subprocess.DEVNULL, stderr=err)
                while not finished.exists() and job.poll() is None:
                    time.sleep(.01)
                if prof and prof.poll() is None:
                    prof.send_signal(signal.SIGINT)
            code = job.wait()
            if prof:
                prof.wait()
                with (a.out/f'{tag}.perf.txt').open('w') as report:
                    subprocess.run(['perf', 'report', '--stdio', '--no-children', '--percent-limit', '1', '-i', str(a.out/f'{tag}.perf.data')], stdout=report, check=True)
                with (a.out/f'{tag}.audio.perf.txt').open('w') as report:
                    subprocess.run(['perf', 'report', '--stdio', '--no-children', '--percentage', 'relative', '--percent-limit', '1', '--tid', ready.read_text(), '-i', str(a.out/f'{tag}.perf.data')], stdout=report, check=True)
        done.write_text(json.dumps({'exit_code': code})+'\n')
        print(tag, code, flush=True)
