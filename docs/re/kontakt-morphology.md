# Morphology filter: cached dispatch and output table

Status (round 3): saved ladder types 30–41 now select the verified native
output rows; type 13 is a separate legacy effect filter and remains under extraction.

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

## Round 3 implementation and confirmation

The cache is accepted protocol step 2 for SHA `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Read `complete-export/pseudocode.c` functions `FUN_140cf4c80`,
`FUN_140af01d0`, `FUN_140b05c70`, `FUN_140b00b80`. radare2 MCP opened
`/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`; hexdumps
`0x144fcebc0..0x144fcef50` confirm all output and compensation rows.
Disassembly `0x140cf4c80..0x140cf4ca4` confirms saved-ID subtraction;
`0x140b00ffc..0x140b01049` confirms compensation interpolation;
`0x140b01283..0x140b012c8` confirms the weighted sum order.

Reused the v1-derived four-pole state, correction, soft clipping, and
32/4-frame control scheduling. Added HP1–4, BP2/4, Peak and Notch output
rows with their own extracted compensation. Existing LP compensation stays
unchanged. IR validation and importer now accept selectors 1–12.
`morphology_hp2_bp4_notch_use_native_weights_and_compensation` failed first
(HP2 expected 47/192, received LP4 47/768), then passed. Independent
first-sample vectors at g=1, no feedback, unit input: HP2 47/192,
BP4 47/128, Notch 47/96; endpoint compensation at q=1: .4/.38/.38.
These are extracted-law vectors, not Kontakt recordings. HQ and version
0x95 secondary cutoff scheduling remain diagnosed by the existing code.

## Saved type 13 (Phaser filter), round 3

NI Kontakt 8 [Filter Reference: Phaser](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference)
documents cutoff and resonance, with no built-in LFO. This is not the legacy Phaser effect.
Same-SHA Ghidra `FUN_1407c9a70` prepares the Taylor polynomial constants,
`FUN_14095d770` sets coefficients and `FUN_140909810` runs four allpasses.
radare2 confirms the constructor immediates at 0x1407c9b19–0x1407c9b41,
coefficient mapping at 0x14095d8a7–0x14095d8f5 and all four stages at
0x1409098b3–0x14090994e. Constants: cutoff factor 3 (0x1447149d0),
square factor 9 (0x1447149fc), double mapping .05431093007467753,
.32999999999999996, .02 (0x144f388f0/0x144f38968/0x144f388b0),
feedback .85 (0x144f389b0). All hexdumps confirmed via radare2 on the
same SHA cited above. Positive cutoff evaluates the sixth-degree exponential
polynomial; zero gives frequency .02 and a=(1-.02)/(1+.02).

Each stage is y=a(x+previous_y)-previous_x. Input adds last stage's previous
output times .85*Resonance; output is half the sum of dry and final stage.
Focused failing-first `effects::tests::phaser_filter_native_four_stage_impulse`
checks the first four f32 impulse samples from the extracted recurrence:
.926062942, -.136394471, -.114671811, -.095316768 (and right channel -.5 times these).
Static saved controls are supported; live filter clock is still open.
Morphology labels now name HP/BP/Peak/Notch instead of showing every selector as LP4.
