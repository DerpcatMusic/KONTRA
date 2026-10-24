#!/usr/bin/env python3
"""CPU of real Kontakt playing the shared progression (the Kontakt side of the KONTRA cpu-* scenarios).

    tools/oracle/oracle-slot tools/oracle/cpu_bench.py --nki NKI --name NAME --out OUT.json   one instrument
    tools/oracle/oracle-slot tools/oracle/cpu_bench.py --empty --out OUT.json                 idle baseline

Plays the same MIDI as sampler-perf's `progression16` (I-vi-IV-V, 16 voices per chord, 2 s per chord,
CC1=110 and CC11=127 at 0 s) for SECONDS (8) and reads utime+stime of the Kontakt process of this slot
(/proc/<pid>/stat, threads included) between the MIDI start and start+SECONDS. CPU% = busy seconds /
wall seconds * 100, so 100 means one full core. The idle baseline is the same session with an empty rack
(no instrument loaded), so the instrument's own cost is kontakt_pct - idle_pct.

Caveats: Kontakt is not pinned to one thread, so its figure can exceed 100 while KONTRA's single-thread
figure is capped at 100. The window is measured from MIDI start, so load time is not counted.
"""
import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import capture  # noqa: E402  (session driver: Xvfb, Kontakt, sink routing, MIDI)
import slot  # noqa: E402

SECONDS = 8.0
# Same as sampler-perf progression16 (crates/sampler-perf/src/main.rs).
ROOTS = [36, 33, 41, 43]
VOICING = [0, 4, 7, 12, 16, 19, 24, 28, 31, 34, 36, 40, 43, 46, 48, 52]
NOTES = [(chord * 2.0, 2.0, root + v, 100) for chord, root in enumerate(ROOTS) for v in VOICING]
CC = [(0.0, 1, 110), (0.0, 11, 127)]
CLK_TCK = os.sysconf("SC_CLK_TCK")


def cpu_percent(cpu):
    """Busy share of one core over [start, start + SECONDS], from the first and last sample inside it."""
    start = cpu["start"]
    inside = [(t, ticks) for t, ticks in cpu["samples"] if start <= t <= start + SECONDS]
    if len(inside) < 2:
        sys.exit("cpu_bench: fewer than two samples inside the window")
    (t0, k0), (t1, k1) = inside[0], inside[-1]
    busy = (k1 - k0) / CLK_TCK
    return busy / (t1 - t0) * 100, t1 - t0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--nki", help="instrument to load")
    ap.add_argument("--empty", action="store_true", help="no instrument: the idle baseline")
    ap.add_argument("--name", default="idle")
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()
    if bool(args.nki) == args.empty:
        ap.error("give exactly one of --nki or --empty")
    work = slot.work(Path.home() / ".cache/kontra-cpu-bench") / args.name
    work.mkdir(parents=True, exist_ok=True)
    midi = work / "progression16.mid"
    capture.make_midi(midi, 1, notes=NOTES, cc=CC)
    cpu = {}
    nkis = [args.nki] if args.nki else []
    _, _, loaded = capture.run_session(nkis, SECONDS, work, midi=midi, cpu=cpu)
    if args.nki and not loaded.get(0, (False, ""))[0]:
        sys.exit(f"cpu_bench: instrument did not load: {loaded}")
    pct, wall = cpu_percent(cpu)
    result = {"name": args.name, "nki": args.nki, "seconds": SECONDS, "wall_s": wall, "kontakt_cpu_pct": pct}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=1))
    print(json.dumps(result))


if __name__ == "__main__":
    main()
