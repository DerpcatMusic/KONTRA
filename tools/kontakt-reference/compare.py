#!/usr/bin/env python3
"""Compare two renders (Kontakt reference vs KONTRA). Usage: compare.py REF.wav MINE.wav [--hop 0.05]

Aligns on the first onset (-60 dBFS over the loudest 20 ms RMS... see ONSET_DB),
then prints per-hop RMS dB, peak dB, spectral centroid and dominant pitch for
each file and their differences, plus a summary line. Needs numpy only.
"""
import struct, sys
import numpy as np

ONSET_DB = -50.0  # relative to the file's peak


def wav(path):
    b = open(path, "rb").read()
    i, fmt = 12, None
    while i + 8 <= len(b):
        tag, n = b[i:i + 4], struct.unpack("<I", b[i + 4:i + 8])[0]
        body = b[i + 8:i + 8 + n]
        if tag == b"fmt ":
            fmt = struct.unpack("<HHIIHH", body[:16])
        elif tag == b"data":
            code, ch, sr, _, _, bits = fmt
            dt = {(3, 32): "<f4", (1, 16): "<i2", (1, 32): "<i4", (0xFFFE, 32): "<f4"}[(code, bits)]
            x = np.frombuffer(body[: len(body) // (bits // 8) * (bits // 8)], dt).astype(np.float64)
            if dt != "<f4":
                x /= 2 ** (bits - 1)
            return x.reshape(-1, ch).mean(axis=1), sr
        i += 8 + n + (n & 1)
    raise SystemExit(f"{path}: no data chunk")


def onset(x, sr):
    peak = np.abs(x).max()
    if peak == 0:
        return 0
    hits = np.nonzero(np.abs(x) > peak * 10 ** (ONSET_DB / 20))[0]
    return int(hits[0])


def db(v):
    return 20 * np.log10(np.maximum(v, 1e-9))


def analyse(x, sr, hop):
    n, w = int(hop * sr), int(hop * sr)
    rows = []
    for s in range(0, len(x) - w + 1, n):
        f = x[s:s + w]
        spec = np.abs(np.fft.rfft(f * np.hanning(w)))
        freq = np.fft.rfftfreq(w, 1 / sr)
        tot = spec.sum()
        cen = (spec * freq).sum() / tot if tot > 1e-9 else 0.0
        pk = freq[spec.argmax()] if tot > 1e-9 else 0.0
        rows.append((s / sr, db(np.sqrt((f ** 2).mean())), db(np.abs(f).max()), cen, pk))
    return np.array(rows)


def end_of_sound(x, sr, off=-60.0):
    hits = np.nonzero(np.abs(x) > np.abs(x).max() * 10 ** (off / 20))[0]
    return hits[-1] / sr if len(hits) else 0.0


def main():
    a = [s for s in sys.argv[1:] if not s.startswith("--")]
    hop = float(sys.argv[sys.argv.index("--hop") + 1]) if "--hop" in sys.argv else 0.05
    if "--hop" in sys.argv:
        a.remove(sys.argv[sys.argv.index("--hop") + 1])
    (r, sr), (m, sr2) = wav(a[0]), wav(a[1])
    if sr != sr2:
        raise SystemExit(f"sample rates differ: {sr} vs {sr2}")
    ro, mo = onset(r, sr), onset(m, sr)
    r, m = r[ro:], m[mo:]
    n = min(len(r), len(m))
    ra, ma = analyse(r[:n], sr, hop), analyse(m[:n], sr, hop)
    print(f"onset  ref {ro / sr:.4f}s  mine {mo / sr:.4f}s  (aligned)")
    print(f"peak   ref {db(np.abs(r).max()):.2f} dB  mine {db(np.abs(m).max()):.2f} dB")
    print(f"tail   ref {end_of_sound(r, sr):.3f}s  mine {end_of_sound(m, sr):.3f}s  (last sample above -60 dB re peak)")
    print(f"{'t':>6} {'rms_ref':>8} {'rms_me':>8} {'d':>6} {'pk_ref':>8} {'pk_me':>8} {'cen_ref':>8} {'cen_me':>8} {'f_ref':>7} {'f_me':>7}")
    for x, y in zip(ra, ma):
        if x[1] < -100 and y[1] < -100:
            continue  # both silent
        print(f"{x[0]:6.2f} {x[1]:8.1f} {y[1]:8.1f} {y[1]-x[1]:6.1f} {x[2]:8.1f} {y[2]:8.1f} {x[3]:8.0f} {y[3]:8.0f} {x[4]:7.1f} {y[4]:7.1f}")
    live = ra[:, 1] > -70
    d = (ma[:, 1] - ra[:, 1])[live]
    print(f"summary: rms diff mean {d.mean():+.2f} dB  max|d| {np.abs(d).max():.2f} dB  "
          f"centroid ratio {np.median(ma[live, 3] / np.maximum(ra[live, 3], 1)):.3f}")


if __name__ == "__main__":
    main()
