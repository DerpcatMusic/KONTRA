#!/usr/bin/env python3
"""Compare inspector v3 audio with capture.py using the shared oracle comparator."""
import importlib.util
import json
import math
import sys
from pathlib import Path

import numpy as np


def module(path):
    spec = importlib.util.spec_from_file_location("kontra_compare", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def bands(audio, rate):
    size, hop = 4096, 2048
    centers = np.array([31.25, 62.5, 125, 250, 500, 1000, 2000, 4000, 8000, 16000])
    window = np.hanning(size)
    freq = np.fft.rfftfreq(size, 1 / rate)
    masks = [(freq >= c / math.sqrt(2)) & (freq < c * math.sqrt(2)) for c in centers]
    sums = np.zeros(len(centers))
    count = 0
    for at in range(0, audio.shape[1] - size + 1, hop):
        spectrum = np.abs(np.fft.rfft(audio[:, at:at + size] * window, axis=1)) ** 2
        sums += [spectrum[:, mask].sum() for mask in masks]
        count += 1
    den = count * size * np.square(window).sum()
    return [None if den == 0 or x <= 0 else 10 * math.log10(x / den) for x in sums]


def metrics(audio, rate, events, shift):
    if audio.shape[1] == 0:
        return None
    diff = np.diff(audio, axis=1)
    bursts = []
    radius = rate // 200
    for event in events:
        if event.get("event", {}).get("type") != "cc":
            continue
        frame = int(event["frame"] + shift)
        lo, hi = max(0, frame - radius), min(audio.shape[1], frame + radius)
        if hi - lo < 3:
            bursts.append({"frame": event["frame"], "event": event["event"], "energy": None})
            continue
        d2 = np.diff(audio[:, lo:hi], n=2, axis=1)
        bursts.append({"frame": event["frame"], "event": event["event"], "energy": float(np.square(d2).sum())})
    return {"peak": float(np.abs(audio).max()),
            "clipped_samples": int(np.count_nonzero(np.abs(audio) > 1.0)),
            "max_sample_jump": float(np.abs(diff).max()) if diff.size else 0.0,
            "octave_band_rms_dbfs": bands(audio, rate), "cc_hf_bursts": bursts}


def main():
    compare_path, native_path, v3_path, events_path, window, alignment = sys.argv[1:]
    compare = module(Path(compare_path))
    events = json.loads(Path(events_path).read_text())
    if alignment == "first-onset":
        rate = compare.RATE
        native = compare.load(Path(native_path))[:, int(compare.PRE * rate):]
        v3 = compare.load(Path(v3_path))
        # Look only in the two-second segment; tails cannot set the threshold.
        nenv = compare.envelope(native[:, :2 * rate])
        venv = compare.envelope(v3[:, :2 * rate])
        if (not nenv.size or not venv.size or nenv.max() < compare.SILENT_DB
                or venv.max() < compare.SILENT_DB):
            print(json.dumps({"status":"uncomparable", "reason":"silent first segment", "alignment":"first onset"}))
            return
        no, vo = compare.onsets(nenv), compare.onsets(venv)
        if not no or not vo:
            print(json.dumps({"status":"uncomparable", "reason":"first onset not detected", "alignment":"first onset"}))
            return
        nf, vf = no[0] * rate // 100, vo[0] * rate // 100
        length = min(2 * rate, native.shape[1] - nf, v3.shape[1] - vf)
        if length < 4096:
            print(json.dumps({"status":"uncomparable", "reason":"insufficient aligned audio", "alignment":"first onset"}))
            return
        native, v3 = native[:, nf:nf + length], v3[:, vf:vf + length]
        nm, vm = metrics(native, rate, events, -nf), metrics(v3, rate, events, -vf)
        def db(value):
            return None if value <= 1e-12 else 20 * math.log10(value)
        def delta(a, b):
            return None if a is None or b is None else a - b
        nr = db(float(np.sqrt(np.mean(native * native))))
        vr = db(float(np.sqrt(np.mean(v3 * v3))))
        npk, vpk = db(nm["peak"]), db(vm["peak"])
        print(json.dumps({"status":"ok", "alignment":"first onset, 10 ms envelope bins; 2 s after each onset",
            "native_onset_ms": no[0] * 10, "v3_onset_ms": vo[0] * 10,
            "comparison":{"kontakt_rms_db":nr, "kontra_rms_db":vr, "level_diff_db":delta(nr,vr),
                          "kontakt_peak_db":npk, "kontra_peak_db":vpk, "peak_diff_db":delta(npk,vpk)},
            "native_metrics":nm, "v3_metrics":vm,
            "native_minus_v3":{"octave_band_rms_db":[delta(a,b) for a,b in zip(nm["octave_band_rms_dbfs"], vm["octave_band_rms_dbfs"])]},
            "octave_centers_hz":[31.25,62.5,125,250,500,1000,2000,4000,8000,16000],
            "unavailable":["native script callbacks and commands", "native voice lifecycle and levels", "native per-route values", "native effect parameters", "native active voice count"]}, allow_nan=False))
        return
    row = compare.compare_one("scenario", Path(native_path), Path(v3_path), float(window))
    if row.get("silent"):
        print(json.dumps({"status": "uncomparable", "reason": "silent " + row["silent"], "comparison": row}))
        return
    rate = compare.RATE
    native = compare.load(Path(native_path))[:, int(compare.PRE * rate):]
    v3 = compare.load(Path(v3_path))
    n = int(float(window) * rate)
    native, v3 = native[:, :n], v3[:, :n]
    lag = round(row["env_lag_ms"] * rate / 1000)
    native_metrics = metrics(native, rate, events, lag)
    v3_metrics = metrics(v3, rate, events, 0)
    def delta(a, b):
        return None if a is None or b is None else a - b
    diff = {"peak": delta(native_metrics["peak"], v3_metrics["peak"]),
            "clipped_samples": native_metrics["clipped_samples"] - v3_metrics["clipped_samples"],
            "max_sample_jump": delta(native_metrics["max_sample_jump"], v3_metrics["max_sample_jump"]),
            "octave_band_rms_db": [delta(a, b) for a, b in zip(native_metrics["octave_band_rms_dbfs"], v3_metrics["octave_band_rms_dbfs"])],
            "cc_hf_bursts": [{"frame": a["frame"], "event": a["event"], "energy": delta(a["energy"], b["energy"])}
                             for a, b in zip(native_metrics["cc_hf_bursts"], v3_metrics["cc_hf_bursts"])]}
    print(json.dumps({"status": "ok", "comparison": row,
        "native_metrics": native_metrics, "v3_metrics": v3_metrics, "native_minus_v3": diff,
        "octave_centers_hz": [31.25, 62.5, 125, 250, 500, 1000, 2000, 4000, 8000, 16000],
        "alignment": "10 ms envelope lag; native capture start has millisecond uncertainty",
        "unavailable": ["native script callbacks and commands", "native voice lifecycle and levels", "native per-route values", "native effect parameters", "native active voice count"]}, allow_nan=False))


if __name__ == "__main__":
    main()
