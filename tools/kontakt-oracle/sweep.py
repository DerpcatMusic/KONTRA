#!/usr/bin/env python3
"""Knob sweeps through stimrender.py in batches, aligned per step on the reference channel.

    sweep.py SPEC.json OUT.npz [BATCH=6]

Long sweeps in one Kontakt session lost audio after a few steps, so the steps run in batches,
each batch in its own session (waits for the single Kontakt oracle slot). Within a batch the
note of each step is found by stimrender.py. Then every step's effect channel (y) is shifted by
the lag that aligns the note onset of its reference channel (r, the same stimulus with no effect)
to the bypass step's onset. Output: x, y[step], r[step], names, lags.
"""
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent


def wait_for_slot():
    while subprocess.run(["pgrep", "-f", "[K]ontakt 8.exe"], capture_output=True).returncode == 0:
        time.sleep(20)


ONSET = 0.02  # reference level that marks the note onset in the clean reference channel


def onset(ref):
    """First sample where the clean reference channel exceeds ONSET. Cross-correlation is not
    used: a 200 Hz stimulus repeats every 240 samples, so its peaks alias across lags."""
    return int(np.argmax(np.abs(ref) > ONSET))


def run_batch(spec, steps, work):
    sub = dict(spec, steps=steps)
    path = work / f"batch{steps[0]['name']}.json".replace(" ", "_").replace("=", "_")
    path.write_text(json.dumps(sub))
    wait_for_slot()
    prefix = work / path.stem
    subprocess.run([sys.executable, str(HERE / "stimrender.py"), str(path), str(prefix)], check=True)
    return np.load(prefix.with_suffix(".npz"))


def main():
    spec = json.load(open(sys.argv[1]))
    batch = int(sys.argv[3]) if len(sys.argv) > 3 else 6
    steps = spec["steps"]
    x = None
    ys, rs, names = [], [], []
    with tempfile.TemporaryDirectory(dir=Path.home() / ".cache" / "kontra-amps-b") as tmp:
        work = Path(tmp)
        for start in range(0, len(steps), batch):
            d = run_batch(spec, steps[start:start + batch], work)
            x = d["x"]
            # Batch names begin with "bypass"; keep only the swept steps (and the first bypass once).
            for i, nm in enumerate(d["names"]):
                if nm == "bypass" and start > 0:
                    continue
                ys.append(d["y"][i])
                rs.append(d["r"][i])
                names.append(nm)
    ys, rs = np.array(ys), np.array(rs)
    lags = []
    first = onset(rs[0])
    for i in range(len(ys)):
        if np.isnan(rs[i]).any():
            lags.append(None)
            continue
        L = onset(rs[i]) - first
        lags.append(L)
        if L > 0:
            ys[i] = np.concatenate([ys[i][L:], np.full(L, np.nan)])
        elif L < 0:
            ys[i] = np.concatenate([np.full(-L, np.nan), ys[i][:L]])
    np.savez(sys.argv[2], x=x, y=ys, r=rs, names=np.array(names), lags=np.array(
        [-9999 if v is None else v for v in lags]))
    print(sys.argv[2], "steps", len(names), "lags", lags)


if __name__ == "__main__":
    main()
