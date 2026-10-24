# Solid G-EQ: Kontakt 8 native sweep (2026-10-24)

Source: `tools/kontakt-oracle/fxrig.py sweep` (one instance per setting, own init script; the reference instance is
bypassed). Table: `solid-geq.tsv` (90 steps; the HP-and-LP step is skipped). Converted with `geq_golden.py`.
Test: `crates/sampler-core/src/dsp/eq_family/solid.rs::native_sweep_table_shape`.

| Section | Worst error vs native | Status |
|---|---|---|
| LF shelf | 0.03 dB | inside 0.5 dB |
| LF bell | 0.04 dB | inside 0.5 dB |
| LMF (freq, Q, gain) | 0.03 dB | inside 0.5 dB |
| HMF (freq, Q, gain) | 0.03 dB | inside 0.5 dB |
| HF bell | 0.03 dB | inside 0.5 dB |
| HP (2 sections) | 0.05 dB | inside 0.5 dB |
| LP (2 sections) | 0.03 dB | inside 0.5 dB |
| HF shelf | 17.2 dB (39 probes over 0.5 dB) | **open** |

HF shelf: at freq 0.25 the low end is +1.8 dB, at 0.5 it is -15.7 dB (30 Hz to 1 kHz), at 0.75 it is 0 dB. The
model's shelf does not produce that non-monotone low-frequency cut at mid freq. Not fitted yet; the cut needs a
second native measurement at freq 0.4 to 0.6 and gain 0.75 to 1.0 before a law can be fitted.

Earlier pasted native rows in `solid.rs` (`sections_match_native_probe_responses`) came from a sequential run and
differ from this table for some LMF/HF rows (e.g. LMF freq 0.25 q 0). This table is the per-instance run; the older
rows were not re-checked.
