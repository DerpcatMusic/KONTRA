#!/usr/bin/env python3
"""Supercharger GT transfer curve and attack/release from stimrender.py output (.npz).

    supercharger-gt-analyze.py RAMP.npz RELEASE.npz OUT_DIR

RAMP.npz: stimrender.py stimulus "ramp" (200 Hz sine ramp 0 -> 1.0 over 0.6 s). Gain = peak(effect L) / peak(reference R)
in one 200 Hz cycle-pair window around each input level (0.2 .. 1.0), as in supercharger-static.tsv.
RELEASE.npz: stimrender.py stimulus "release" (0.2 s DC at 0.6, then 0.7 s 2 kHz probe at 0.02). Gain envelope = 1 ms
RMS (effect) minus 1 ms RMS (latency-free reference). Attack = DC onset to 63% of the steady gain (taken at 0.19 s);
release = probe onset to 63% recovery (37% of the offset gain left).
Writes OUT_DIR/supercharger-gt-ramp.tsv and OUT_DIR/supercharger-gt-release.tsv.
"""
import sys
from pathlib import Path

import numpy as np

RATE = 48000
LEVELS = [0.2, 0.4, 0.6, 0.8, 1.0]
RAMP_S = 0.6
DC_S = 0.2
PRE = 64  # stimrender.py starts each capture 64 samples before the first sound


def ramp_gains(npz):
    d = np.load(npz)
    x_names = [str(n) for n in d["names"]]
    rows = []
    for name, y, r in zip(x_names, d["y"], d["r"]):
        g = []
        for a in LEVELS:
            t = a * RAMP_S  # linspace(0, 1, n) over RAMP_S: amplitude a at t = a * RAMP_S
            c = int((t + 0.0) * RATE) + PRE
            w = int(0.0025 * RATE)  # +-2.5 ms, about one 200 Hz period
            py = np.max(np.abs(y[c - w:c + w]))
            pr = np.max(np.abs(r[c - w:c + w]))
            g.append(20 * np.log10(py / pr) if py > 0 and pr > 0 else float("nan"))
        rows.append((name, g))
    return rows


def env_db(v):
    n = int(0.001 * RATE)
    m = len(v) // n
    return 10 * np.log10(np.mean(v[:m * n].reshape(m, n) ** 2, axis=1) + 1e-20)  # per 1 ms


def release_times(npz):
    d = np.load(npz)
    rows = []
    for name, y, r in zip([str(n) for n in d["names"]], d["y"], d["r"]):
        gain = env_db(y) - env_db(r)  # per 1 ms
        dc = int(DC_S * 1000)
        steady = gain[dc - 10:dc - 1].mean() if dc > 10 else gain[0]
        # gain[0] is the DC onset (after the PRE samples); steady gain taken just before the DC offset
        attack = next((k for k in range(0, dc) if gain[k] <= 0.63 * steady), None)
        off_gain = gain[dc + 1] if len(gain) > dc + 1 else gain[-1]
        release = next((k for k in range(dc + 1, len(gain)) if gain[k] >= 0.37 * off_gain), None)
        rows.append((name, "" if attack is None else attack, "" if release is None else release - dc,
                     f"{steady:.2f}", f"{off_gain:.2f}"))
    return rows


def main():
    ramp_npz, rel_npz, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    out.mkdir(parents=True, exist_ok=True)
    lines = ["step\t" + "\t".join(f"gain@in{a}" for a in LEVELS)]
    for name, g in ramp_gains(ramp_npz):
        lines.append(name + "\t" + "\t".join(f"{v:.2f}" for v in g))
    (out / "supercharger-gt-ramp.tsv").write_text("\n".join(lines) + "\n")
    lines = ["step\tattack63_ms\trelease63_ms\tsteady_dB\toffset_dB"]
    for row in release_times(rel_npz):
        lines.append("\t".join(str(v) for v in row))
    (out / "supercharger-gt-release.tsv").write_text("\n".join(lines) + "\n")
    print((out / "supercharger-gt-ramp.tsv").read_text())
    print((out / "supercharger-gt-release.tsv").read_text())


if __name__ == "__main__":
    main()
