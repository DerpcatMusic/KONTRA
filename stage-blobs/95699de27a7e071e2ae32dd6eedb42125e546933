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
p.add_argument('--block', type=int, choices=[32, 64, 256])
p.add_argument('--only', choices=['piano', 'strings', 'fx'])
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=True)
root = Path('/mnt/MAIN_STORAGE/Libraries/Kontakt')
scenarios = {
    'piano': ('Una Corda Library', 'Instruments/Una Corda Pure.nki'),
    'strings': ('Performance Samples Vista', 'Instruments/Vista - 5 Violins.nki'),
    'fx': ('ANALOG STRINGS', 'Instruments/ANALOG STRINGS.nki'),
}
heavy = '/home/derpcat/.cache/kontakto-heavy'
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
                    prof = subprocess.Popen(['perf', 'record', '-F', '499', '-e', 'cycles:u', '-p', ready.read_text(), '-o', str(a.out/f'{tag}.perf.data')], stdout=subprocess.DEVNULL, stderr=err)
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
