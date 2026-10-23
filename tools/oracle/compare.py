#!/usr/bin/env python3
"""Per-instrument diff of a Kontakt capture against the KONTRA render of the same notes.

    tools/oracle/compare.py CAPTURE_DIR KONTRA_DIR [--window S] [--md OUT.md] [--json OUT.json]

CAPTURE_DIR: capture.json + <name>.wav from capture.py (slot starts PRE seconds into each WAV).
KONTRA_DIR:  <name>.wav from kontra_render.py (the same notes, starting at 0).

Per instrument, over the first WINDOW seconds of the note scenario:
  level      integrated RMS and peak, dBFS, Kontakt minus KONTRA (L+R)
  onsets     10 ms envelope rises; per-note onset lag (Kontakt minus KONTRA) in ms
  timing     the envelope cross-correlation lag (+-2.5 s); renders are aligned by it before comparing; the median over all instruments is the
             systematic latency (audio path + MIDI), the per-instrument deviation from it is the diff
  alignment  |timing deviation| >= 290 ms = the search hit its edge; timing and waveform are then void
  timbre     1/3-octave band levels after removing the broadband level offset: RMS of the residual, dB
  waveform   normalised cross-correlation at the best lag (+-20 ms) after the timing alignment;
             r near 1 = same sample and articulation; low r = wrong sample, articulation or
             round-robin (a single capture cannot separate those three)
  silence    either side below -80 dBFS: the capture or the render failed, not a parity result

Flags use these thresholds (module constants below). The severity score ranks instruments: a
weighted sum of the deviations, larger = worse. Round-robin cannot be identified from one note
scenario; it shows up as low `r` and is listed as "sample/articulation/RR suspected".
"""
import argparse
import importlib.util
import json
import math
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("wavutil", HERE.parent / "kontakt-oracle" / "wavutil.py")
wavutil = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wavutil)

RATE = 48000
PRE = 0.5
LEVEL_TOL_DB = 1.0  # level deviation from the median offset
TIMING_TOL_MS = 10.0  # onset lag deviation from the median
TIMBRE_TOL_DB = 3.0  # band residual RMS
WAVE_R_MIN = 0.7  # normalised waveform correlation
SILENT_DB = -80.0


def load(path):
    ch, rate, chans = wavutil.read(path)
    assert rate == RATE, f"{path}: {rate} Hz, expected {RATE}"
    x = np.asarray(chans, dtype=np.float64)  # (ch, n)
    if x.shape[0] == 1:
        x = np.vstack([x, x])
    return x[:2]


def db(v):
    return 20 * math.log10(max(v, 1e-12))


def rms_db(x):
    return db(math.sqrt(float(np.mean(x * x)) + 1e-30))


def envelope(x, frame=RATE // 100):
    """10 ms RMS of the stereo sum, in dB."""
    mono = x.mean(axis=0)
    n = len(mono) // frame
    e = np.sqrt(np.mean(mono[: n * frame].reshape(n, frame) ** 2, axis=1) + 1e-30)
    return 20 * np.log10(np.maximum(e, 1e-12))


def onsets(env_db):
    """Rising edges through (peak - 30 dB), hysteresis 10 dB; returns frame indices."""
    thr = env_db.max() - 30.0
    out, armed = [], True
    for i, v in enumerate(env_db):
        if armed and v > thr:
            out.append(i)
            armed = False
        elif not armed and v < thr - 10.0:
            armed = True
    return out


def envelope_lag(ke, re, center=0, span=250):
    """Lag (frames) that maximises the correlation of the mean-removed envelopes, searched in
    center +- span frames; + means K later."""
    n = min(len(ke), len(re))
    best, best_lag = -2.0, 0
    k, r = ke[:n] - ke[:n].mean(), re[:n] - re[:n].mean()
    for lag in range(center - span, center + span + 1):
        if lag >= 0:
            a, b = k[lag:], r[: n - lag]
        else:
            a, b = k[: n + lag], r[-lag:]
        m = min(len(a), len(b))
        if m < 50:
            continue
        a, b = a[:m], b[:m]
        c = float(np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b) + 1e-30))
        if c > best:
            best, best_lag = c, lag
    return best_lag, best


def waveform_r(k, r, max_lag=RATE // 50):
    """Normalised correlation of the mono sums at the best lag within +-20 ms (+ means K later)."""
    km, rm = k.mean(axis=0), r.mean(axis=0)
    best, best_lag = -2.0, 0
    for lag in range(-max_lag, max_lag + 1, 4):  # 4-sample steps; refined below
        if lag >= 0:
            a, b = km[lag:], rm[: len(rm) - lag]
        else:
            a, b = km[: len(km) + lag], rm[-lag:]
        m = min(len(a), len(b), 2 * RATE)
        if m < RATE // 2:
            continue
        a, b = a[:m], b[:m]
        c = float(np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b) + 1e-30))
        if c > best:
            best, best_lag = c, lag
    return best_lag, best


