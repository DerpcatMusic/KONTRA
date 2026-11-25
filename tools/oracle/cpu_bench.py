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

The audio thread alone (`audio_thread_*`): the session waits until Kontakt's disk reads go quiet
(preload and DFD buffers filled, `settle_s`), samples every thread over an idle window with no notes,
then over the playing window; the audio thread is the one whose CPU rises most from idle to playing.
`audio_thread_net_pct` = its playing CPU minus its idle CPU (the render cost of the notes); `threads`
lists the threads that rose most, as (name, playing %, idle %). Whole-process figures under Wine also
carry the GUI, Xvfb, disk and Wine service threads, so compare KONTRA's audio thread with this one.
Kontakt renders at its Settings.cfg block size (AB2 AudioBlockSize, 1024 frames) and 48 kHz.
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


SOUNDING = 1e-4  # a session that peaks below this recorded no notes


def recorded_peak(wav):
    """Peak magnitude of the session's float WAV (0 when it is missing)."""
    try:
        data = Path(wav).read_bytes()
    except (OSError, TypeError):
        return 0.0
    at = data.find(b"data")
    body = data[at + 8:] if at >= 0 else b""
    values = memoryview(body[: len(body) // 4 * 4]).cast("f")
    return max((abs(v) for v in values), default=0.0)


def audio_thread(cpu):
    """The thread whose CPU rose most from the idle window to the playing window, and the rises."""
    idle_s, before, after = cpu["idle"]
    idle = {t: (after[t][1] - before[t][1]) / CLK_TCK / idle_s for t in after if t in before}
    start = cpu["start"]
    inside = [(t, th) for t, th in cpu["threads"] if start <= t <= start + SECONDS]
    if len(inside) < 2:
        sys.exit("cpu_bench: fewer than two thread samples inside the window")
    (t0, s0), (t1, s1) = inside[0], inside[-1]
    play = {t: (s1[t][1] - s0[t][1]) / CLK_TCK / (t1 - t0) for t in s1 if t in s0}
    rise = sorted(play, key=lambda t: play[t] - idle.get(t, 0.0), reverse=True)
    top = rise[0]
    return {"audio_thread": s1[top][0], "audio_thread_pct": play[top] * 100,
            "audio_thread_idle_pct": idle.get(top, 0.0) * 100,
            "audio_thread_net_pct": (play[top] - idle.get(top, 0.0)) * 100, "settle_s": cpu["settle_s"],
            "threads": [(s1[t][0], round(play[t] * 100, 1), round(idle.get(t, 0.0) * 100, 1)) for t in rise[:6]]}


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
    wav, _, loaded = capture.run_session(nkis, SECONDS, work, midi=midi, cpu=cpu)
    if args.nki and not loaded.get(0, (False, ""))[0]:
        sys.exit(f"cpu_bench: instrument did not load: {loaded}")
    # A session whose notes never reached the output measures an idle Kontakt, not its render.
    if args.nki and recorded_peak(wav) < SOUNDING:
        sys.exit(f"cpu_bench: the session recorded silence (MIDI or audio routing failed); see {work}")
    pct, wall = cpu_percent(cpu)
    result = {"name": args.name, "nki": args.nki, "seconds": SECONDS, "wall_s": wall, "kontakt_cpu_pct": pct,
              **audio_thread(cpu)}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=1))
    print(json.dumps(result))


if __name__ == "__main__":
    main()
