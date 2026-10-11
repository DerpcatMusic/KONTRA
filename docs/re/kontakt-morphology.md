# Morphology filter: cached dispatch and output table

Status: implementation blocked at the [MCP evidence gate](kontakt-fx-2-evidence.md).

Docs first: Kontakt 8 [Filter Reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference),
sections Ladder high-pass, band-pass and peak/notch filters; KSP
[Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters), filter type constants.

Historical binary leads: reader `0x140d00db0`, saved-ID conversion
`0x140cf4c80`, subtype clamp `0x140af01d0`, wrapper `0x1408faa30`,
parameter setter `0x140b05c70`, coefficient clock `0x140b036a0`,
four-stage ladder audio `0x140b00b80`. Reader contains versions 0x92 and
0x95; saved types 30–41 become internal 22–33 and ladder selectors 0–11.
Saved 13 remains a separate Phaser filter (`0x140ae9f90`), not this ladder.

The output table at `0x144fcebc0` has five columns: input, then stages 1–4.

| Saved type | Output weights |
| --- | --- |
| 30 | 0, 1, 0, 0, 0 |
| 31 | 0, 0, 1, 0, 0 |
| 32 | 0, 0, 0, 1, 0 |
| 33 | 0, 0, 0, 0, 1 |
| 34 | 1, -1, 0, 0, 0 |
| 35 | 1, -2, 1, 0, 0 |
| 36 | 1, -3, 3, -1, 0 |
| 37 | 1, -4, 6, -4, 1 |
| 38 | 0, -2, 2, 0, 0 |
| 39 | 0, 0, 6, -12, 6 |
| 40 | 1, 0, 0, 0, 0 |
| 41 | 1, -4, 12, -16, 8 |

Thus the missing 35, 39 and 41 outputs require weighted combinations of
the shared four-stage state. Row 40's input tap does not prove a bypass:
feedback still affects the input signal. Resonance maps to
q = 1 − (1 − r)². Compensation uses q² and per-selector piecewise knot
tables at `0x144fcecb0`, values at `0x144fced70`, reciprocal widths at
`0x144fceec0`; reusing LP compensation for every row would be incorrect.
Within a knot interval t, s = (1 − t)², w = 1 − (2 − s)s;
w becomes w + |yᵢ|((2 − t)t − w), and the gain is
2yᵢ + 2(yᵢ₊₁ − yᵢ)w. This is a cached interpretation pending MCP confirmation.

Confirmation: read-only PE reads reproduce all 60 output-table floats.
For input/stages (1, 1/2, 1/4, 1/8, 1/16), rows 35, 39 and 41 give
1/4, 3/8 and 1/2 before compensation. These are table arithmetic checks,
not audio parity tests. Record-version field order, the type-13 kernel,
native clamp/smoothing behavior and a production vector test remain open.
