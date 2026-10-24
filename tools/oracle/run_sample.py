#!/usr/bin/env python3
"""Representative Kontakt-vs-KONTRA sample: scenarios from the instruments, one capture, one report.

    tools/oracle/run_sample.py LIST.tsv OUT_DIR [--window S] [--md ORACLE_DIFF.md] [--json ORACLE_DIFF.json]

LIST.tsv: `name<TAB>nki_path` per line. Steps:
  1. scenario.py: KONTRA scans keys 24..96 of every instrument; the sounding keys make its notes.
     Instruments with no sounding key are reported with the reason and are not sent to Kontakt.
  2. capture.py: the notes go to real Kontakt, 15 instruments per session plus a canary (a known
     audible instrument in slot 0). A silent canary aborts the run: audio or MIDI is broken.
  3. kontra_render.py's render-kontakt-midi: the same notes per instrument through KONTRA.
  4. compare.py: level, timing, timbre, waveform r per instrument, silent reasons, per library.
Kontakt runs only under `flock /tmp/kontra-oracle.lock` (shared with every other oracle lane).
"""
import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import capture  # noqa: E402
import compare  # noqa: E402
import kontra_render  # noqa: E402
import scenario  # noqa: E402

CANARY_NKI = ("/data/main/Libraries/Kontakt/Areia 1.2.0 [Audio Imperia]/Instruments/"
              "02 Legato Technique Patches/Areia - 04 4 Double Basses - Performance Legato.nki")


def read_list(path):
    rows = [l.split("\t") for l in Path(path).read_text().splitlines() if l.strip() and not l.startswith("#")]
    return [(r[0].strip(), r[1].strip()) for r in rows]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("list")
    ap.add_argument("out_dir")
    ap.add_argument("--window", type=float, default=6.0)
    ap.add_argument("--md")
    ap.add_argument("--json")
    args = ap.parse_args()
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    rows = read_list(args.list)

    canary = scenario.derive(CANARY_NKI)
    if not canary["notes"]:
        sys.exit(f"canary has no sounding key on KONTRA: {canary['reason']}")
    print(f"canary notes {canary['notes']}", file=sys.stderr)

    derived = {}
    for i, (name, nki) in enumerate(rows, 1):
        derived[name] = scenario.derive(nki)
        print(f"[{i}/{len(rows)}] {name}: {derived[name]['notes'] or derived[name]['reason']}", file=sys.stderr)
    (out / "scenario.json").write_text(json.dumps({n: dict(d, library=compare.library_of(d["nki"]))
                                                   for n, d in derived.items()}, indent=1))

    playable = [(n, d["nki"], d["notes"]) for n, d in derived.items() if d["notes"]]
    cap_dir = out / "capture"
    manifest = {}
    if playable:
        manifest = capture.capture(playable, cap_dir, args.window, canary=(capture.CANARY_NAME, CANARY_NKI, canary["notes"]))
        (cap_dir / "capture.json").write_text(json.dumps(manifest, indent=1))
    shutil.copy(out / "scenario.json", cap_dir / "scenario.json")

    kdir = out / "kontra"
    kdir.mkdir(exist_ok=True)
    binary = kontra_render.binary()
    for name, nki, notes in playable:
        midi = out / "midi" / f"{name}.mid"
        midi.parent.mkdir(exist_ok=True)
        capture.make_midi(midi, 1, notes=[tuple(x) for x in notes])
        target = kdir / f"{name}.wav"
        p = subprocess.run(kontra_render.capped([binary, "render-kontakt-midi", nki, str(target), str(midi)]),
                           capture_output=True, text=True, timeout=900)
        if p.returncode != 0 or not target.exists():
            derived[name]["reason"] = "KONTRA render failed: " + (p.stderr.strip().splitlines() or ["?"])[-1]
            derived[name]["notes"] = None
    (cap_dir / "scenario.json").write_text(json.dumps({n: dict(d, library=compare.library_of(d["nki"]))
                                                       for n, d in derived.items()}, indent=1))

    argv = [str(cap_dir), str(kdir), "--window", str(args.window)]
    if args.md:
        argv += ["--md", args.md]
    if args.json:
        argv += ["--json", args.json]
    sys.argv = ["compare.py"] + argv
    return compare.main()


if __name__ == "__main__":
    sys.exit(main())
