#!/usr/bin/env python3
"""Per-instrument note scenarios that the instrument actually plays.

    tools/oracle/scenario.py NKI [NKI ...] [--json OUT.json]

A fixed scenario (keys 60/64/67) silences instruments whose notes sit outside their key ranges
(Areia legato articulations play 24..48 and their script gates everything else). So each instrument
is scanned first: one KONTRA render plays every key 24..96 in turn, and the keys that sound become
the scenario (three notes, quartiles of the playable keys, the usual velocities and timing). The
scan runs KONTRA only, never Kontakt. An instrument with no sounding key gets no scenario and a
reason; that is a finding, not a silent pass.
"""
import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import capture  # noqa: E402
import compare  # noqa: E402  (RATE, load)
import kontra_render  # noqa: E402  (binary lookup)

SCAN_KEYS = list(range(24, 97))
SCAN_START = 0.2
SCAN_SPACING = 0.6
SCAN_LEN = 0.4
SCAN_VEL = 100
PLAY_DB = -50.0  # a scanned key sounds when its window peaks above this
CHOSEN_VELS = (100, 90, 110)
CHOSEN_TIMES = ((0.2, 1.0), (1.6, 1.0), (3.0, 1.5))  # (start, length), as capture.SCENARIO
CACHE = Path.home() / ".cache/kontra-oracle-scenarios"


def scan_notes():
    return [(SCAN_START + i * SCAN_SPACING, SCAN_LEN, k, SCAN_VEL) for i, k in enumerate(SCAN_KEYS)]


def sounding_keys(wav, notes):
    """Keys whose note window peaks above PLAY_DB in the KONTRA scan render."""
    rate = compare.RATE
    x = np.abs(compare.load(wav)).max(axis=0)  # (2, n) -> per-frame peak over channels
    keys = []
    for start, length, key, _ in notes:
        a, b = int(start * rate), int((start + length) * rate)
        peak = float(x[a:b].max()) if b <= len(x) else 0.0
        if 20 * np.log10(peak + 1e-12) > PLAY_DB:
            keys.append(key)
    return sorted(set(keys))


def choose(keys):
    """Three notes: quartiles of the sounding keys (duplicates dropped), with the usual timing."""
    if not keys:
        return None
    picks = [keys[int(q * (len(keys) - 1))] for q in (0.25, 0.5, 0.75)]
    picks = sorted(set(picks), key=picks.index)
    while len(picks) < 3:
        picks.append(picks[-1])
    return [[s, l, k, v] for (s, l), k, v in zip(CHOSEN_TIMES, picks, CHOSEN_VELS)]


def render_scan(nki, work):
    midi = work / "scan.mid"
    capture.make_midi(midi, 1, notes=scan_notes())
    wav = work / "scan.wav"
    p = subprocess.run(kontra_render.capped([kontra_render.binary(), "render-kontakt-midi", nki, str(wav), str(midi)]),
                       capture_output=True, text=True, timeout=900)
    if p.returncode != 0 or not wav.exists():
        return None, p.stderr.strip().splitlines()[-1:] or ["render failed"]
    return wav, []


def derive(nki):
    """{"nki", "keys", "notes" or None, "reason"} for one NKI, cached by path, size and mtime."""
    path = Path(nki)
    st = path.stat()
    key = hashlib.sha1(f"{path}:{st.st_size}:{st.st_mtime_ns}".encode()).hexdigest()
    CACHE.mkdir(parents=True, exist_ok=True)
    cached = CACHE / f"{key}.json"
    if cached.exists():
        return json.loads(cached.read_text())
    work = CACHE / f"work-{key}"
    work.mkdir(parents=True, exist_ok=True)
    wav, err = render_scan(str(path), work)
    if wav is None:
        result = {"nki": nki, "keys": [], "notes": None, "reason": "KONTRA render failed: " + " ".join(err)}
    else:
        keys = sounding_keys(wav, scan_notes())
        notes = choose(keys)
        reason = "" if notes else "no key 24..96 sounds on KONTRA's default state (keyswitch or articulation state gates every note)"
        result = {"nki": nki, "keys": keys, "notes": notes, "reason": reason}
    cached.write_text(json.dumps(result))
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("nkis", nargs="+")
    ap.add_argument("--json")
    args = ap.parse_args()
    results = {Path(n).name: derive(n) for n in args.nkis}
    text = json.dumps(results, indent=1)
    if args.json:
        Path(args.json).write_text(text)
    else:
        print(text)


if __name__ == "__main__":
    main()
