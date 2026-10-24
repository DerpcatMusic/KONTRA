#!/usr/bin/env python3
"""Re-extract an fxrig sweep with the capture test's window (peak-centred), for comparison with the model.

    sweep_peak.py SPEC.json OUT_PREFIX [--wavs DIR]

Reads OUT_PREFIX_c*.wav written by `fxrig.py sweep` (instance 0 = bypassed reference, instance i at FIRST + i*SLOT).
For each instance: the response peak is searched in the first second after the onset, then 16384 samples are taken from
256 before that peak (the window `cascade.rs` `sv_responses_match_the_kontakt_capture` uses). Output: bypass-referenced
dB at the ten probes, same columns as `fxrig.py sweep`, to OUT_PREFIX.peak.tsv.
"""
import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import fxrig  # noqa: E402
import wavutil  # noqa: E402

N = 16384
PRE = 256
PROBES = fxrig.PROBES


def window(x, rate, onset):
    m = np.abs(x)
    lo = int(onset * rate)
    span = int(fxrig.SLOT * rate)  # the instance's own slot (the rig's sound starts about 1 s after its note)
    start = lo + int(np.argmax(m[lo:lo + span] > 1e-4))  # first sample above threshold after onset
    seg = m[start:start + rate]
    peak = start + int(np.argmax(seg))
    first = peak - PRE
    out = np.zeros(N)
    src = x[max(first, 0):first + N]
    out[max(0, -first):max(0, -first) + len(src)] = src
    return out


def main():
    spec_path, prefix = Path(sys.argv[1]), Path(sys.argv[2])
    spec = json.loads(spec_path.read_text())
    rows = []
    freqs = np.fft.rfftfreq(16384, 1 / 48000)
    for wav in sorted(prefix.parent.glob(prefix.name + "_c*.wav"), key=lambda p: int(p.stem.rsplit("_c", 1)[1])):
        c0 = int(wav.stem.rsplit("_c", 1)[1]) * fxrig.CHUNK
        ch, rate, (L, R) = wavutil.read(wav)
        x = np.array(L)
        ref = window(x, rate, fxrig.FIRST)
        ref_db = 20 * np.log10(np.abs(np.fft.rfft(ref)) + 1e-12)
        for i, st in enumerate(spec["steps"][c0:c0 + fxrig.CHUNK], start=1):
            act = window(x, rate, fxrig.FIRST + i * fxrig.SLOT)
            h = 20 * np.log10(np.abs(np.fft.rfft(act)) + 1e-12) - ref_db
            rows.append([c0 + i, st["name"]] + [f"{np.interp(p, freqs, h):.2f}" for p in PROBES])
    out = prefix.with_name(prefix.name + ".peak.tsv")
    out.write_text("\n".join("\t".join(map(str, r)) for r in rows) + "\n")
    print(out)


if __name__ == "__main__":
    main()
