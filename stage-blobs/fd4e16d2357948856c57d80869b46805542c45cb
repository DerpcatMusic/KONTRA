#!/usr/bin/env python3
"""Prepare the pinned v1 adapter from the ONE shared scanner source bundle."""
from pathlib import Path
import shutil
import subprocess
import sys

source=Path(__file__).resolve().parent
repo=source.parents[1]
destination=Path(sys.argv[1]).resolve()
subprocess.run(['git','-C',str(repo),'worktree','add','--detach',str(destination),'0cb7a8a0'],check=True)
subprocess.run(['git','-C',str(destination),'apply',str(source/'v1-instrumentation.patch')],check=True)
shutil.copytree(source,destination/'tools/kontra-scan',ignore=shutil.ignore_patterns('__pycache__','v1-instrumentation.patch'))
metrics=destination/'tools/kontra-scan/metrics.rs'
s=metrics.read_text();needle='    #[test]\n    fn scanner_observes_compiler_and_lua_phases'
assert needle in s
# The v2 host phase check refers to crates absent from pinned v1; v1 has its own VM-yield check.
metrics.write_text(s.replace(needle,'    #[cfg(any())]\n    fn scanner_observes_compiler_and_lua_phases'))
print(f'Prepared {destination}. Build via ~/.cache/kontakto-heavy cargo build --release --example kontra_scan --features shots')
