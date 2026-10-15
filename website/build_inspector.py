#!/usr/bin/env python3
"""Compile the shared library inspector for browsers (Emscripten 6.0.12)."""
import argparse
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
TARGET = 'wasm32-unknown-emscripten'


def build(output, profile='ci'):
    if not shutil.which('emcc'):
        raise SystemExit('Emscripten is required. Activate emsdk 6.0.12 and install the Rust wasm32-unknown-emscripten target.')
    output.mkdir(parents=True, exist_ok=True)
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    env = dict(os.environ, KONTRA_WEB_REVISION=revision)
    flags = [
        '-sMODULARIZE=1', '-sEXPORT_ES6=1', '-sEXPORT_NAME=createKontraInspector', '-sDEFAULT_TO_CXX=1',
        '-sENVIRONMENT=worker,node', '-sALLOW_MEMORY_GROWTH=1',
        '-sINITIAL_MEMORY=67108864', '-sMAXIMUM_MEMORY=1073741824',
        '-sSTACK_SIZE=8388608', '-sEXIT_RUNTIME=0', '-sDYNAMIC_EXECUTION=0',
        '-sEXPORTED_RUNTIME_METHODS=FS,WORKERFS,callMain,ENV', '-lworkerfs.js',
    ]
    command = ['cargo', 'rustc', '--locked', '-j', '4', '--profile', profile,
               '-p', 'corpus-health', '--bin', 'browser-inspector', '--target', TARGET, '--']
    for flag in flags:
        command += ['-C', f'link-arg={flag}']
    with tempfile.TemporaryDirectory(prefix='kontra-web-build-', dir=output.parent) as scratch:
        env['TMPDIR'] = scratch
        subprocess.run(command, cwd=ROOT, env=env, check=True)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT, text=True))
    artifacts = Path(metadata['target_directory']) / TARGET / ('debug' if profile == 'dev' else profile)
    shutil.copy2(artifacts / 'browser-inspector.js', output / 'inspector-engine.mjs')
    shutil.copy2(artifacts / 'browser-inspector.wasm', output / 'browser-inspector.wasm')
    (output / 'inspector-build.json').write_text(json.dumps({'revision': revision, 'target': TARGET, 'emscripten': '6.0.12'}) + '\n')
    print(f'Built browser inspector at {revision[:7]} into {output}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--profile', default='ci', choices=['ci', 'dev'])
    args = parser.parse_args()
    build(args.output.resolve(), args.profile)
