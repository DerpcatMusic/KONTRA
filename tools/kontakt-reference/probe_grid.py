#!/usr/bin/env python3
"""Sample-identity probe grid. Usage: probe_grid.py OUTDIR KEY,KEY,KEY [CC1=64] [KS=KEY]

Writes OUTDIR/scen.txt (for scenario.py), OUTDIR/grid.json (for `sampler-native identify-kontakt`) and OUTDIR/v1.args
(`--notes` / `--cc` values for `kontakto render`, timeline at half spacing: pass v1.wav*0.5 to identify). Grid: per key, velocities 20 60 100 127, 4 repeats each, notes held 0.5 s
every 2 s. KS= taps that keyswitch key once before the first note (articulation under test).
"""
import json, os, sys

out, keys = sys.argv[1], [int(k) for k in sys.argv[2].split(",")]
opt = dict(a.split("=") for a in sys.argv[3:])
cc1, ks, mode = int(opt.get("CC1", 64)), opt.get("KS"), opt.get("MODE", "grid")
VELS, REPS, LEN, GAP, T0 = (20, 60, 100, 127), 4, 0.5, 2.0, 1.0
os.makedirs(out, exist_ok=True)
scen = ["# probe grid"] + [f"0 cc {n} {v}" for n, v in ((1, cc1), (7, 127), (10, 64), (11, 127), (64, 0))]
cc = [f"{n}@0:{v}" for n, v in ((1, cc1), (7, 127), (10, 64), (11, 127), (64, 0))]
notes, v1, t = [], [], T0
if mode == "legato":
    # overlapping stepwise phrases (A held while B starts) at 3 velocities, two keys: tests transition samples and their starts
    t = T0
    for base in keys[:2]:
        for vel in (40, 80, 120):
            for rep, step in enumerate((0, 2, 4, 2, 0)):
                scen.append(f"{t} note {base + step} {vel} 1.3"); v1.append(f"{base + step}@{int(t*1000)}-{int((t+1.3)*1000)}:{vel}")
                notes.append({"key": base + step, "vel": vel, "rep": rep, "t": t}); t += 1.0
            t += 3.0
    scen.append(f"{t + 1} end")
    open(f"{out}/scen.txt", "w").write("\n".join(scen) + "\n")
    json.dump({"notes": notes, "cc1": cc1, "ks": ks, "scale": 1.0}, open(f"{out}/grid.json", "w"))
    open(f"{out}/v1.args", "w").write(",".join(v1) + "\n" + ",".join(cc) + "\n")
    print(f"{len(notes)} notes, {t - T0:.0f} s"); sys.exit()
if ks:
    scen.append(f"0.3 note {ks} 100 0.2"); v1.append(f"{ks}@300-500:100")
for key in keys:
    for vel in VELS:
        for rep in range(REPS):
            scen.append(f"{t} note {key} {vel} {LEN}")
            t1 = T0 + (t - T0) / 2  # v1 stops a render at 60 s: half-speed timeline, identify gets *0.5
            v1.append(f"{key}@{int(t1*1000)}-{int((t1+LEN)*1000)}:{vel}")
            notes.append({"key": key, "vel": vel, "rep": rep, "t": t}); t += GAP
scen.append(f"{t + 1} end")
open(f"{out}/scen.txt", "w").write("\n".join(scen) + "\n")
json.dump({"notes": notes, "cc1": cc1, "ks": ks, "scale": 0.5}, open(f"{out}/grid.json", "w"))
open(f"{out}/v1.args", "w").write(",".join(v1) + "\n" + ",".join(cc) + "\n")
print(f"{len(notes)} notes, {t - T0:.0f} s")
