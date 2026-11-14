# Replika Delay, measured (Kontakt 8.13.1, factory `Effects/RDelay`, 2026-11-14)

Tools: `tools/kontakt-oracle/fx_lab.py` (render an effect at several settings, keep raw audio) and `fx_rdl.py` (analysis).
Knobs must be set late, from a note callback, and `RDL_TIME_UNIT` 0 before `RDL_TIME` (INIT is tempo-synced).

Impulse renders, TIME raw 100000 (about 200 ms), FEEDBACK 0 unless noted. The dry signal passes along with the echoes.

| Type | First echo | Level vs dry |
|---|---|---|
| Modern | 197.2 ms | +0.3 dB |
| Tape | about 190.8 ms | -7.9 dB |
| Vintage | about 190.5 ms | -4.7 dB |
| Diffusion | smeared cluster 159-238 ms | not measured |
| Analogue | no clear echo near 200 ms | unexplained |

Modern, feedback .6: repeats at -1.6 / -5.4 / -8.2 / -11.7 dB, about 191 / 399 / 591 / 799 ms.

Not reached: DEPTH and RATE. The click-train render (15 settings) gave constant delays for most settings and 12-20 ms
swings for a few, with a 20 Hz alias in the rate estimate; the analysis is not trustworthy yet. No model is fitted, so
no dB error is claimed. The engine still models Modern only (`crates/format/kontakt/src/replika.rs`).
