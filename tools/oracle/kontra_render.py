#!/usr/bin/env python3
"""Render the same notes as capture.py through KONTRA (sampler-native render-kontakt-midi).

    tools/oracle/kontra_render.py LIST.tsv OUTDIR [--midi FILE | --scenario NAME]

LIST.tsv: `name<TAB>nki_path` (the same list as capture.py --batch). Writes OUTDIR/<name>.wav.
The binary is $SAMPLER_NATIVE, else the release build under the KONTRA cargo target root
(build with `build-slot cargo build --release -p sampler-native`).
"""
import argparse
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import capture  # noqa: E402  (scenario writer, same notes as the Kontakt slots)


def binary():
    env = os.environ.get("SAMPLER_NATIVE")
    if env:
        return env
    root = Path("/data/dev/workspace/Toolchains/User/cargo-target/KONTRA")
    found = sorted(root.glob("*/release/sampler-native"), key=lambda p: p.stat().st_mtime, reverse=True)
    if not found:
        sys.exit("sampler-native not built: build-slot cargo build --release -p sampler-native")
    return str(found[0])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("list")
    ap.add_argument("out_dir")
    ap.add_argument("--midi")
    ap.add_argument("--scenario", choices=sorted(capture.SCENARIOS), default="base")
    args = ap.parse_args()
    midi = Path(args.midi) if args.midi else capture.write_scenario(args.scenario)
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    rows = [l.split("\t") for l in Path(args.list).read_text().splitlines() if l.strip() and not l.startswith("#")]
    ok = 0
    for name, nki in ((r[0], r[1]) for r in rows):
        target = out / f"{name}.wav"
        p = subprocess.run([binary(), "render-kontakt-midi", nki, str(target), str(midi)],
                           capture_output=True, text=True, timeout=900)
        if p.returncode == 0 and target.exists():
            ok += 1
        else:
            print(f"{name}: render failed rc={p.returncode}: {p.stderr.strip().splitlines()[-1:]}", file=sys.stderr)
    print(f"rendered {ok}/{len(rows)} -> {out}")
    return 0 if ok == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main())