BANDS = []  # 1/3-octave centres 40 Hz .. 16 kHz
f = 40.0
while f <= 16000:
    BANDS.append(f)
    f *= 2 ** (1 / 3)


def band_levels(x, n_fft=4096):
    mono = x.mean(axis=0)
    win = np.hanning(n_fft)
    frames = [mono[i:i + n_fft] * win for i in range(0, len(mono) - n_fft, n_fft // 2)]
    if not frames:
        return np.full(len(BANDS), -200.0)
    spec = np.abs(np.fft.rfft(np.array(frames), axis=1)) ** 2
    frame_energy = spec.sum(axis=1)
    keep = frame_energy > frame_energy.max() * 1e-6  # active frames only
    p = spec[keep].mean(axis=0)
    freqs = np.fft.rfftfreq(n_fft, 1 / RATE)
    out = []
    for c in BANDS:
        lo, hi = c / 2 ** (1 / 6), c * 2 ** (1 / 6)
        sel = (freqs >= lo) & (freqs < hi)
        out.append(10 * math.log10(p[sel].sum() + 1e-30) if sel.any() else -200.0)
    return np.array(out)


def compare_one(name, kontakt_path, kontra_path, window, center_ms=None):
    """Aligns the two renders first (envelope cross-correlation over +-2.5 s), then compares the
    overlap of the first WINDOW seconds, so a systematic offset never truncates either side."""
    n = int(window * RATE)
    k_all = load(kontakt_path)[:, int(PRE * RATE):]  # from the slot start
    r_all = load(kontra_path)  # from the slot start
    res = {"name": name}
    res["kontakt_rms_db"] = rms_db(k_all[:, :n])
    res["kontra_rms_db"] = rms_db(r_all[:, :n])
    if res["kontakt_rms_db"] < SILENT_DB or res["kontra_rms_db"] < SILENT_DB:
        res["silent"] = "kontakt" if res["kontakt_rms_db"] < SILENT_DB else "kontra"
        return res
    if center_ms is None:  # first pass: coarse search over +-2.5 s
        lag_frames, env_r = envelope_lag(envelope(k_all), envelope(r_all[:, : n + 2 * RATE]))
    else:  # second pass: +-300 ms around the median lag, so a local envelope match cannot win
        lag_frames, env_r = envelope_lag(envelope(k_all), envelope(r_all[:, : n + 2 * RATE]),
                                         center=int(round(center_ms / 10.0)), span=30)
    lag = lag_frames * RATE // 100  # samples; + means Kontakt is later
    if lag >= 0:
        k, r = k_all[:, lag:], r_all
    else:
        k, r = k_all, r_all[:, -lag:]
    m = min(k.shape[1], r.shape[1], n)
    k, r = k[:, :m], r[:, :m]
    res["seconds"] = m / RATE
    res["kontakt_rms_db"] = rms_db(k)
    res["kontra_rms_db"] = rms_db(r)
    res["kontakt_peak_db"] = db(np.abs(k).max())
    res["kontra_peak_db"] = db(np.abs(r).max())
    res["level_diff_db"] = res["kontakt_rms_db"] - res["kontra_rms_db"]
    res["peak_diff_db"] = res["kontakt_peak_db"] - res["kontra_peak_db"]
    res["env_lag_ms"] = lag_frames * 10.0
    res["env_r"] = env_r
    ke, re = envelope(k), envelope(r)
    ko, ro = onsets(ke), onsets(re)
    res["onsets_kontakt_ms"] = [round(i * 10.0, 1) for i in ko]
    res["onsets_kontra_ms"] = [round(i * 10.0, 1) for i in ro]
    kb, rb = band_levels(k), band_levels(r)
    resid = (kb - rb) - np.median(kb - rb)
    res["timbre_db"] = float(np.sqrt(np.mean(resid ** 2)))
    res["band_diff_db"] = [round(float(v), 1) for v in (kb - rb)]
    fine_lag, fine_r = waveform_r(k[:, : 2 * RATE], r[:, : 2 * RATE])
    res["wave_r"] = fine_r
    res["wave_lag_ms"] = fine_lag * 1000.0 / RATE
    return res


def finish(rows):
    """Median-referenced deviations, flags and severity."""
    ok = [r for r in rows if "silent" not in r]
    if not ok:
        return rows
    med_level = float(np.median([r["level_diff_db"] for r in ok]))
    med_lag = float(np.median([r["env_lag_ms"] for r in ok]))
    med_wlag = float(np.median([r["wave_lag_ms"] for r in ok]))
    for r in ok:
        r["level_dev_db"] = r["level_diff_db"] - med_level
        r["timing_dev_ms"] = r["env_lag_ms"] - med_lag
        r["wave_lag_dev_ms"] = r["wave_lag_ms"] - med_wlag
        # an envelope lag at the edge of its +-300 ms search is a failed alignment: timing and
        # waveform say nothing about the instrument then
        unreliable = abs(r["timing_dev_ms"]) >= 290
        r["alignment_unreliable"] = unreliable
        flags = []
        if abs(r["level_dev_db"]) > LEVEL_TOL_DB:
            flags.append("level")
        if r["timbre_db"] > TIMBRE_TOL_DB:
            flags.append("timbre")
        if unreliable:
            flags.append("alignment unreliable")
        else:
            if abs(r["timing_dev_ms"]) > TIMING_TOL_MS:
                flags.append("timing")
            if r["wave_r"] < WAVE_R_MIN:
                flags.append("sample/articulation/RR suspected")
            elif abs(r["wave_lag_dev_ms"]) > 1.5:
                flags.append("start offset")
        r["flags"] = flags
        r["severity"] = round(abs(r["level_dev_db"]) / LEVEL_TOL_DB + r["timbre_db"] / TIMBRE_TOL_DB
                              + (0 if unreliable else abs(r["timing_dev_ms"]) / TIMING_TOL_MS
                                 + 5 * (1 - r["wave_r"])), 3)
    rows.sort(key=lambda r: (-(r.get("severity") or 0), r["name"]))
    return rows, {"median_level_offset_db": med_level, "median_env_lag_ms": med_lag}


def markdown(rows, summary, window):
    lines = [
        "# Kontakt vs KONTRA: ranked mismatch report",
        "",
        f"Window {window:g} s per instrument; Kontakt 8.13.1 standalone under Wine (PipeWire null sink), "
        "KONTRA `render-kontakt-midi` on the same note scenario. Generated by `tools/oracle/compare.py`.",
        "",
        f"Median level offset (Kontakt - KONTRA): {summary['median_level_offset_db']:+.2f} dB; "
        f"median envelope lag (systematic latency): {summary['median_env_lag_ms']:+.1f} ms. "
        "Deviations below are from these medians.",
        "",
        "| # | instrument | level dev dB | timing dev ms | timbre dB | waveform r | flags | severity |",
        "|---|---|---:|---:|---:|---:|---|---:|",
    ]
    for i, r in enumerate(rows, 1):
        if "silent" in r:
            lines.append(f"| {i} | {r['name']} | silent ({r['silent']}) | | | | capture/render failure, not a parity result | |")
            continue
        lines.append(f"| {i} | {r['name']} | {r['level_dev_db']:+.2f} | {r['timing_dev_ms']:+.1f} | "
                     f"{r['timbre_db']:.2f} | {r['wave_r']:.3f} | {', '.join(r['flags']) or 'ok'} | {r['severity']:.2f} |")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("capture_dir")
    ap.add_argument("kontra_dir")
    ap.add_argument("--window", type=float, default=6.0)
    ap.add_argument("--md")
    ap.add_argument("--json")
    args = ap.parse_args()
    cap = Path(args.capture_dir)
    manifest = json.loads((cap / "capture.json").read_text())
    rows = []
    for name, entry in sorted(manifest.items()):
        kontra = Path(args.kontra_dir) / f"{name}.wav"
        if not entry.get("wav") or not kontra.exists():
            rows.append({"name": name, "silent": "capture" if not entry.get("wav") else "kontra", "error": entry.get("error", "")})
            continue
        rows.append(compare_one(name, entry["wav"], kontra, args.window))
    coarse = [r["env_lag_ms"] for r in rows if "silent" not in r]
    center = float(np.median(coarse)) if coarse else 0.0
    for i, r in enumerate(rows):
        if "silent" not in r:
            rows[i] = compare_one(r["name"], manifest[r["name"]]["wav"], Path(args.kontra_dir) / f"{r['name']}.wav",
                                  args.window, center_ms=center)
    out = finish(rows)
    if isinstance(out, tuple):
        rows, summary = out
    else:
        rows, summary = out, {"median_level_offset_db": float("nan"), "median_env_lag_ms": float("nan")}
    if args.json:
        Path(args.json).write_text(json.dumps({"summary": summary, "rows": rows}, indent=1, default=float))
    if args.md:
        Path(args.md).write_text(markdown(rows, summary, args.window))
    flagged = sum(1 for r in rows if r.get("flags"))
    print(f"{len(rows)} instruments, {flagged} flagged, "
          f"{sum(1 for r in rows if 'silent' in r)} silent (capture/render failure)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
