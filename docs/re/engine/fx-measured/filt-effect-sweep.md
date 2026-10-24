# Filters/Effect native sweep (Kontakt 8, 2026-10-24)

Source: `fxrig.py sweep` on `Filters/Effect` (84 steps: cutoff grid at resonance 0, resonance 0.5 and 0.9 at cutoff 0.5 for
SV LP1/HP1/BP2/BP4/LP4/HP4/Notch4/LP6, and the legacy 1/4/6-pole types). Table: `filt-effect-sweep.tsv`
(bypass-referenced dB at 30..15000 Hz, LL pair), spec: `filt-effect-sweep.spec.json`.

Fixed (2026-10-24): the silent rows were the MIDI channel bug (fixed by oracle-slot / fxrig channel block). The 23 rows that read
-100 dB or lower were rerun (`filt-effect-sweep-rerun.spec.json`, `oracle-slot`) and replaced in `filt-effect-sweep.tsv`.
Their responses are plausible (for example SV HP4 cutoff 0.5 at resonance 0.5 has a peak near 1.5 kHz; SV LP4 cutoff 0.25 is -3.6 dB at
30 Hz and -52 dB at 15 kHz). The HP at cutoff 1.0 reads -124 dB below 1 kHz: a genuine high-pass at the top of the range, not a capture
fault.

Not compared against the model yet. Parity rows are unchanged.
