#!/usr/bin/env python3
"""Per-note sample attribution from audio alone: which zone's sample did Kontakt play, and did KONTRA play the same?

For every scenario note, the first WINDOW ms of the note are cross-correlated (FFT, normalised) against each
candidate zone that matches the note's key and velocity. A candidate is the zone's sample decoded by KONTRA's own
reader (examples/zone_samples.rs), shifted to the played pitch (zone key tracking and tune) and resampled to the
capture rate. The sample start may lie up to START_MAX ms into the sample, and the note may begin up to
TARGET_LEAD ms before the candidate starts. The best-correlating zone is the pick if r >= R_MIN.

Each note's Kontakt pick is compared with KONTRA's pick and classified:
  match                        same zone
  same sample, other zone      same asset file, different zone record
  wrong velocity layer         same key range and articulation group, different velocity range
  wrong articulation group     different group
  wrong key zone               different key range
  wrong sample (same zone)     same zone geometry and group, different asset (RR step or sample choice)
  timing only                  same zone, start offsets differ by more than TIMING_MS
  unidentified                 one or both sides have no candidate with r >= R_MIN

Usage:
  attribute.py NKI NAME OUTDIR            # one instrument; expects OUTDIR/kontakt/NAME.wav and OUTDIR/kontra/NAME.wav
  attribute.py --list LIST.tsv OUTDIR     # name<TAB>nki per line
Options: --midi FILE (default: the base scenario), --json FILE, --r-min 0.95.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import capture  # noqa: E402
import compare  # noqa: E402
import notes  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
RATE = 48000
WINDOW_MS = 200
START_MAX_MS = 100  # the sample may start up to this far into the candidate
TARGET_LEAD_MS = 30  # the played note may begin this far before the candidate's own start
TIMING_MS = 5
R_MIN = 0.95
ZONE_CACHE = Path.home() / ".cache" / "kontra-attr" / "zones"


def zone_table(nki, outdir, keys, vels):
    """KONTRA's zone list for the note set, plus decoded assets (mono f32) in outdir."""
    outdir.mkdir(parents=True, exist_ok=True)
    cmd = ["build-slot", "cargo", "run", "-q", "--release", "-p", "sampler-kontakt", "--example", "zone_samples", "--",
           str(nki), str(outdir), ",".join(map(str, sorted(keys))), ",".join(map(str, sorted(vels)))]
    res = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True)
    if res.returncode != 0:
        raise SystemExit(f"zone_samples failed: {res.stderr[-800:]}")
    zones, assets = {}, {}
    for line in res.stdout.splitlines():
        f = line.split("\t")
        if f[0] == "zone":
            kv = {f[i]: f[i + 1] for i in range(2, len(f) - 1, 2)}
            zones[int(f[1])] = {
                "index": int(f[1]), "group": int(kv["group"]), "low": int(kv["low"]), "high": int(kv["high"]),
                "vlow": int(kv["vlow"]), "vhigh": int(kv["vhigh"]), "root": int(kv["root"]),
                "cents": int(kv["cents"]), "tune": float(kv["tune"]), "asset": int(kv["asset"]),
                "art": int(kv["art"])}
        elif f[0] == "asset" and len(f) > 3 and f[2] == "rate":
            assets[int(f[1])] = {"rate": int(f[3]), "frames": int(f[5])}
    return zones, assets


def pitched(asset_index, zone, key, assets, outdir):
    """The candidate at the played key, resampled to RATE; first ~0.3 s only (start offset + window)."""
    info = assets[asset_index]
    x = np.fromfile(outdir / f"asset{asset_index}.f32", dtype="<f4").astype(np.float64)
    factor = 2.0 ** (zone["tune"] / 12.0)
    if zone["root"] >= 0:
        factor *= 2.0 ** ((key - zone["root"]) * zone["cents"] / 1200.0)
    speed = factor * info["rate"] / RATE
    need = int((START_MAX_MS + WINDOW_MS) / 1000 * RATE) + 1
    positions = np.arange(need) * speed
    positions = positions[positions <= len(x) - 1]
    return np.interp(positions, np.arange(len(x)), x)


def best_match(target, cand):
    """Normalised cross-correlation of target against cand over candidate positions -TARGET_LEAD..START_MAX.

    Returns (r, shift_ms): shift_ms > 0 means the candidate starts that long after the note onset."""
    n = len(target)
    lead = int(TARGET_LEAD_MS / 1000 * RATE)
    span = int(START_MAX_MS / 1000 * RATE)
    padded = np.concatenate([np.zeros(lead), cand, np.zeros(max(0, n + span - len(cand)))])
    m = len(padded)
    size = 1 << int(np.ceil(np.log2(m + n)))
    corr = np.fft.irfft(np.fft.rfft(padded, size) * np.conj(np.fft.rfft(target, size)), size)[: span + lead + 1]
    energy = np.concatenate([[0.0], np.cumsum(padded * padded)])
    seg_energy = energy[n:n + len(corr)] - energy[:len(corr)]
    tnorm = np.sqrt(np.dot(target, target)) + 1e-30
    r = corr / (tnorm * np.sqrt(np.maximum(seg_energy, 1e-30)))
    q = int(np.argmax(r))
    return float(r[q]), (q - lead) * 1000 / RATE


