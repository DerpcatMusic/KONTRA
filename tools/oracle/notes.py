#!/usr/bin/env python3
"""Per-note comparison of a Kontakt capture against its KONTRA render, for one instrument.

    tools/oracle/notes.py KONTAKT.wav KONTRA.wav --midi FILE [--json OUT.json]

Both files are slots from the slot start (capture.py output and kontra_render.py output). For each
note in the MIDI file the attack is found in each render (first 10 ms frame >= 10 dB above the level
just before the note, within 600 ms; KONTRA renders at zero latency, so its attack is the note time).
The Kontakt attack minus the KONTRA attack is that note's lag. Each note is then scored alone:
waveform r at its own lag (+-20 ms), level difference over 0.3 s after the attack, and whether an
attack was found at all. Per-note numbers separate the causes of a mismatch: a sample or
round-robin difference shows as low r on some notes and high r on others, a velocity-layer difference
as a step across the velocity sweep, a legato difference on the overlapping notes, and a start
offset as a lag that differs from the others.
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import compare  # noqa: E402

RATE = compare.RATE
TICKS_PER_S = 960  # capture.make_midi: 120 bpm, 480 ppq
FRAME = RATE // 100  # 10 ms
SEG_S = 0.3  # scored window after the attack
ATTACK_RISE_DB = 10.0
ATTACK_SEARCH_S = 0.6
MAX_LAG_S = 0.02


def parse_midi(path):
    """Notes of a format-0 file as [(on_s, off_s, key, velocity)], as capture.make_midi writes them.
    Running status is handled; meta events are skipped."""
    data = Path(path).read_bytes()
    assert data[:4] == b"MThd" and data[14:18] == b"MTrk", f"{path}: not a format-0 file"
    pos, end = 22, 22 + int.from_bytes(data[18:22], "big")
    tick, status, open_notes, notes = 0, 0, {}, []

    def vlq():
        nonlocal pos
        value = 0
        while True:
            b = data[pos]
            pos += 1
            value = (value << 7) | (b & 0x7F)
            if not b & 0x80:
                return value

    while pos < end:
        tick += vlq()
        b = data[pos]
        if b == 0xFF:  # meta
            pos += 3 + data[pos + 2]
            continue
        if b & 0x80:
            status = b
            pos += 1
        kind, chan = status & 0xF0, status & 0x0F
        if kind in (0xC0, 0xD0):  # one data byte
            pos += 1
            continue
        key, vel = data[pos], data[pos + 1]
        pos += 2
        if kind == 0x90 and vel > 0:
            open_notes.setdefault((chan, key), []).append((tick, vel))
        elif kind in (0x80, 0x90):
            stack = open_notes.get((chan, key))
            if stack:
                on_tick, on_vel = stack.pop(0)
                notes.append((on_tick / TICKS_PER_S, tick / TICKS_PER_S, key, on_vel))
    return sorted(notes)


def frame_db(x):
    """10 ms RMS of the channel mean, in dB."""
    m = x.mean(axis=0)
    n = len(m) // FRAME
    rms = np.sqrt((m[: n * FRAME].reshape(n, FRAME) ** 2).mean(axis=1) + 1e-18)
    return 20 * np.log10(rms)


def attack(db, t_s):
    """Frame of the first 10 ms frame at or after t_s that is ATTACK_RISE_DB above the level 100 ms
    before t_s, or None. The floor is the minimum over that 100 ms, so a sustained note is no attack."""
    f0 = int(round(t_s * 100))
    if f0 - 10 < 0 or f0 + 1 >= len(db):
        return None
    floor = float(np.min(db[f0 - 10:f0 + 1]))
    stop = min(len(db), f0 + int(ATTACK_SEARCH_S * 100))
    hits = np.nonzero(db[f0:stop] >= floor + ATTACK_RISE_DB)[0]
    return f0 + int(hits[0]) if len(hits) else None


def corr_at_lag(k, r, lag):
    """Normalised correlation of the mono mix; a positive lag means k is later than r."""
    km, rm = k.mean(axis=0), r.mean(axis=0)
    if lag >= 0:
        a, b = km[lag:], rm[: len(rm) - lag]
    else:
        a, b = km[: len(km) + lag], rm[-lag:]
    m = min(len(a), len(b))
    if m < RATE // 20:
        return -1.0
    a, b = a[:m], b[:m]
    return float(np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b) + 1e-30))


def best_corr(k, r):
    """Best correlation over +-MAX_LAG_S, 4-sample steps then refined by 1 sample."""
    span = int(MAX_LAG_S * RATE)
    coarse = max(range(-span, span + 1, 4), key=lambda lag: corr_at_lag(k, r, lag))
    fine = max(range(coarse - 3, coarse + 4), key=lambda lag: corr_at_lag(k, r, lag))
    return fine, corr_at_lag(k, r, fine)


def score(kontakt, kontra, notes):
    k_all, r_all = compare.load(kontakt)[:, int(compare.PRE * RATE):], compare.load(kontra)
    kdb, rdb = frame_db(k_all), frame_db(r_all)
    rows = []
    for i, (on, off, key, vel) in enumerate(notes):
        ka, ra = attack(kdb, on), attack(rdb, on)
        row = {"i": i, "t_s": round(on, 3), "key": key, "vel": vel, "attack_kontakt": ka is not None,
               "attack_kontra": ra is not None}
        if ka is None or ra is None:
            rows.append(row)
            continue
        # align on the attacks: each side's sample index is its own attack frame
        k0, r0 = ka * FRAME, ra * FRAME
        seg = int(min(SEG_S, max(off - on, 0.1)) * RATE)
        k = k_all[:, k0:k0 + seg + int(MAX_LAG_S * RATE) * 2]
        r = r_all[:, r0:r0 + seg + int(MAX_LAG_S * RATE) * 2]
        if k.shape[1] < seg // 2 or r.shape[1] < seg // 2:
            rows.append(row)
            continue
        m = min(k.shape[1], r.shape[1])
        pad = int(MAX_LAG_S * RATE)
        lag, rho = best_corr(k[:, pad:m - pad], r[:, pad:m - pad])
        # lag = Kontakt minus KONTRA: the attack difference plus the residual correlation lag
        row.update({
            "lag_ms": round((ka - ra) * 10.0 + lag * 1000.0 / RATE, 1),
            "wave_r": round(rho, 3),
            "level_diff_db": round(compare.rms_db(k[:, :seg]) - compare.rms_db(r[:, :seg]), 2),
        })
        rows.append(row)
    return rows


def matrix(kontakt, kontra, notes):
    """r of every KONTRA note (rows) against every Kontakt note (columns), each cut at its own attack.
    Round robin: a diagonal shift means the same samples in another order; a scattered matrix means
    different samples; a row with no high value means KONTRA's sample is not in Kontakt's sequence."""
    k_all, r_all = compare.load(kontakt)[:, int(compare.PRE * RATE):], compare.load(kontra)
    kdb, rdb = frame_db(k_all), frame_db(r_all)
    pad = int(MAX_LAG_S * RATE)

    def cut(x, db, t):
        a = attack(db, t)
        return None if a is None else x[:, a * FRAME: a * FRAME + int(SEG_S * RATE) + 2 * pad]

    rows = [cut(r_all, rdb, on) for on, _, _, _ in notes]
    cols = [cut(k_all, kdb, on) for on, _, _, _ in notes]
    grid = []
    for r in rows:
        line = []
        for k in cols:
            if r is None or k is None:
                line.append(None)
                continue
            m = min(r.shape[1], k.shape[1]) - 2 * pad
            line.append(round(best_corr(k[:, pad:pad + m], r[:, pad:pad + m])[1], 3))
        grid.append(line)
    return grid


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("kontakt")
    ap.add_argument("kontra")
    ap.add_argument("--midi", required=True)
    ap.add_argument("--matrix", action="store_true", help="also print KONTRA-note x Kontakt-note correlations")
    ap.add_argument("--json")
    args = ap.parse_args()
    notes = parse_midi(args.midi)
    rows = score(args.kontakt, args.kontra, notes)
    if args.matrix:
        grid = matrix(args.kontakt, args.kontra, notes)
        print("matrix: rows = KONTRA note i, columns = Kontakt note j (r at each own attack)")
        print("     " + "".join(f"{j:>7}" for j in range(len(grid[0]) if grid else 0)))
        for i, line in enumerate(grid):
            print(f"{i:>4} " + "".join(f"{'  -  ' if v is None else f'{v:>7.3f}'}" for v in line))
    scored = [r for r in rows if "wave_r" in r]
    print(f"{len(scored)}/{len(rows)} notes scored (attack found in both renders)")
    print(f"{'#':>3} {'t(s)':>6} {'key':>4} {'vel':>4} {'r':>6} {'lag ms':>7} {'lvl dB':>7}")
    for row in rows:
        if "wave_r" in row:
            print(f"{row['i']:>3} {row['t_s']:>6.2f} {row['key']:>4} {row['vel']:>4} {row['wave_r']:>6.3f} "
                  f"{row['lag_ms']:>+7.1f} {row['level_diff_db']:>+7.1f}")
        else:
            miss = "kontakt" if not row["attack_kontakt"] else "kontra"
            print(f"{row['i']:>3} {row['t_s']:>6.2f} {row['key']:>4} {row['vel']:>4}   no attack in {miss}")
    if scored:
        rs = np.array([r["wave_r"] for r in scored])
        lags = np.array([r["lag_ms"] for r in scored])
        print(f"r: median {np.median(rs):.3f}, min {rs.min():.3f}, max {rs.max():.3f}; r>0.9: "
              f"{int((rs > 0.9).sum())}/{len(rs)}. lag: median {np.median(lags):+.1f} ms, "
              f"range {lags.min():+.1f}..{lags.max():+.1f}")
    if args.json:
        Path(args.json).write_text(json.dumps(rows, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
