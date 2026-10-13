#!/usr/bin/env python3
"""Diff KONTRA's translation against Kontakt's own post-load state.

    tools/kontakt-oracle/diff.py NATIVE.jsonl OURS.json [--examples N]

NATIVE.jsonl: one record from dump.py. OURS.json: the ir_json example's output for
the same .nki. Zones and groups are compared by index (Kontakt's order). Every
mismatch is counted per field, with a few examples, so a systematic parser or
translation error shows up as one large count instead of one anecdote.
"""
import argparse
import json
import math
from collections import Counter, defaultdict

TOL_DB = 0.05
TOL_ST = 0.005


def db(gain):
    if gain is None:
        return None
    lin = gain.get("Linear") if isinstance(gain, dict) else gain
    if lin is None and isinstance(gain, dict) and "Decibels" in gain:
        return gain["Decibels"]
    # Kontakt's dB is 6 dB per doubling (probes/volume_law.lua), not 20 log10.
    return -math.inf if not lin else 6 * math.log2(lin)


def semitones(p):
    if not isinstance(p, dict):
        return p
    if "Semitones" in p:
        return p["Semitones"]
    if "Ratio" in p:
        return 12 * math.log2(p["Ratio"]) if p["Ratio"] > 0 else None
    if "Cents" in p:
        return p["Cents"] / 100
    return None


def pan_percent(p):
    # KONTRA stores pan as -1..1 position; Kontakt's Lua API reports -100..100.
    return None if p is None else p.get("position", 0) * 100


def sample_tail(path):
    # Kontakt: D:\...\Samples\<<<UC005.nkx>>>\Samples\x.ncw  KONTRA: /.../UC005.nkx/Samples/x.ncw
    if not path:
        return None
    return path.replace("\\", "/").replace("<<<", "").replace(">>>", "").rsplit("/", 2)[-2:]


class Diff:
    def __init__(self, examples):
        self.count = Counter()
        self.checked = Counter()
        self.examples = defaultdict(list)
        self.limit = examples

    def eq(self, field, where, native, ours, close=None):
        self.checked[field] += 1
        same = close(native, ours) if close and native is not None and ours is not None else native == ours
        if not same:
            self.count[field] += 1
            if len(self.examples[field]) < self.limit:
                self.examples[field].append((where, native, ours))


