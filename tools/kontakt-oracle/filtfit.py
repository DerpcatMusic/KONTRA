#!/usr/bin/env python3
"""Cutoff / Q / slope of filter responses from an eqrender run (filt spec).

    filtfit.py PREFIX      (PREFIX.tsv + PREFIX.h.npy)  -> TSV on stdout

LP/HP: -3 dB point relative to the passband (LP: 30 Hz, HP: 18 kHz), slope over the octave that starts one
octave past the corner, peak above the passband (resonance).  BP: centre, -3 dB bandwidth, Q = fc / bw,
slopes either side.  NOTCH: notch frequency and depth.
"""
import sys

import numpy as np

F = np.fft.rfftfreq(16384, 1 / 48000)
SEL = (F >= 20) & (F <= 21000)


def at(db, f):
    return float(np.interp(f, F, db))


def analyse(name, db):
    f, d = F[SEL], db[SEL]
    kind = name.split()[0]
    if "NOTCH" in kind:
        k = int(np.argmin(d))
        return dict(kind="notch", f0=f[k], depth=d[k], passband=at(db, 30))
    if "BP" in kind:
        k = int(np.argmax(d))
        pk = d[k]
        lo = k
        while lo > 0 and d[lo] > pk - 3:
            lo -= 1
        hi = k
        while hi < len(d) - 1 and d[hi] > pk - 3:
            hi += 1
        return dict(kind="bp", f0=f[k], peak=pk, q=f[k] / max(f[hi] - f[lo], 1e-9),
                    slope_lo=at(db, f[k] / 4) - at(db, f[k] / 8), slope_hi=at(db, f[k] * 4) - at(db, f[k] * 8))
    lp = "LP" in kind
    ref = at(db, 30) if lp else at(db, 18000)
    idx = np.nonzero(d >= ref - 3)[0]
    if not len(idx):
        return dict(kind="lp" if lp else "hp", fc=float("nan"), passband=ref)
    fc = f[idx[-1]] if lp else f[idx[0]]
    a, b = (fc * 2, fc * 4) if lp else (fc / 2, fc / 4)
    slope = at(db, a) - at(db, b) if (lp and b < 21000) or (not lp and b > 20) else float("nan")
    return dict(kind="lp" if lp else "hp", fc=fc, passband=ref, slope_db_per_oct=abs(slope),
                reso_peak=float(d.max() - ref))


def main():
    p = sys.argv[1]
    rows = [l.split("\t") for l in open(p + ".tsv").read().splitlines()[1:]]
    names = [(r[0], r[1]) for r in rows if r[2] == "LL"]
    h = np.load(p + ".h.npy")
    print("step\tname\tresult")
    for (n, name), db in zip(names, h):
        r = analyse(name, db)
        print(f"{n}\t{name}\t" + " ".join(f"{k}={v:.2f}" if isinstance(v, float) else f"{k}={v}" for k, v in r.items()))


if __name__ == "__main__":
    main()
