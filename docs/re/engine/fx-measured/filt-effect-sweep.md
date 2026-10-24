# Filters/Effect native sweep (Kontakt 8, 2026-10-24)

Source: `fxrig.py sweep` on `Filters/Effect` (84 steps: cutoff grid at resonance 0, resonance 0.5 and 0.9 at cutoff 0.5 for
SV LP1/HP1/BP2/BP4/LP4/HP4/Notch4/LP6, and the legacy 1/4/6-pole types). Table: `filt-effect-sweep.tsv`
(bypass-referenced dB at 30..15000 Hz, LL pair), spec: `filt-effect-sweep.spec.json`.

Caveat, not yet resolved: several rows read exactly -165.66 or -123.9 dB at every probe (for example `SV_LP4 cutoff=0.25`,
`SV_HP4 cutoff=0.5 res=0.5`). A flat floor at every frequency means the output was zero, so those rows are a capture or
subtype-selection problem, not a response. They must not be used as golden data until re-run with a longer window. The
other rows are plausible (a LP1 at cutoff 0.5 is -1.7 dB at 500 Hz and -30 dB at 15 kHz).

Not compared against the model yet. Parity rows are unchanged.
