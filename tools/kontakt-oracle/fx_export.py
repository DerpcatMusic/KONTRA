#!/usr/bin/env python3
"""Cuts oracle renders into gate cases: fx_export.py DIR NPZ EFFECT PREFIX I:K=V,K=V [I:K=V ...]

Writes DIR/<PREFIX><i>.in.f32 (the bypassed instance, the effect's input) and .oracle.f32 (instance i) from 0.1 s
before the reference's first sound, and appends `<PREFIX><i>|EFFECT|k=v,...` to DIR/cases.txt. Each instance comes
as an argument `i:k=v,k=v` (the settings that model needs; run `cargo test -p kontra-format-kontakt fit -- --ignored`
with KONTRA_FIT=DIR, then tools/kontakt-oracle/fx_gate.py DIR/<name>.model.f32 DIR/<name>.oracle.f32).

"""
import sys
from pathlib import Path

import numpy as np

out, npz, effect, prefix = Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
z = np.load(npz)
rate = int(z["rate"])
ref = z["x0"]
i0 = int(np.argmax(np.abs(ref) > 1e-4)) - int(0.1 * rate)
out.mkdir(parents=True, exist_ok=True)
with open(out / "cases.txt", "a") as cases:
    for arg in sys.argv[5:]:
        i, keys = arg.split(":", 1)
        name = f"{prefix}{i}"
        ref[i0:].astype("<f4").tofile(out / f"{name}.in.f32")
        z[f"x{i}"][i0:].astype("<f4").tofile(out / f"{name}.oracle.f32")
        cases.write(f"{name}|{effect}|{keys}\n")
