#!/usr/bin/env python3
"""First five minutes with a library: what a user hits on the first load.

Picks libraries from the corpus (seeded, so a rerun picks the same ones), one
instrument or preset from each, and runs the plugin-path check on it:
corpus-health's full tier, which streams the load as the plugin does and renders
a note.

Checklist:
  load       the item loads and renders a finite note              (automated)
  keyswitch  every keyswitch the script declares is recognised     (automated)
  cc         controller needs are reported, not refused            (automated)
  UI visible the load report and rack row show the instrument      (not automated)
  release    no voice keeps sounding after note-off                (automated: stuck-voice tail)
  persist    state recalled from a saved snapshot                  (not automated)

Usage: tools/first-five-minutes.py [--seed N] [--count N] [--out DIR] [--workers N]
"""

import argparse
import json
import pathlib
import random
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
RUN = HERE / "corpus-health" / "run.sh"
CORPUS = [pathlib.Path("/data/main/Libraries/Kontakt"), pathlib.Path("/data/main/Libraries/UVI")]


def libraries():
    return sorted(p for root in CORPUS if root.is_dir() for p in root.iterdir() if p.is_dir())


def pick_item(lib, rng):
    """One loadable item of a library: an NKI instrument, or a UVI bank's preset."""
    if "UVI" in lib.parts:
        banks = sorted(lib.rglob("*.ufs"))
        if not banks:
            return None
        bank = rng.choice(banks)
        listed = subprocess.run([str(RUN), "list", str(bank)], capture_output=True, text=True).stdout
        ids = [line.split("\t", 1)[1] for line in listed.splitlines() if "\t" in line]
        return (bank, rng.choice(ids)) if ids else None
    nkis = sorted(lib.rglob("*.nki"))
    if not nkis:
        return None
    nki = rng.choice(nkis)
    return (nki, str(nki))


def checklist(record):
    """The automated checks on one full-tier record. Each is a list of failure strings."""
    failures = []
    load = record.get("load") or {}
    stage = record.get("stage")
    sound = record.get("sound") or {}
    if stage == "release":
        tail = sound.get("tail_peak_db")
        failures.append(
            f"release: {sound.get('stuck_voices')} voice(s) still sounding "
            f"{tail:.1f} dB after note-off (stored release {record.get('stored_release_s')} s)"
        )
    elif stage not in ("ok", "needs-controller") or load.get("ok") is False:
        failures.append(f"load: {stage}: {load.get('error') or sound.get('error')}")
    if sound.get("finite") is False:
        failures.append("load: rendered non-finite audio")
    unrecognised = (record.get("articulation") or {}).get("unrecognised") or []
    if unrecognised:
        failures.append(f"keyswitch: unrecognised {unrecognised}")
    if stage == "needs-controller":
        failures.append("cc: refused until a controller is set (stage needs-controller)")
    return failures


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seed", type=int, default=20261024)
    ap.add_argument("--count", type=int, default=10)
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--out", type=pathlib.Path, default=pathlib.Path.home() / ".cache/kontra-first-five")
    args = ap.parse_args()

    rng = random.Random(args.seed)
    libs = libraries()
    rng.shuffle(libs)
    picks = []
    for lib in libs:
        if len(picks) == args.count:
            break
        item = pick_item(lib, rng)
        if item:
            picks.append((lib, item))
    if len(picks) < args.count:
        print(f"only {len(picks)} libraries had a loadable item", file=sys.stderr)

    args.out.mkdir(parents=True, exist_ok=True)
    out = args.out / f"seed-{args.seed}.jsonl"
    roots, only = [], []
    for _, (path, item) in picks:
        roots.append(str(path))
        only.append(item)
    cmd = [str(RUN), "run", str(out), "--tier", "full", "--workers", str(args.workers)]
    for pattern in only:
        cmd += ["--only", pattern]
    cmd += roots
    subprocess.run(["nice", "-n", "19", *cmd], check=False)

    records = [json.loads(line) for line in out.read_text().splitlines() if line.strip()]
    records = {r["id"]: r for r in records if "stage" in r}
    failed = 0
    for lib, (path, item) in picks:
        record = records.get(item)
        if record is None:
            print(f"{lib.name}: not run")
            failed += 1
            continue
        problems = checklist(record)
        failed += bool(problems)
        status = "PASS" if not problems else "FAIL"
        print(f"{status} {lib.name} :: {item.removeprefix(str(path.parent) + '/')}")
        for problem in problems:
            print(f"     {problem}")
    print(f"{len(picks) - failed}/{len(picks)} passed the automated checks; UI and persistence are not automated here.")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
