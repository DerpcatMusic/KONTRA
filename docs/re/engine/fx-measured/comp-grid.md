# Kontakt Compressor (Effects/Compressor, COMP_TYPE=1 Enhanced) threshold and ratio grid (2026-10-24)

Rig: `tools/kontakt-oracle/fxrig.py levels` through `oracle-slot` (slot pool, not `--exclusive`). 1 kHz tone, 7 levels
(-40..0 dBFS, 1 s each), bypassed reference in the same session. Attack knob 100000, release 300000 (fixed).
Data: `comp-grid.tsv` (step, name, level, reference dB, output dB, delta dB), spec `comp-grid.spec.json`.
Insert check: every setting moves the output against the reference (deltas 0 to -49 dB), so the insert is applied.

## Grids

- Threshold knob 0 / 0.25 / 0.5 / 0.75 / 1.0 at ratio knob 0.5.
- Ratio knob 0 / 0.25 / 0.5 / 0.75 / 1.0 at threshold knob 0.5.

## Fit (`enhanced_matches_native_levels`, `crates/sampler-core/src/dsp/dynamics/tests.rs`)

Each setting fitted to the Enhanced law with (threshold dB, saved ratio) on a 2 dB x 0.05 grid, against the 7 native deltas:

| setting | threshold dB | saved ratio | RMS (dB) |
|---|---|---|---|
| thr 0.25 | -60 | 0.30 | 0.35 |
| thr 0.50 | -42 | 0.25 | 0.40 |
| thr 0.75 | -26 | 0.25 | 0.13 |
| thr 1.00 | -10 | 0.60 | 0.00 |
| ratio 0.00 | -40 | 1.00 | 0.01 |
| ratio 0.25 | -42 | 0.50 | 0.16 |
| ratio 0.75 | -42 | 0.10 | 0.65 |
| ratio 1.00 | -42 | 0.05 | 0.64 |
| thr 0.00 | (grid edge -80) | 0.20 | 6.98, outlier |

Gate: RMS <= 1 dB for the 8 rows in the test. The threshold-knob-0 row is excluded. Its native output is nearly flat at
about -72 dBFS for inputs -40 to -12 dBFS (delta -21 to -49 dB), so the law's shape does not reproduce it. Kept as data.

## Knob maps (measured, not yet wired to a live path)

- Threshold dB against knob k: -60 (0.25), -42 (0.5), -26 (0.75), -10 (1.0), close to -74 + 64k. The 0.25 point is 2 dB
  off that line.
- Saved ratio against ratio knob r: 1.0 (0), 0.5 (0.25), 0.25 (0.5), 0.1 (0.75), 0.05 (1.0). Slope term
  `1/(ratio+0.13) - 0.884` grows from 0 at r=0 to about 4.7 at r=1, so the knob is not linear in the saved ratio.

Not yet done: the live KSP `set_engine_par` path has no compressor map in the code (`sampler-kontakt` consumes saved
records only), so these maps are measured data, not engine behaviour. Attack and release are not measured here; the
rig holds them fixed. The 0.25 s steady window is not verified against a 2 s tone yet.
