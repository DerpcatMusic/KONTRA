#!/usr/bin/env python3
"""Run resumable <=290-second shards. Outputs contain derived metadata only."""
import json, pathlib, subprocess, sys
work = pathlib.Path(__file__).resolve().parents[2]
cache = pathlib.Path.home() / '.cache/kontakto-audit-ksp'
cache.mkdir(exist_ok=True)
root = pathlib.Path('/mnt/MAIN_STORAGE/Libraries/Kontakt')
master = {line.split('\t')[0] for line in (pathlib.Path.home()/'.cache/kontakto-gpt-format-gaps/census/files.tsv').read_text().splitlines()[1:]}
files = sorted(str(p) for p in root.rglob('*') if p.suffix.lower() in ('.nki','.nkm','.nkb'))
manifest = cache/'files.tsv'
manifest.write_text('path\tstatus\n'+''.join(p+'\tok\n' for p in files))
binary = '/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/kontakto-audit-ksp/ci/examples/ksp_audit'
v1 = '/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/ci/examples/ksp_audit_bridge'
for start in range(0,len(files),100):
    out = cache/f'shard-{start:04}.jsonl'
    if out.exists() and len(out.read_text().splitlines()) == min(100,len(files)-start): continue
    with out.open('w') as f:
        rc = subprocess.run([str(pathlib.Path.home()/'.cache/kontakto-heavy'),'timeout','290',binary,str(manifest),str(start),'100',v1],cwd=work,stdout=f).returncode
    print(f'{start}/{len(files)} rc={rc}',flush=True)
    if rc: sys.exit(rc)
