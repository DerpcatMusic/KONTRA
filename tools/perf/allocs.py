#!/usr/bin/env python3
"""Heap allocations inside the plugin's `process`, over a whole playtest.

    tools/perf/allocs.py Dolce
    tools/perf/allocs.py ANALOG --sites

Plays the named program's playtest with allocs.c preloaded: only calls made
between the start and end of each `process` call count (the harness's own
buffers do not). Prints the allocations and frees; the audio thread should
show 0. `--sites` builds with line tables (the corpus profile) and groups
the allocations by the first KONTRA source line that asked for them.

Cargo runs as $KONTRA_CARGO (default `cargo`; here `build-slot cargo`).
Needs a C compiler (`cc`) and, for `--sites`, `addr2line`.
"""

import collections
import json
import os
import re
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def playtest_binary(*profile: str) -> str:
    cargo = shlex.split(os.environ.get("KONTRA_CARGO", "cargo"))
    build = [*cargo, "test", *profile, "-p", "kontra-plugin", "--lib", "--no-run",
             "--message-format=json"]
    out = subprocess.run(build, cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True,
                         env=dict(os.environ, CARGO_PROFILE_CORPUS_DEBUG="line-tables-only")).stdout
    for line in out.splitlines():
        msg = json.loads(line)
        if msg.get("reason") == "compiler-artifact" and msg.get("executable") \
                and msg["target"]["name"] == "kontra_plugin":
            return msg["executable"]
    sys.exit("no kontra_plugin test binary in cargo's output")


def sites(binary: str, log: str) -> collections.Counter:
    """Each allocation's first frame in KONTRA's own source."""
    stacks = [re.findall(r"\(\+(0x[0-9a-f]+)\)", s) for s in log.split("ALLOC #")[1:]]
    lines = {}
    for addr in sorted({a for s in stacks for a in s}):
        out = subprocess.run(["addr2line", "-f", "-C", "-i", "-e", binary, addr],
                             stdout=subprocess.PIPE, text=True).stdout.splitlines()
        lines[addr] = [(fn, loc.replace(f"{ROOT}/", "")) for fn, loc in zip(out[::2], out[1::2])]
    count = collections.Counter()
    for stack in stacks:
        own = (f"{fn[:60]} {loc}" for a in stack for fn, loc in lines[a]
               if loc.startswith(("crates/", "plugin/")) and "alloc" not in loc)
        count[next(own, "?")] += 1
    return count


def main() -> None:
    args = [a for a in sys.argv[1:] if a != "--sites"]
    if len(args) != 1:
        sys.exit(__doc__)
    want_sites = "--sites" in sys.argv
    binary = playtest_binary("--profile", "corpus") if want_sites \
        else playtest_binary("--release")
    with tempfile.TemporaryDirectory(dir=Path(binary).parent) as scratch:
        shim = Path(scratch) / "allocs.so"
        subprocess.run(["cc", "-O2", "-shared", "-fPIC", "-o", shim,
                        Path(__file__).with_name("allocs.c"), "-ldl"], check=True)
        env = dict(os.environ, KONTRA_PLAYTEST=args[0], KONTRA_ALLOC_MARKS="1",
                   LD_PRELOAD=str(shim), KX_ALLOC_STACKS="400" if want_sites else "0")
        run = subprocess.run([binary, "real_performances", "--ignored", "--nocapture"],
                             cwd=ROOT / "plugin", env=env, stdout=subprocess.DEVNULL,
                             stderr=subprocess.PIPE, text=True)
    total = [l.split() for l in run.stderr.splitlines() if l.startswith("ALLOCS ")]
    if not total:
        sys.exit(f"the playtest of {args[0]!r} did not finish\n{run.stderr[-2000:]}")
    _, allocs, frees = total[-1]
    print(f"{args[0]}: {allocs} allocations, {frees} frees inside process")
    if want_sites:
        for site, n in sites(binary, run.stderr).most_common():
            print(f"{n:6}  {site}")


if __name__ == "__main__":
    main()
