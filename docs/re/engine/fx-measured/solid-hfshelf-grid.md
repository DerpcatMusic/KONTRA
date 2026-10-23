# Solid G-EQ HF shelf: native grid at freq 0.4-0.6, gain 0.75-1.0 (2026-10-24)

Data: `solid-hfshelf-grid.tsv` (15 steps, `fxrig.py sweep`, bypass-referenced LL dB at 30..15000 Hz).

Result: at gain 0.75 and 0.875 the response is a plain high shelf (DC ~0 dB, HF +8 and +16 dB). At gain 1.0 a
low-frequency cut appears whose DC depth is non-monotone in freq:

| freq | DC (30 Hz) | 500 Hz | 2 kHz | 15 kHz |
|---|---|---|---|---|
| 0.40 | -11.6 | -11.2 | +5.3 | +18.2 |
| 0.45 | -26.9 | -25.0 | +4.4 | +18.3 |
| 0.50 | -15.7 | -16.1 | +3.5 | +18.4 |
| 0.55 | -8.5 | -8.6 | +2.7 | +18.4 |
| 0.60 | -3.6 | -3.7 | +2.0 | +18.5 |

The model's HF shelf (`solid.rs` `high_shelf` at `C_6138` freq law, plus the `peak` at `C_6BB0`) has DC 0 dB at every
setting. No parameter law tried so far reproduces a DC cut that is deepest at 0.45 and shallower on both sides, so
the fit is blocked: the native section that makes the cut is not identified. Next: measure gain 1.0 at freq 0.42,
0.44, 0.46, 0.48 (and the same at HF gain 0.95) to see whether the cut is a second shelf with its own freq law or an
interaction of the two HF biquads, then fit that structure. HF shelf stays open; Solid G-EQ stays partial.

## Follow-up grids (same day)

`solid-hfshelf-grid2.tsv` (gain 1.0 at freq 0.42, 0.44, 0.46, 0.48; gain 0.90/0.95 at 0.45; freq-0.45 gain-1.0 with bell 0) and
`solid-hfshelf-grid3.tsv` (gain 0.90 and 0.95 at freq 0.40, 0.44, 0.48, 0.52).

The 30 Hz magnitude `A = 10^(DC/20)` shows the low-frequency cut is a zero crossing of the DC gain in the (freq, gain)
plane, not a resonance:
- gain 1.00: `A` = 0.262, 0.174, 0.088, 0.045, 0.0038 (at 0.46), 0.082, 0.166, 0.376, 0.66 for freq 0.40, 0.42, 0.44, 0.45, 0.46, 0.48, 0.50, 0.55, 0.60. Linear in freq on both sides: `|DC| ~ 4.35 |freq - 0.460|`.
- gain 0.95: `A` = 0.094 (0.40), 0.068 (0.44), 0.116 (0.45), 0.227 (0.48), 0.38 (0.52). `|DC| ~ 3.8 |freq - 0.42|`.
- gain 0.90: DC is positive (`~0.62` at 0.40 up to `0.99` at 0.52), slope about 3.1 per unit freq.

So the native DC gain changes sign between gain 0.90 and 0.95 at freq 0.40, and its zero moves with gain (0.46 at gain 1, 0.42 at gain 0.95). A model that puts the cut in a separate section cannot reproduce this without the shelf's DC term. Blocker: the native DC law `H_DC(freq, gain)` must be recovered (the DC term of the HF biquad pair, `solid.rs` `high_shelf` + `peak`), then fitted. Not fitted; Solid G-EQ stays partial.