def picks(audio, onsets, zones, assets, outdir, r_min):
    """Best zone per note: list of dicts with r, zone, asset, shift."""
    out = []
    mono = audio
    n = int(WINDOW_MS / 1000 * RATE)
    for (on_s, key, vel) in onsets:
        start = int(round(on_s * RATE))
        target = mono[start:start + n]
        if len(target) < n:
            target = np.pad(target, (0, n - len(target)))
        best = {"r": -1.0, "zone": None, "asset": None, "shift_ms": None}
        cands = [z for z in zones.values()
                 if z["low"] <= key <= z["high"] and z["vlow"] <= vel <= z["vhigh"] and z["asset"] in assets]
        for z in cands:
            cand = pitched(z["asset"], z, key, assets, outdir)
            r, shift = best_match(target, cand)
            if r > best["r"]:
                best = {"r": round(r, 4), "zone": z["index"], "asset": z["asset"], "shift_ms": round(shift, 2)}
        best["pick"] = best["zone"] if best["r"] >= r_min else None
        best["candidates"] = len(cands)
        out.append(best)
    return out


def classify(k, r, zones):
    """Why KONTRA's note differs from Kontakt's (k, r are per-note picks)."""
    if k["pick"] is None or r["pick"] is None:
        return "unidentified"
    if k["pick"] == r["pick"]:
        return "timing only" if abs((k["shift_ms"] or 0) - (r["shift_ms"] or 0)) > TIMING_MS else "match"
    kz, rz = zones[k["pick"]], zones[r["pick"]]
    if kz["asset"] == rz["asset"]:
        return "same sample, other zone"
    if kz["group"] != rz["group"] and (kz["low"], kz["high"]) == (rz["low"], rz["high"]):
        return "wrong articulation group"
    if (kz["low"], kz["high"]) != (rz["low"], rz["high"]):
        return "wrong key zone"
    if (kz["vlow"], kz["vhigh"]) != (rz["vlow"], rz["vhigh"]):
        return "wrong velocity layer"
    return "wrong sample (same zone geometry)"


def run_one(nki, name, outdir, midi, r_min, json_path=None):
    kontakt_wav = outdir / "kontakt" / f"{name}.wav"
    kontra_wav = outdir / "kontra" / f"{name}.wav"
    # Note timings within the slot from the scenario; the slot start in the file comes from capture.json.
    scenario = notes.parse_midi(midi)
    cap = json.loads((outdir / "kontakt" / "capture.json").read_text())[name]
    slot_start = cap.get("slot_start_in_wav_s", 0.5)
    onsets = [(slot_start + on, key, vel) for on, _off, key, vel in scenario]
    keys = {k for _, k, _ in onsets}
    vels = {v for _, _, v in onsets}
    zones, assets = zone_table(nki, ZONE_CACHE / name, keys, vels)
    kontakt = compare.load(str(kontakt_wav))
    kontra = compare.load(str(kontra_wav))
    kontakt = kontakt.mean(axis=0) if kontakt.ndim > 1 else kontakt
    kontra = kontra.mean(axis=0) if kontra.ndim > 1 else kontra
    kp = picks(kontakt, onsets, zones, assets, ZONE_CACHE / name, r_min)
    rp = picks(kontra, onsets, zones, assets, ZONE_CACHE / name, r_min)
    rows = []
    for i, ((_, key, vel), k, r) in enumerate(zip(onsets, kp, rp)):
        rows.append({"note": i, "key": key, "vel": vel, "kontakt": k, "kontra": r,
                     "class": classify(k, r, zones)})
    counts = {}
    for row in rows:
        counts[row["class"]] = counts.get(row["class"], 0) + 1
    result = {"name": name, "notes": rows, "counts": counts,
              "zones": {str(z["index"]): z for z in zones.values()}}
    if json_path:
        json_path.write_text(json.dumps(result, indent=1))
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("inputs", nargs="*", help="NKI NAME OUTDIR, or OUTDIR with --list")
    ap.add_argument("--list", metavar="LIST.tsv")
    ap.add_argument("--midi", help="scenario MIDI (default: the base scenario)")
    ap.add_argument("--r-min", type=float, default=R_MIN)
    ap.add_argument("--json", type=Path)
    args = ap.parse_args()
    midi = Path(args.midi) if args.midi else capture.write_scenario("base", None)
    if args.list:
        if len(args.inputs) != 1:
            ap.error("--list LIST.tsv OUTDIR")
        outdir = Path(args.inputs[0])
        summary = {}
        for line in Path(args.list).read_text().splitlines():
            if not line.strip():
                continue
            name, nki = line.split("\t")[:2]
            res = run_one(Path(nki), name, outdir, midi, args.r_min,
                          outdir / "attr" / f"{name}.json")
            summary[name] = res["counts"]
            print(name, res["counts"], flush=True)
        (outdir / "attr" / "summary.json").write_text(json.dumps(summary, indent=1))
        totals = {}
        for c in summary.values():
            for k, v in c.items():
                totals[k] = totals.get(k, 0) + v
        print("TOTAL", totals)
        return 0
    if len(args.inputs) != 3:
        ap.error("NKI NAME OUTDIR")
    nki, name, outdir = Path(args.inputs[0]), args.inputs[1], Path(args.inputs[2])
    res = run_one(nki, name, outdir, midi, args.r_min, args.json)
    for row in res["notes"]:
        print(row)
    print(res["counts"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
