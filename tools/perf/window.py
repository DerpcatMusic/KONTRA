#!/usr/bin/env python3
"""Instructions and cycles per 512-frame block over one playtest segment.

    tools/perf/window.py Dolce "8-note chord"

Builds the plugin's playtest (release), plays the named program, and has
`perf` count user-space instructions and cycles only while the segment
plays (plugin/src/playtest/window.rs opens and closes the window), so load
time stays out. Plays twice: `perf stat` for the exact totals, then sampled
to split them between the audio thread (the playtest thread runs the
plugin's process) and the streaming thread. Instructions repeat to ~0.1%;
cycles move with machine load, so instructions are the number of record.

Cargo runs as $KONTRA_CARGO (default `cargo`; here `build-slot cargo`).
Needs `perf` with user-space counters (perf_event_paranoid <= 2).
"""

import json
import os
import shlex
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EVENTS = ("instructions:u", "cycles:u")
# perf names a thread by its first 15 bytes.
PERIOD = 1_000_000
THREADS = (("audio", "playtest::real_"), ("streaming", "kontra-stream"))


def playtest_binary() -> str:
    cargo = shlex.split(os.environ.get("KONTRA_CARGO", "cargo"))
    build = [*cargo, "test", "--release", "-p", "kontra-plugin", "--lib", "--no-run",
             "--message-format=json"]
    out = subprocess.run(build, cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True).stdout
    for line in out.splitlines():
        msg = json.loads(line)
        if msg.get("reason") == "compiler-artifact" and msg.get("executable") \
                and msg["target"]["name"] == "kontra_plugin":
            return msg["executable"]
    sys.exit("no kontra_plugin test binary in cargo's output")


def main() -> None:
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    program = sys.argv[1]
    segment = sys.argv[2] if len(sys.argv) == 3 else "8-note chord"
    binary = playtest_binary()
    # The FIFOs sit beside the binary, inside the build's own directory.
    with tempfile.TemporaryDirectory(dir=Path(binary).parent) as scratch:
        ctl, ack, counts, data = (Path(scratch) / n for n in ("ctl", "ack", "counts", "perf.data"))
        os.mkfifo(ctl)
        os.mkfifo(ack)
        env = dict(os.environ, KONTRA_PLAYTEST=program, KONTRA_PERF_SEGMENT=segment,
                   KONTRA_PERF_CTL=str(ctl), KONTRA_PERF_ACK=str(ack))
        def play(*perf: str) -> tuple[int, str]:
            """The playtest under `perf ...`, counting only the window."""
            cmd = ["perf", *perf, "-D", "-1", "--control", f"fifo:{ctl},{ack}",
                   "-e", ",".join(EVENTS), binary, "real_performances", "--ignored",
                   "--nocapture"]
            run = subprocess.run(cmd, cwd=ROOT / "plugin", env=env,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
            window = [l for l in run.stderr.splitlines() if l.startswith("PERFWIN ")]
            if not window:
                sys.exit(f"segment {segment!r} of {program!r} never played\n{run.stderr[-2000:]}")
            name, _, blocks = window[0].removeprefix("PERFWIN ").rpartition("|")
            return int(blocks), name
        # The exact counts of the window, every thread together.
        blocks, name = play("stat", "-x,", "-o", str(counts))
        exact = {}
        for row in counts.read_text().splitlines():
            cells = row.split(",")
            if len(cells) > 2 and cells[0].isdigit():
                exact[cells[2]] = int(cells[0])
        # Each thread's share, from fixed-period samples (`perf stat` cannot
        # split a launched command's threads; samples undercount ~4%, so they
        # only split the exact totals).
        play("record", "-q", "-c", str(PERIOD), "-o", str(data))
        samples = subprocess.run(["perf", "script", "-i", str(data), "-F", "comm,event"],
                                 check=True, stdout=subprocess.PIPE, text=True).stdout
        shares = defaultdict(lambda: defaultdict(int))
        for line in samples.splitlines():
            thread, _, event = line.strip().rpartition(" ")
            kind = next((k for k, prefix in THREADS if thread.strip().startswith(prefix)), "other")
            shares[event.rstrip(":")][kind] += 1
    print(f"{name} | {blocks} blocks")
    print(f"{'thread':<10} {'instructions/block':>19} {'cycles/block':>13}")
    for kind in ("audio", "streaming", "other", "all"):
        row = []
        for e in EVENTS:
            seen = sum(shares[e].values()) or 1
            part = 1.0 if kind == "all" else shares[e][kind] / seen
            row.append(exact.get(e, 0) * part / blocks)
        if kind == "all" or any(shares[e][kind] for e in EVENTS):
            print(f"{kind:<10} {row[0]:>19,.0f} {row[1]:>13,.0f}")


if __name__ == "__main__":
    main()