def near(tol):
    return lambda a, b: abs(a - b) <= tol


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("native")
    ap.add_argument("ours")
    ap.add_argument("--examples", type=int, default=3)
    a = ap.parse_args()
    native = json.loads(open(a.native).readline())
    ours = json.load(open(a.ours))
    print("input:", native["input"])
    if "load_error" in native or "load_error" in ours:
        print("load errors: native", native.get("load_error"), "ours", ours.get("load_error"))
        return
    k, o = native["instrument"], ours["instrument"]
    assets = o["assets"]
    d = Diff(a.examples)

    d.eq("group.count", "instrument", k["num_groups"], len(o["groups"]))
    for kg, og in zip(k["groups"], o["groups"]):
        w = f"group {kg['index']} {kg['name']!r}"
        d.eq("group.name", w, kg["name"], og["name"])
        d.eq("group.volume_db", w, kg["volume_db"], db(og["gain"]), near(TOL_DB))
        d.eq("group.tune", w, kg["tune"], semitones(og["tune"]), near(TOL_ST))
        d.eq("group.pan", w, kg["pan"], pan_percent(og["pan"]), near(0.5))
        d.eq("group.start_conditions", w, len(kg["start_options"] or []), len(og["start"]))
        d.eq("group.muted_vs_playable", w, False, og["muted"])

    hv = o.get("host_volume") or {}
    d.eq("instrument.volume_db", "instrument", k["volume_db"], db(hv.get("saved", 1.0)), near(TOL_DB))
    # KONTRA folds instrument volume/pan into every zone (topology T01); compare
    # zone values net of that fold so a per-zone error is not hidden by it.
    fold_db = db(hv.get("saved", 1.0)) if hv else 0.0
    d.eq("zone.count", "instrument", k["num_zones"], len(o["zones"]))
    for kz, oz in zip(k["zones"], o["zones"]):
        g = kz["geometry"]
        w = f"zone {kz['index']}"
        d.eq("zone.group", w, kz["group"], oz["group"])
        d.eq("zone.low_key", w, g["low_key"], oz["keys"]["low"])
        d.eq("zone.high_key", w, g["high_key"], oz["keys"]["high"])
        d.eq("zone.low_velocity", w, g["low_velocity"], oz["velocities"]["low"])
        d.eq("zone.high_velocity", w, g["high_velocity"], oz["velocities"]["high"])
        f = oz.get("fades") or {}
        d.eq("zone.low_key_fade", w, g["low_key_fade"], f.get("key_in"))
        d.eq("zone.high_key_fade", w, g["high_key_fade"], f.get("key_out"))
        d.eq("zone.low_velocity_fade", w, g["low_velocity_fade"], f.get("velocity_in"))
        d.eq("zone.high_velocity_fade", w, g["high_velocity_fade"], f.get("velocity_out"))
        tracked = (oz.get("pitch") or {}).get("Tracked") if isinstance(oz.get("pitch"), dict) else None
        if tracked:  # an untracked zone plays at its sample pitch; its root has no effect
            d.eq("zone.root_key", w, g["root_key"], tracked["root"])
        d.eq("zone.tune", w, kz["tune"], semitones(oz["tune"]), near(TOL_ST))
        d.eq("zone.volume_db", w, kz["volume_db"], db(oz["gain"]) - fold_db, near(TOL_DB))
        d.eq("zone.pan(+instrument pan)", w, kz["pan"] + k["pan"], pan_percent(oz["pan"]), near(0.5))
        asset = assets[oz["asset"]] if isinstance(oz.get("asset"), int) and oz["asset"] < len(assets) else {}
        loc = (asset.get("location") or {}).get("Path")
        d.eq("zone.sample", w, sample_tail(kz["sample"]), sample_tail(loc))
        pb = oz.get("playback") or {}
        d.eq("zone.sample_start", w, kz["sample_start"], pb.get("start"))
        # Kontakt reports end <= 0 as an offset from the sample's last frame.
        end = kz["sample_end"]
        if end is not None and end <= 0 and kz.get("sample_frames"):
            end = kz["sample_frames"] + end if end < 0 else None
        d.eq("zone.sample_end", w, end, pb.get("end"))
        d.eq("zone.sample_start_mod_range", w, max(kz["sample_start_mod_range"], 0), pb.get("start_range"))
        native_loops = [l for l in kz["loops"] if l["mode"] != "off"]
        looping = pb.get("looping")
        slots = [x for x in (looping.get("Slots") or []) if x] if isinstance(looping, dict) else []
        d.eq("zone.loop_count", w, len(native_loops), len(slots))
        for n, (kl, ol) in enumerate(zip(native_loops, slots)):
            r = ol["range"]
            d.eq("zone.loop_start", f"{w} loop {n}", kl["start"], r["start"])
            d.eq("zone.loop_end", f"{w} loop {n}", kl["start"] + kl["length"], r["end"])
            d.eq("zone.loop_alternating", f"{w} loop {n}", kl["mode"].endswith("_alt"), r["alternating"])
            d.eq("zone.loop_count_value", f"{w} loop {n}", kl["count"], ol["count"])
            d.eq("zone.loop_until_release", f"{w} loop {n}", kl["mode"].startswith("until_release"), ol["until_release"])

    width = max(len(f) for f in d.checked)
    print(f"\n{'field':<{width}}  mismatches / checked")
    for field in d.checked:
        n = d.count[field]
        mark = "" if n == 0 else "   <-- " + "; ".join(
            f"{w}: kontakt={nv!r} kontra={ov!r}" for w, nv, ov in d.examples[field])
        print(f"{field:<{width}}  {n:>6} / {d.checked[field]:<6}{mark}")
    total = sum(d.count.values())
    print(f"\n{total} mismatches across {sum(d.checked.values())} checked values")


if __name__ == "__main__":
    main()
