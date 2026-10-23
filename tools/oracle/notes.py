#!/usr/bin/env python3
"""Per-note comparison of a Kontakt capture against its KONTRA render, for one instrument.

    tools/oracle/notes.py KONTAKT.wav KONTRA.wav --midi FILE [--json OUT.json]

The renders are aligned exactly as compare.py does (envelope cross-correlation over +-2.5 s), then
every note in the MIDI file is cut from both at its onset and scored on its own: waveform r (+-20 ms),
level difference over the note's first 0.3 s, and the onset lag. Per-note numbers separate the causes
of a mismatch: a sample or round-robin difference shows as low r on some notes and high r on others,
a velocity-layer difference as a level/r step across the velocity sweep, a legato difference on the
overlapping notes only, and a start offset as a constant onset lag.
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
SEG_S = 0.3  # per-note window after the onset


def parse_midi(path):
    """Notes of a format-0 file as [(on_s, off_s, key, velocity)], as capture.make_midi writes them.
    Running status is handled; anything else (meta, sysex, other channels) is skipped."""
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
            length = data[pos + 2]
            pos += 3 + length
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


def aligned(kontakt, kontra, window):
    """Both renders from the slot start, shifted by the envelope lag so that the same index is the
    same instant. Returns (k, r, lag_ms)."""
    n = int(window * RATE)
    k_all = compare.load(kontakt)[:, int(compare.PRE * RATE):]
    r_all = compare.load(kontra)
    lag_frames, _ = compare.envelope_lag(compare.envelope(k_all), compare.envelope(r_all[:, : n + 2 * RATE]))
    lag = lag_frames * RATE // 100
    if lag >= 0:
        k, r = k_all[:, lag:], r_all
    else:
        k, r = k_all, r_all[:, -lag:]
    m = min(k.shape[1], r.shape[1])
    return k[:, :m], r[:, :m], lag_frames * 10.0


def score_note(k, r, t, length):
    a = int(t * RATE)
    b = min(int((t + min(length, SEG_S)) * RATE), k.shape[1], r.shape[1])
    if b - a < RATE // 100 or a < 0:
        return None
    ks, rs = k[:, a:b], r[:, a:b]
    margin = int(0.1 * RATE)  # onsets are searched in [t-100 ms, t+300 ms]
    lo, hi = max(0, a - margin), min(k.shape[1], b + margin)
    ko = compare.onsets(compare.envelope(k[:, lo:hi]))
    ro = compare.onsets(compare.envelope(r[:, max(0, a - margin):min(r.shape[1], b + margin)]))
    lag_frames, wave_r = compare.waveform_r(ks, rs, max_lag=RATE // 50)
    return {
        "wave_r": round(float(wave_r), 3),
        "level_diff_db": round(compare.rms_db(ks) - compare.rms_db(rs), 2),
        "onset_kontakt_ms": round(float(ko[0]) * 10.0 - margin * 1000 / RATE, 1) if len(ko) else None,
        "onset_kontra_ms": round(float(ro[0]) * 10.0 - margin * 1000 / RATE, 1) if len(ro) else None,
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("kontakt")
    ap.add_argument("kontra")
    ap.add_argument("--midi", required=True)
    ap.add_argument("--window", type=float, default=6.0)
    ap.add_argument("--json")
    args = ap.parse_args()
    notes = parse_midi(args.midi)
    k, r, lag_ms = aligned(args.kontakt, args.kontra, args.window)
    rows = []
    for i, (on, off, key, vel) in enumerate(notes):
        s = score_note(k, r, on, off - on)
        if s:
            rows.append({"i": i, "t_s": on, "key": key, "vel": vel, **s})
    print(f"alignment lag {lag_ms:+.0f} ms; {len(rows)}/{len(notes)} notes scored")
    print(f"{'#':>3} {'t(s)':>6} {'key':>4} {'vel':>4} {'r':>6} {'lvl dB':>7} {'onK ms':>8} {'onR ms':>8}")
    for row in rows:
        print(f"{row['i']:>3} {row['t_s']:>6.2f} {row['key']:>4} {row['vel']:>4} {row['wave_r']:>6.3f} "
              f"{row['level_diff_db']:>+7.1f} {str(row['onset_kontakt_ms']):>8} {str(row['onset_kontra_ms']):>8}")
    if rows:
        rs = np.array([row["wave_r"] for row in rows])
        print(f"r: median {np.median(rs):.3f}, min {rs.min():.3f}, max {rs.max():.3f}; "
              f"notes with r>0.9: {int((rs > 0.9).sum())}/{len(rs)}")
    if args.json:
        Path(args.json).write_text(json.dumps({"lag_ms": lag_ms, "notes": rows}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
